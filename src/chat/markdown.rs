use crate::{
    MarkdownBlock, MarkdownBlockKind, MarkdownInlineKind, MarkdownInlineLine, MarkdownInlineSpan,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

pub struct AssistantRenderPayload {
    pub rendered_text: String,
    pub is_markdown: bool,
    pub blocks: Vec<MarkdownBlock>,
}

#[derive(Clone)]
struct PendingBlock {
    kind: MarkdownBlockKind,
    text: String,
    spans: Vec<MarkdownInlineSpan>,
    has_rich_inline: bool,
}

/// 使用 pulldown-cmark 做 Markdown 解析，产出 UI 渲染块。
pub fn build_assistant_render_payload(content: &str) -> AssistantRenderPayload {
    build_assistant_render_payload_with_wrap_cols(content, None)
}

/// 使用 pulldown-cmark 做 Markdown 解析，支持传入动态换行列数。
pub fn build_assistant_render_payload_with_wrap_cols(
    content: &str,
    wrap_cols: Option<usize>,
) -> AssistantRenderPayload {
    if !looks_like_markdown(content) {
        return AssistantRenderPayload {
            rendered_text: plain_text_to_display(content),
            is_markdown: false,
            blocks: Vec::new(),
        };
    }

    let blocks = markdown_to_blocks(content, wrap_cols);
    AssistantRenderPayload {
        rendered_text: plain_text_to_display(content),
        is_markdown: !blocks.is_empty(),
        blocks,
    }
}

/// 检测文本是否“看起来像” Markdown。
pub fn looks_like_markdown(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.contains("```")
        || trimmed.contains('`')
        || trimmed.contains("**")
        || trimmed.contains("__")
        || trimmed.contains("### ")
        || trimmed.contains("## ")
        || trimmed.contains("# ")
        || (trimmed.contains('[') && trimmed.contains("]("))
    {
        return true;
    }

    for line in trimmed.lines() {
        let l = line.trim_start();
        if l.starts_with("- ")
            || l.starts_with("* ")
            || l.starts_with("+ ")
            || l.starts_with("> ")
            || l.starts_with("1. ")
            || l.starts_with("2. ")
            || l.starts_with("3. ")
        {
            return true;
        }
    }

    false
}

/// 纯文本直接用于显示。
pub fn plain_text_to_display(content: &str) -> String {
    content.to_string()
}

fn markdown_to_blocks(markdown: &str, wrap_cols: Option<usize>) -> Vec<MarkdownBlock> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut blocks: Vec<MarkdownBlock> = Vec::new();

    let mut current: Option<PendingBlock> = None;
    let mut list_depth = 0usize;
    let mut in_strong = false;
    let mut in_emphasis = false;
    let mut link_href: Option<String> = None;

    let ensure_block = |current: &mut Option<PendingBlock>, kind: MarkdownBlockKind| {
        if current.is_none() {
            *current = Some(PendingBlock {
                kind,
                text: String::new(),
                spans: Vec::new(),
                has_rich_inline: false,
            });
        }
    };

    let finalize_block = |blocks: &mut Vec<MarkdownBlock>, current: &mut Option<PendingBlock>| {
        if let Some(p) = current.take() {
            let text = p.text.trim_end().to_string();
            if text.trim().is_empty() {
                return;
            }
            let inline_lines = if p.kind == MarkdownBlockKind::CodeBlock {
                Vec::new()
            } else if p.has_rich_inline {
                // 仅在真正存在行内样式时启用 span 渲染。
                // 普通文本块走 Slint Text 自适应换行，避免窗口变宽后仍提前换行。
                let cols = wrap_cols.unwrap_or_else(|| max_cols_for_block(p.kind));
                wrap_inline_spans(&p.spans, cols)
            } else {
                Vec::new()
            };
            blocks.push(MarkdownBlock {
                kind: p.kind,
                text: text.into(),
                inline_lines: inline_lines.as_slice().into(),
            });
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                finalize_block(&mut blocks, &mut current);
                current = Some(PendingBlock {
                    kind: heading_to_kind(level),
                    text: String::new(),
                    spans: Vec::new(),
                    has_rich_inline: false,
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                finalize_block(&mut blocks, &mut current);
            }
            Event::Start(Tag::Paragraph) => {
                if !matches!(
                    current.as_ref().map(|b| b.kind),
                    Some(MarkdownBlockKind::ListItem)
                ) {
                    ensure_block(&mut current, MarkdownBlockKind::Paragraph);
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if matches!(
                    current.as_ref().map(|b| b.kind),
                    Some(MarkdownBlockKind::Paragraph)
                ) || matches!(
                    current.as_ref().map(|b| b.kind),
                    Some(MarkdownBlockKind::Quote)
                ) {
                    finalize_block(&mut blocks, &mut current);
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                finalize_block(&mut blocks, &mut current);
                current = Some(PendingBlock {
                    kind: MarkdownBlockKind::Quote,
                    text: String::new(),
                    spans: Vec::new(),
                    has_rich_inline: false,
                });
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                finalize_block(&mut blocks, &mut current);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                finalize_block(&mut blocks, &mut current);
                current = Some(PendingBlock {
                    kind: MarkdownBlockKind::CodeBlock,
                    text: String::new(),
                    spans: Vec::new(),
                    has_rich_inline: false,
                });
            }
            Event::End(TagEnd::CodeBlock) => {
                finalize_block(&mut blocks, &mut current);
            }
            Event::Start(Tag::List(_)) => {
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
            }
            Event::Start(Tag::Item) => {
                finalize_block(&mut blocks, &mut current);
                current = Some(PendingBlock {
                    kind: MarkdownBlockKind::ListItem,
                    text: String::new(),
                    spans: Vec::new(),
                    has_rich_inline: false,
                });
                let prefix = format!("{}• ", "  ".repeat(list_depth.saturating_sub(1)));
                if let Some(block) = current.as_mut() {
                    push_span_text(block, MarkdownInlineKind::Text, "", &prefix);
                }
            }
            Event::End(TagEnd::Item) => {
                finalize_block(&mut blocks, &mut current);
            }
            Event::Start(Tag::Strong) => in_strong = true,
            Event::End(TagEnd::Strong) => in_strong = false,
            Event::Start(Tag::Emphasis) => in_emphasis = true,
            Event::End(TagEnd::Emphasis) => in_emphasis = false,
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_href = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                link_href = None;
            }
            Event::Text(t) => {
                ensure_block(&mut current, MarkdownBlockKind::Paragraph);
                if let Some(block) = current.as_mut() {
                    let kind = if link_href.is_some() {
                        MarkdownInlineKind::Link
                    } else if in_strong {
                        MarkdownInlineKind::Strong
                    } else if in_emphasis {
                        MarkdownInlineKind::Emphasis
                    } else {
                        MarkdownInlineKind::Text
                    };
                    push_span_text(block, kind, link_href.as_deref().unwrap_or(""), &t);
                }
            }
            Event::Code(t) => {
                ensure_block(&mut current, MarkdownBlockKind::Paragraph);
                if let Some(block) = current.as_mut() {
                    push_span_text(
                        block,
                        MarkdownInlineKind::Code,
                        link_href.as_deref().unwrap_or(""),
                        &t,
                    );
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                ensure_block(&mut current, MarkdownBlockKind::Paragraph);
                if let Some(block) = current.as_mut() {
                    push_span_text(
                        block,
                        if in_strong {
                            MarkdownInlineKind::Strong
                        } else if in_emphasis {
                            MarkdownInlineKind::Emphasis
                        } else {
                            MarkdownInlineKind::Text
                        },
                        link_href.as_deref().unwrap_or(""),
                        "\n",
                    );
                }
            }
            Event::Rule => {
                finalize_block(&mut blocks, &mut current);
                let separator_line = MarkdownInlineLine {
                    spans: [MarkdownInlineSpan {
                        kind: MarkdownInlineKind::Text,
                        text: "--------------------".into(),
                        href: "".into(),
                    }]
                    .as_slice()
                    .into(),
                };
                blocks.push(MarkdownBlock {
                    kind: MarkdownBlockKind::Paragraph,
                    text: "--------------------".into(),
                    inline_lines: [separator_line].as_slice().into(),
                });
            }
            Event::TaskListMarker(checked) => {
                ensure_block(&mut current, MarkdownBlockKind::Paragraph);
                if let Some(block) = current.as_mut() {
                    push_span_text(
                        block,
                        MarkdownInlineKind::Text,
                        "",
                        if checked { "[x] " } else { "[ ] " },
                    );
                }
            }
            _ => {}
        }
    }

    finalize_block(&mut blocks, &mut current);
    blocks
}

fn heading_to_kind(level: HeadingLevel) -> MarkdownBlockKind {
    match level {
        HeadingLevel::H1 => MarkdownBlockKind::Heading1,
        HeadingLevel::H2 => MarkdownBlockKind::Heading2,
        _ => MarkdownBlockKind::Heading3,
    }
}

fn push_span_text(block: &mut PendingBlock, kind: MarkdownInlineKind, href: &str, text: &str) {
    if text.is_empty() {
        return;
    }

    if kind != MarkdownInlineKind::Text {
        block.has_rich_inline = true;
    }

    block.text.push_str(text);
    if let Some(last) = block.spans.last_mut() {
        if last.kind == kind && last.href.as_str() == href {
            let mut merged = last.text.to_string();
            merged.push_str(text);
            last.text = merged.into();
            return;
        }
    }

    block.spans.push(MarkdownInlineSpan {
        kind,
        text: text.into(),
        href: href.into(),
    });
}

fn max_cols_for_block(kind: MarkdownBlockKind) -> usize {
    match kind {
        MarkdownBlockKind::Heading1 => 20,
        MarkdownBlockKind::Heading2 => 24,
        MarkdownBlockKind::Heading3 => 28,
        MarkdownBlockKind::CodeBlock => 36,
        _ => 44,
    }
}

fn wrap_inline_spans(spans: &[MarkdownInlineSpan], max_cols: usize) -> Vec<MarkdownInlineLine> {
    if spans.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<Vec<MarkdownInlineSpan>> = Vec::new();
    let mut current_line: Vec<MarkdownInlineSpan> = Vec::new();
    let mut current_cols = 0usize;

    for span in spans {
        let text = span.text.to_string();
        for ch in text.chars() {
            if ch == '\n' {
                if !current_line.is_empty() {
                    lines.push(current_line);
                    current_line = Vec::new();
                } else {
                    lines.push(Vec::new());
                }
                current_cols = 0;
                continue;
            }

            let char_width = if ch.is_ascii() { 1 } else { 2 };
            if current_cols + char_width > max_cols && !current_line.is_empty() {
                lines.push(current_line);
                current_line = Vec::new();
                current_cols = 0;
            }

            append_char_to_line(&mut current_line, &span.kind, &span.href.to_string(), ch);
            current_cols += char_width;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
        .into_iter()
        .map(|line_spans| MarkdownInlineLine {
            spans: line_spans.as_slice().into(),
        })
        .collect()
}

fn append_char_to_line(
    line: &mut Vec<MarkdownInlineSpan>,
    kind: &MarkdownInlineKind,
    href: &str,
    ch: char,
) {
    if let Some(last) = line.last_mut() {
        if last.kind == *kind && last.href.as_str() == href {
            let mut merged = last.text.to_string();
            merged.push(ch);
            last.text = merged.into();
            return;
        }
    }

    line.push(MarkdownInlineSpan {
        kind: kind.clone(),
        text: ch.to_string().into(),
        href: href.into(),
    });
}
