use std::sync::{Arc, LazyLock};

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{CodeParserRegistry, ParserLoadStrategy};
use crate::{
    CancellationToken, MarkdownBlock, MarkdownBlockKind, MarkdownDocument, MarkdownInlineKind,
    MarkdownInlineSpan, PreviewError,
};

pub trait MarkdownParser: Send + Sync {
    fn parse(
        &self,
        markdown: &str,
        fallback_title: &str,
        cancellation: &CancellationToken,
    ) -> Result<MarkdownDocument, PreviewError>;
}

#[derive(Clone)]
pub struct MarkdownParserLoader {
    code_parsers: Arc<CodeParserRegistry>,
}

impl MarkdownParserLoader {
    pub fn with_builtins(strategy: ParserLoadStrategy) -> Self {
        Self {
            code_parsers: Arc::new(CodeParserRegistry::with_builtins(strategy)),
        }
    }

    pub fn new(code_parsers: CodeParserRegistry) -> Self {
        Self {
            code_parsers: Arc::new(code_parsers),
        }
    }

    pub fn load(&self) -> Arc<dyn MarkdownParser> {
        Arc::new(PulldownMarkdownParser {
            code_parsers: self.code_parsers.clone(),
        })
    }
}

struct PulldownMarkdownParser {
    code_parsers: Arc<CodeParserRegistry>,
}

impl MarkdownParser for PulldownMarkdownParser {
    fn parse(
        &self,
        markdown: &str,
        fallback_title: &str,
        cancellation: &CancellationToken,
    ) -> Result<MarkdownDocument, PreviewError> {
        self.code_parsers.prepare(cancellation)?;
        parse_document(markdown, fallback_title, cancellation, &self.code_parsers)
    }
}

static DEFAULT_MARKDOWN_PARSER: LazyLock<Arc<dyn MarkdownParser>> =
    LazyLock::new(|| MarkdownParserLoader::with_builtins(ParserLoadStrategy::OnDemand).load());

pub fn parse_markdown(markdown: &str, fallback_title: &str) -> MarkdownDocument {
    DEFAULT_MARKDOWN_PARSER
        .parse(markdown, fallback_title, &CancellationToken::default())
        .expect("non-cancellable Markdown parsing cannot be cancelled")
}

#[cfg(test)]
pub(crate) fn parse_markdown_inner(
    markdown: &str,
    fallback_title: &str,
    cancellation: Option<&CancellationToken>,
) -> Result<MarkdownDocument, PreviewError> {
    let default_cancellation = CancellationToken::default();
    DEFAULT_MARKDOWN_PARSER.parse(
        markdown,
        fallback_title,
        cancellation.unwrap_or(&default_cancellation),
    )
}

#[derive(Clone)]
struct PendingBlock {
    kind: MarkdownBlockKind,
    text: String,
    spans: Vec<MarkdownInlineSpan>,
    code_language: String,
}

