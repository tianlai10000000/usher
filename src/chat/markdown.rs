use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(super) const MAX_INLINE_PROJECTION_CHARS: usize = 1_200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AssistantProjection {
    pub text: String,
    pub preview_available: bool,
    pub preview_title_key: Option<&'static str>,
    pub markdown: bool,
}

pub(super) fn project_assistant_content(content: &str) -> AssistantProjection {
    let markdown = looks_like_markdown(content);
    let readable = if markdown {
        markdown_to_plain_text(content)
    } else {
        content.to_string()
    };
    let (text, truncated) = bounded_text(readable.trim_end());
    AssistantProjection {
        text,
        preview_available: markdown || truncated,
        preview_title_key: if markdown {
            Some("preview.open_formatted_response")
        } else if truncated {
            Some("preview.open_full_response")
        } else {
            None
        },
        markdown,
    }
}

/// Detect content that benefits from the shared formatted Preview surface.
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

    trimmed.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line.starts_with("+ ")
            || line.starts_with("> ")
            || ordered_list_prefix(line)
    })
}

fn markdown_to_plain_text(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut output = String::with_capacity(markdown.len().min(MAX_INLINE_PROJECTION_CHARS));
    let mut list_depth = 0usize;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                push_block_break(&mut output);
                output.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                output.push_str("- ");
            }
            Event::Text(text) | Event::Code(text) => output.push_str(&text),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Rule => {
                push_block_break(&mut output);
                output.push_str("---");
                output.push('\n');
            }
            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::TableRow,
            ) => push_block_break(&mut output),
            _ => {}
        }
    }
    output.trim().to_string()
}

fn push_block_break(output: &mut String) {
    if output.is_empty() || output.ends_with("\n\n") {
        return;
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
}

fn bounded_text(content: &str) -> (String, bool) {
    let count = content.chars().count();
    if count <= MAX_INLINE_PROJECTION_CHARS {
        return (content.to_string(), false);
    }

    const ELLIPSIS: &str = "...";
    let retained = MAX_INLINE_PROJECTION_CHARS - ELLIPSIS.chars().count();
    let mut text = content.chars().take(retained).collect::<String>();
    let trimmed_len = text.trim_end().len();
    text.truncate(trimmed_len);
    text.push_str(ELLIPSIS);
    (text, true)
}

fn ordered_list_prefix(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && line[digits..].starts_with(". ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_projection_is_readable_and_opens_formatted_preview() {
        let projection =
            project_assistant_content("# Result\n\nUse **cargo check**.\n\n- one\n- two");
        assert_eq!(
            projection.text,
            "Result\n\nUse cargo check.\n\n- one\n\n- two"
        );
        assert!(projection.preview_available);
        assert!(projection.markdown);
        assert_eq!(
            projection.preview_title_key,
            Some("preview.open_formatted_response")
        );
    }

    #[test]
    fn long_projection_is_unicode_safe_and_bounded() {
        let projection = project_assistant_content(&"中".repeat(1_400));
        assert_eq!(projection.text.chars().count(), MAX_INLINE_PROJECTION_CHARS);
        assert!(projection.preview_available);
        assert!(!projection.markdown);
        assert_eq!(
            projection.preview_title_key,
            Some("preview.open_full_response")
        );
    }

    #[test]
    fn ordinary_plain_text_does_not_offer_a_preview() {
        let projection = project_assistant_content("plain response");
        assert_eq!(projection.text, "plain response");
        assert!(!projection.preview_available);
        assert_eq!(projection.preview_title_key, None);
    }
}
