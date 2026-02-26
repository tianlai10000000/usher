use crate::{
    MarkdownBlock, MarkdownBlockKind, MarkdownInlineKind, MarkdownInlineLine, MarkdownInlineSpan,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

// 微调字符宽度估算：偏保守会导致提前换行和右侧留白，因此这里适度下调。
const WIDTH_TAB_PX: f32 = 14.0;
const WIDTH_SPACE_PX: f32 = 3.8;
const WIDTH_CJK_PX: f32 = 12.4;
const WIDTH_ASCII_PUNCT_PX: f32 = 6.4;
const WIDTH_ASCII_DIGIT_PX: f32 = 7.2;
const WIDTH_ASCII_LETTER_PX: f32 = 7.6;
const WIDTH_OTHER_UNICODE_PX: f32 = 9.8;

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
    build_assistant_render_payload_with_wrap_width(content, None)
}

/// 使用 pulldown-cmark 做 Markdown 解析，支持传入动态换行像素宽度。
pub fn build_assistant_render_payload_with_wrap_width(
    content: &str,
    wrap_width_px: Option<f32>,
) -> AssistantRenderPayload {
    if !looks_like_markdown(content) {
        return AssistantRenderPayload {
            rendered_text: plain_text_to_display(content),
            is_markdown: false,
            blocks: Vec::new(),
        };
    }

    let blocks = markdown_to_blocks(content, wrap_width_px);
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

fn markdown_to_blocks(markdown: &str, wrap_width_px: Option<f32>) -> Vec<MarkdownBlock> {
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
                let max_width = wrap_width_px.unwrap_or_else(|| max_width_for_block_px(p.kind));
                wrap_inline_spans(&p.spans, max_width)
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

fn max_width_for_block_px(kind: MarkdownBlockKind) -> f32 {
    match kind {
        MarkdownBlockKind::Heading1 => 420.0,
        MarkdownBlockKind::Heading2 => 500.0,
        MarkdownBlockKind::Heading3 => 560.0,
        MarkdownBlockKind::CodeBlock => 620.0,
        _ => 760.0,
    }
}

fn wrap_inline_spans(spans: &[MarkdownInlineSpan], max_width_px: f32) -> Vec<MarkdownInlineLine> {
    if spans.is_empty() {
        return Vec::new();
    }

    let mut lines: Vec<Vec<MarkdownInlineSpan>> = Vec::new();
    let mut current_line: Vec<MarkdownInlineSpan> = Vec::new();
    let mut current_width = 0.0f32;
    let max_width_px = max_width_px.max(160.0);

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
                current_width = 0.0;
                continue;
            }

            let char_width = estimated_char_width_px(ch);
            if current_width + char_width > max_width_px && !current_line.is_empty() {
                lines.push(current_line);
                current_line = Vec::new();
                current_width = 0.0;
            }

            append_char_to_line(&mut current_line, &span.kind, &span.href.to_string(), ch);
            current_width += char_width;
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

fn estimated_char_width_px(ch: char) -> f32 {
    if ch == '\t' {
        return WIDTH_TAB_PX;
    }
    if ch.is_whitespace() {
        return WIDTH_SPACE_PX;
    }
    if is_cjk(ch) {
        return WIDTH_CJK_PX;
    }
    if ch.is_ascii_punctuation() {
        return WIDTH_ASCII_PUNCT_PX;
    }
    if ch.is_ascii_digit() {
        return WIDTH_ASCII_DIGIT_PX;
    }
    if ch.is_ascii() {
        return WIDTH_ASCII_LETTER_PX;
    }
    WIDTH_OTHER_UNICODE_PX
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF   // CJK Unified Ideographs Extension A
            | 0x4E00..=0x9FFF // CJK Unified Ideographs
            | 0xF900..=0xFAFF // CJK Compatibility Ideographs
            | 0x3040..=0x30FF // Hiragana + Katakana
            | 0xAC00..=0xD7AF // Hangul Syllables
    )
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