fn parse_document(
    markdown: &str,
    fallback_title: &str,
    cancellation: &CancellationToken,
    code_parsers: &CodeParserRegistry,
) -> Result<MarkdownDocument, PreviewError> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut blocks = Vec::new();
    let mut current: Option<PendingBlock> = None;
    let mut list_depth = 0usize;
    let mut strong = false;
    let mut emphasis = false;
    let mut link_href: Option<String> = None;
    let mut in_image = false;

    let finalize = |blocks: &mut Vec<MarkdownBlock>, current: &mut Option<PendingBlock>| {
        if let Some(pending) = current.take() {
            let text = pending.text.trim_end().to_string();
            if !text.trim().is_empty() {
                blocks.push(MarkdownBlock {
                    kind: pending.kind,
                    text,
                    spans: pending.spans,
                    code_language: pending.code_language,
                    code_spans: Vec::new(),
                });
            }
        }
    };

    for (event_index, event) in Parser::new_ext(markdown, options).enumerate() {
        if event_index % 256 == 0 {
            cancellation.check()?;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(heading_kind(level)));
            }
            Event::End(TagEnd::Heading(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::Paragraph) => {
                current.get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph));
            }
            Event::End(TagEnd::Paragraph) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::BlockQuote(_)) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(MarkdownBlockKind::Quote));
            }
            Event::End(TagEnd::BlockQuote(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::CodeBlock(kind)) => {
                finalize(&mut blocks, &mut current);
                let mut block = PendingBlock::new(MarkdownBlockKind::CodeBlock);
                block.code_language = code_language(&kind);
                current = Some(block);
            }
            Event::End(TagEnd::CodeBlock) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                finalize(&mut blocks, &mut current);
                let mut item = PendingBlock::new(MarkdownBlockKind::ListItem);
                item.push(
                    MarkdownInlineKind::Text,
                    "",
                    &format!("{}• ", "  ".repeat(list_depth.saturating_sub(1))),
                );
                current = Some(item);
            }
            Event::End(TagEnd::Item) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::Table(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::TableRow) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(MarkdownBlockKind::TableRow));
            }
            Event::End(TagEnd::TableRow) => finalize(&mut blocks, &mut current),
            Event::End(TagEnd::TableCell) => {
                if let Some(block) = current.as_mut() {
                    block.push(MarkdownInlineKind::Text, "", " | ");
                }
            }
            Event::Start(Tag::Strong) => strong = true,
            Event::End(TagEnd::Strong) => strong = false,
            Event::Start(Tag::Emphasis) => emphasis = true,
            Event::End(TagEnd::Emphasis) => emphasis = false,
            Event::Start(Tag::Link { dest_url, .. }) => link_href = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link_href = None,
            Event::Start(Tag::Image { dest_url, .. }) => {
                in_image = true;
                link_href = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Image) => {
                in_image = false;
                link_href = None;
            }
            Event::TaskListMarker(checked) => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::ListItem))
                    .push(
                        MarkdownInlineKind::Text,
                        "",
                        if checked { "[x] " } else { "[ ] " },
                    );
            }
            Event::Text(text) => {
                let block =
                    current.get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph));
                let kind = if link_href.is_some() {
                    MarkdownInlineKind::Link
                } else if strong {
                    MarkdownInlineKind::Strong
                } else if emphasis {
                    MarkdownInlineKind::Emphasis
                } else {
                    MarkdownInlineKind::Text
                };
                if in_image {
                    block.push(MarkdownInlineKind::Text, "", "Image: ");
                }
                block.push(kind, link_href.as_deref().unwrap_or(""), &text);
            }
            Event::Code(text) => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph))
                    .push(MarkdownInlineKind::Code, "", &text);
            }
            Event::SoftBreak | Event::HardBreak => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph))
                    .push(MarkdownInlineKind::Text, "", "\n");
            }
            Event::Rule => {
                finalize(&mut blocks, &mut current);
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::Rule,
                    text: String::new(),
                    spans: Vec::new(),
                    code_language: String::new(),
                    code_spans: Vec::new(),
                });
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                current
                    .get_or_insert_with(|| PendingBlock::new(MarkdownBlockKind::Paragraph))
                    .push(MarkdownInlineKind::Text, "", &html);
            }
            _ => {}
        }
    }
    finalize(&mut blocks, &mut current);

    for block in &mut blocks {
        if block.kind == MarkdownBlockKind::CodeBlock {
            block.code_spans =
                code_parsers.highlight(&block.text, &block.code_language, cancellation)?;
        }
    }

    let title = blocks
        .iter()
        .find(|block| {
            matches!(
                block.kind,
                MarkdownBlockKind::Heading1
                    | MarkdownBlockKind::Heading2
                    | MarkdownBlockKind::Heading3
            )
        })
        .map(|block| block.text.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback_title.trim())
        .to_string();
    let title = if title.is_empty() {
        "Markdown document".to_string()
    } else {
        title.chars().take(120).collect()
    };
    let plain_text = blocks
        .iter()
        .filter(|block| block.kind != MarkdownBlockKind::Rule)
        .map(|block| block.text.trim_end())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(MarkdownDocument {
        title,
        plain_text,
        blocks,
    })
}

impl PendingBlock {
    fn new(kind: MarkdownBlockKind) -> Self {
        Self {
            kind,
            text: String::new(),
            spans: Vec::new(),
            code_language: String::new(),
        }
    }

    fn push(&mut self, kind: MarkdownInlineKind, href: &str, text: &str) {
        self.text.push_str(text);
        if let Some(last) = self.spans.last_mut()
            && last.kind == kind
            && last.href == href
        {
            last.text.push_str(text);
            return;
        }
        self.spans.push(MarkdownInlineSpan {
            kind,
            text: text.to_string(),
            href: href.to_string(),
        });
    }
}

fn code_language(kind: &CodeBlockKind<'_>) -> String {
    match kind {
        CodeBlockKind::Indented => String::new(),
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|character| matches!(character, '{' | '}' | '.'))
            .to_ascii_lowercase(),
    }
}

fn heading_kind(level: HeadingLevel) -> MarkdownBlockKind {
    match level {
        HeadingLevel::H1 => MarkdownBlockKind::Heading1,
        HeadingLevel::H2 => MarkdownBlockKind::Heading2,
        _ => MarkdownBlockKind::Heading3,
    }
}
