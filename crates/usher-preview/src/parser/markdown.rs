use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

#[cfg(test)]
use std::cell::Cell;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{CodeParserRegistry, ParserLoadStrategy};
use crate::{
    CancellationToken, MarkdownBlock, MarkdownBlockKind, MarkdownDocument, MarkdownInlineKind,
    MarkdownInlineSpan, PreviewError,
};

#[cfg(test)]
thread_local! {
    static MARKDOWN_PARSER_LOADER_INITIALIZATIONS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn record_markdown_parser_loader_initialization() {
    MARKDOWN_PARSER_LOADER_INITIALIZATIONS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
fn markdown_parser_loader_initializations() -> usize {
    MARKDOWN_PARSER_LOADER_INITIALIZATIONS.with(Cell::get)
}

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
        #[cfg(test)]
        record_markdown_parser_loader_initialization();
        Self {
            code_parsers: Arc::new(CodeParserRegistry::with_builtins(strategy)),
        }
    }

    pub fn new(code_parsers: CodeParserRegistry) -> Self {
        #[cfg(test)]
        record_markdown_parser_loader_initialization();
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
        parse_document(
            markdown,
            fallback_title,
            cancellation,
            Some(&self.code_parsers),
        )
    }
}

static DEFAULT_MARKDOWN_PARSER: LazyLock<Arc<dyn MarkdownParser>> =
    LazyLock::new(|| MarkdownParserLoader::with_builtins(ParserLoadStrategy::OnDemand).load());

pub fn parse_markdown(markdown: &str, fallback_title: &str) -> MarkdownDocument {
    DEFAULT_MARKDOWN_PARSER
        .parse(markdown, fallback_title, &CancellationToken::default())
        .expect("non-cancellable Markdown parsing cannot be cancelled")
}

/// Parse Markdown structure without loading or invoking a code highlighter.
///
/// Fenced code remains represented as [`MarkdownBlockKind::CodeBlock`] with
/// its language and source text intact, while `code_spans` stays empty. This is
/// intended for lightweight inline projections; the full Preview path should
/// continue to use [`parse_markdown`].
pub fn parse_markdown_structure(markdown: &str, fallback_title: &str) -> MarkdownDocument {
    parse_markdown_structure_with_cancellation(
        markdown,
        fallback_title,
        &CancellationToken::default(),
    )
    .expect("non-cancellable Markdown structure parsing cannot be cancelled")
}

/// Cancellable variant of [`parse_markdown_structure`].
pub fn parse_markdown_structure_with_cancellation(
    markdown: &str,
    fallback_title: &str,
    cancellation: &CancellationToken,
) -> Result<MarkdownDocument, PreviewError> {
    parse_document(markdown, fallback_title, cancellation, None)
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

#[derive(Clone, Copy)]
struct ListState {
    next_number: Option<u64>,
}

fn parse_document(
    markdown: &str,
    fallback_title: &str,
    cancellation: &CancellationToken,
    code_parsers: Option<&CodeParserRegistry>,
) -> Result<MarkdownDocument, PreviewError> {
    cancellation.check()?;
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut blocks = Vec::new();
    let mut current: Option<PendingBlock> = None;
    let mut list_stack = Vec::<ListState>::new();
    let mut strong = false;
    let mut emphasis = false;
    let mut strikethrough = false;
    let mut link_href: Option<String> = None;
    let mut in_image = false;

    let finalize = |blocks: &mut Vec<MarkdownBlock>, current: &mut Option<PendingBlock>| {
        if let Some(mut pending) = current.take() {
            if pending.kind != MarkdownBlockKind::CodeBlock {
                for span in &mut pending.spans {
                    if let Cow::Owned(normalized) = normalize_model_prose_escapes(&span.text) {
                        span.text = normalized;
                    }
                }
                normalize_residual_strong_markers(&mut pending.spans);
                pending.text.clear();
                pending
                    .text
                    .extend(pending.spans.iter().flat_map(|span| span.text.chars()));
            }
            let mut text = pending.text.trim_end().to_string();
            if pending.kind == MarkdownBlockKind::TableRow {
                text = text.trim_end_matches('|').trim_end().to_string();
            }
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
            Event::Start(Tag::List(start)) => list_stack.push(ListState { next_number: start }),
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                finalize(&mut blocks, &mut current);
                let mut item = PendingBlock::new(MarkdownBlockKind::ListItem);
                let marker = list_stack
                    .last_mut()
                    .and_then(|state| {
                        let number = state.next_number?;
                        state.next_number = Some(number.saturating_add(1));
                        Some(format!("{number}. "))
                    })
                    .unwrap_or_else(|| "• ".to_string());
                item.push(
                    MarkdownInlineKind::Text,
                    "",
                    &format!(
                        "{}{}",
                        "  ".repeat(list_stack.len().saturating_sub(1)),
                        marker
                    ),
                );
                current = Some(item);
            }
            Event::End(TagEnd::Item) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::Table(_)) => finalize(&mut blocks, &mut current),
            Event::Start(Tag::TableHead) => {
                finalize(&mut blocks, &mut current);
                current = Some(PendingBlock::new(MarkdownBlockKind::TableRow));
            }
            Event::End(TagEnd::TableHead) => finalize(&mut blocks, &mut current),
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
            Event::Start(Tag::Strikethrough) => strikethrough = true,
            Event::End(TagEnd::Strikethrough) => strikethrough = false,
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
                } else if strikethrough {
                    MarkdownInlineKind::Strikethrough
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

    if let Some(code_parsers) = code_parsers {
        for block in &mut blocks {
            if block.kind == MarkdownBlockKind::CodeBlock {
                block.code_spans =
                    code_parsers.highlight(&block.text, &block.code_language, cancellation)?;
            }
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
                    | MarkdownBlockKind::Heading4
                    | MarkdownBlockKind::Heading5
                    | MarkdownBlockKind::Heading6
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
        HeadingLevel::H3 => MarkdownBlockKind::Heading3,
        HeadingLevel::H4 => MarkdownBlockKind::Heading4,
        HeadingLevel::H5 => MarkdownBlockKind::Heading5,
        HeadingLevel::H6 => MarkdownBlockKind::Heading6,
    }
}

/// Models occasionally emit a second Markdown escape layer after transport
/// decoding. pulldown-cmark consumes the first layer, leaving artifacts such as
/// `\_` and `\[` in prose. Remove only residual escapes before ASCII Markdown
/// punctuation. Fenced code and link targets bypass this path; inline code is
/// normalized because model-generated identifiers commonly carry this extra
/// transport escape layer.
fn normalize_model_prose_escapes(text: &str) -> Cow<'_, str> {
    if !text
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'\\' && pair[1].is_ascii_punctuation())
    {
        return Cow::Borrowed(text);
    }

    let mut normalized = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && characters
                .peek()
                .is_some_and(|next| next.is_ascii_punctuation())
        {
            continue;
        }
        normalized.push(character);
    }
    Cow::Owned(normalized)
}

/// Valid strong delimiters are consumed by pulldown-cmark. When a model emits
/// a misplaced pair such as `Cargo.toml** consistency**`, both delimiter runs
/// survive as ordinary text. Remove only paired residual runs; a single literal
/// `**` remains visible and fenced code never passes through this normalization.
fn normalize_residual_strong_markers(spans: &mut [MarkdownInlineSpan]) {
    let marker_count = spans
        .iter()
        .filter(|span| span.kind == MarkdownInlineKind::Text)
        .map(|span| span.text.matches("**").count())
        .sum::<usize>();
    if marker_count < 2 {
        return;
    }
    for span in spans
        .iter_mut()
        .filter(|span| span.kind == MarkdownInlineKind::Text)
    {
        if span.text.contains("**") {
            span.text = span.text.replace("**", "");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn structure_parse_matches_full_markdown_semantics_without_code_tokens() {
        let markdown = "# Result\n\nUse **cargo check** and `Cargo.toml`.\n\n- one\n- two\n\n> note\n\n```usher-unknown\nalpha < beta\n```";
        let structure = parse_markdown_structure(markdown, "Fallback");
        let mut highlighted = parse_markdown(markdown, "Fallback");
        for block in &mut highlighted.blocks {
            block.code_spans.clear();
        }

        assert_eq!(structure, highlighted);
        assert!(
            structure
                .blocks
                .iter()
                .filter(|block| block.kind == MarkdownBlockKind::CodeBlock)
                .all(|block| block.code_spans.is_empty())
        );
    }

    #[test]
    fn structure_parse_never_reports_code_parser_loading() {
        let progress_count = Arc::new(AtomicUsize::new(0));
        let cancellation = CancellationToken::default();
        let observed = progress_count.clone();
        let loader_initializations_before = markdown_parser_loader_initializations();
        cancellation.set_progress_handler(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        });

        let document = parse_markdown_structure_with_cancellation(
            "```typescript\nconst value: number = 1;\n```",
            "Code",
            &cancellation,
        )
        .expect("structure parse");

        assert_eq!(progress_count.load(Ordering::Relaxed), 0);
        assert_eq!(
            markdown_parser_loader_initializations(),
            loader_initializations_before
        );
        assert_eq!(document.blocks.len(), 1);
        assert!(document.blocks[0].code_spans.is_empty());
    }

    #[test]
    fn structure_parse_observes_pre_cancelled_token_even_for_empty_input() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();

        assert_eq!(
            parse_markdown_structure_with_cancellation("", "Fallback", &cancellation).unwrap_err(),
            PreviewError::Cancelled
        );
    }

    #[test]
    fn model_double_escapes_are_removed_from_prose_but_preserved_in_fenced_code() {
        let document = parse_markdown_structure(
            r#"Use \\[dependencies\\], `literal\\_code`, and i-slint-backend-\\*.

```text
literal\\_block
```"#,
            "Escapes",
        );

        assert_eq!(
            document.blocks[0].text,
            "Use [dependencies], literal_code, and i-slint-backend-*."
        );
        assert_eq!(document.blocks[1].text, r"literal\\_block");
    }

    #[test]
    fn paired_residual_strong_markers_are_hidden_but_single_literals_remain() {
        let document = parse_markdown_structure(
            "2. portable-pty** 是否被正确引入**\n\n4. Cargo.toml** 与 Cargo.lock 的一致性**\n\nKeep one literal value** marker.",
            "Markers",
        );

        assert_eq!(document.blocks[0].text, "2. portable-pty 是否被正确引入");
        assert_eq!(
            document.blocks[1].text,
            "3. Cargo.toml 与 Cargo.lock 的一致性"
        );
        assert_eq!(document.blocks[2].text, "Keep one literal value** marker.");
    }

    #[test]
    fn table_rows_do_not_keep_the_parser_cell_delimiter_suffix() {
        let document = parse_markdown_structure(
            "| Check | Expected |\n| --- | --- |\n| Markdown | Preserved |",
            "Table",
        );

        assert_eq!(
            document
                .blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            vec!["Check | Expected", "Markdown | Preserved"]
        );
    }

    #[test]
    fn all_heading_levels_and_strikethrough_keep_their_semantics() {
        let document = parse_markdown_structure(
            "# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6\n\n~~removed~~",
            "Headings",
        );

        assert_eq!(
            document
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![
                MarkdownBlockKind::Heading1,
                MarkdownBlockKind::Heading2,
                MarkdownBlockKind::Heading3,
                MarkdownBlockKind::Heading4,
                MarkdownBlockKind::Heading5,
                MarkdownBlockKind::Heading6,
                MarkdownBlockKind::Paragraph,
            ]
        );
        assert_eq!(
            document.blocks[6].spans[0].kind,
            MarkdownInlineKind::Strikethrough
        );
    }

    #[test]
    fn ordered_and_nested_lists_keep_readable_markers() {
        let document =
            parse_markdown_structure("3. third\n4. fourth\n\n- outer\n  - inner", "Lists");

        assert_eq!(document.blocks[0].text, "3. third");
        assert_eq!(document.blocks[1].text, "4. fourth");
        assert_eq!(document.blocks[2].text, "• outer");
        assert_eq!(document.blocks[3].text, "  • inner");
    }
}
