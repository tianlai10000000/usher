//! AI Review's Slint adapter for framework-neutral agent-response projections.

use std::rc::Rc;

use slint::{ModelRc, VecModel};
use usher_preview::{MarkdownBlock, MarkdownBlockKind, MarkdownInlineKind};

use crate::agent_response::AgentResponseProjection;
use crate::{AgentResponseBlock, AgentResponseBlockKind, AiReviewConversationItem};

pub(super) const RESPONSE_VARIANT_ENV: &str = "USHER_AI_REVIEW_RESPONSE_VARIANT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResponseVariant {
    Baseline,
    Formatted,
}

#[derive(Clone)]
pub(super) struct PreparedAgentResponse {
    blocks: Vec<PreparedAgentResponseBlock>,
    preview_title: String,
    preview_available: bool,
    truncated: bool,
    projected_bytes: i32,
    repository_action_count: i32,
}

#[derive(Clone)]
struct PreparedAgentResponseBlock {
    kind: AgentResponseBlockKind,
    styled: slint::StyledText,
    plain_text: String,
    list_marker: String,
    list_depth: i32,
    code_language: String,
    span_count: i32,
    line_count: i32,
}

impl ResponseVariant {
    pub(super) fn from_environment() -> Self {
        if std::env::var(RESPONSE_VARIANT_ENV).as_deref() == Ok("baseline") {
            Self::Baseline
        } else {
            Self::Formatted
        }
    }

    pub(super) const fn is_formatted(self) -> bool {
        matches!(self, Self::Formatted)
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Formatted => "formatted",
        }
    }
}

pub(super) fn conversation_item(
    message_id: &str,
    role: &str,
    content: &str,
    elapsed: &str,
    projection: Option<PreparedAgentResponse>,
) -> AiReviewConversationItem {
    let formatted = projection.is_some() && role != "user";
    let (
        blocks,
        preview_title,
        preview_available,
        truncated,
        projected_bytes,
        repository_action_count,
    ) = projection.map_or_else(
        || (ModelRc::default(), String::new(), false, false, 0, 0),
        |projection| {
            let blocks = projection
                .blocks
                .into_iter()
                .map(PreparedAgentResponseBlock::into_ui_block)
                .collect::<Vec<_>>();
            (
                ModelRc::new(Rc::new(VecModel::from(blocks))),
                projection.preview_title,
                projection.preview_available,
                projection.truncated,
                projection.projected_bytes,
                projection.repository_action_count,
            )
        },
    );

    AiReviewConversationItem {
        id: message_id.into(),
        role: role.into(),
        content: content.into(),
        elapsed: elapsed.into(),
        formatted,
        blocks,
        preview_title: preview_title.into(),
        preview_available,
        truncated,
        projected_bytes,
        repository_action_count,
    }
}

pub(super) fn plain_conversation_item(
    message_id: &str,
    role: &str,
    content: &str,
    elapsed: &str,
) -> AiReviewConversationItem {
    conversation_item(message_id, role, content, elapsed, None)
}

/// Prepares Slint's inline styled-text values on the same worker that owns the
/// framework-neutral Markdown projection. The event loop only materializes the
/// final ModelRc and commits it to the component tree.
pub(super) fn prepare_projection(
    projection: &AgentResponseProjection,
    repository_actions_enabled: bool,
) -> PreparedAgentResponse {
    PreparedAgentResponse {
        blocks: projection
            .blocks
            .iter()
            .map(|block| prepare_block(block, repository_actions_enabled))
            .collect(),
        preview_title: projection.title.clone(),
        preview_available: projection.has_rich_markdown(),
        truncated: projection.truncated,
        projected_bytes: projection.approximate_bytes.min(i32::MAX as usize) as i32,
        repository_action_count: if repository_actions_enabled {
            projection
                .repository_file_actions
                .len()
                .min(i32::MAX as usize) as i32
        } else {
            0
        },
    }
}

pub(super) fn prepare_plain_projection(
    projection: &AgentResponseProjection,
) -> PreparedAgentResponse {
    PreparedAgentResponse {
        blocks: vec![PreparedAgentResponseBlock {
            kind: AgentResponseBlockKind::Paragraph,
            styled: slint::StyledText::from_plain_text(&projection.plain_text),
            plain_text: projection.plain_text.clone(),
            list_marker: String::new(),
            list_depth: 0,
            code_language: String::new(),
            span_count: 1,
            line_count: projection
                .plain_text
                .lines()
                .count()
                .max(1)
                .min(i32::MAX as usize) as i32,
        }],
        preview_title: projection.title.clone(),
        preview_available: projection.has_rich_markdown(),
        truncated: projection.truncated,
        projected_bytes: projection.approximate_bytes.min(i32::MAX as usize) as i32,
        repository_action_count: 0,
    }
}

fn prepare_block(
    block: &MarkdownBlock,
    repository_actions_enabled: bool,
) -> PreparedAgentResponseBlock {
    let (display_block, list_marker, list_depth) = list_presentation(block);
    PreparedAgentResponseBlock {
        kind: match block.kind {
            MarkdownBlockKind::Heading1 => AgentResponseBlockKind::Heading1,
            MarkdownBlockKind::Heading2 => AgentResponseBlockKind::Heading2,
            MarkdownBlockKind::Heading3 => AgentResponseBlockKind::Heading3,
            MarkdownBlockKind::Heading4 => AgentResponseBlockKind::Heading4,
            MarkdownBlockKind::Heading5 => AgentResponseBlockKind::Heading5,
            MarkdownBlockKind::Heading6 => AgentResponseBlockKind::Heading6,
            MarkdownBlockKind::Paragraph => AgentResponseBlockKind::Paragraph,
            MarkdownBlockKind::ListItem => AgentResponseBlockKind::ListItem,
            MarkdownBlockKind::Quote => AgentResponseBlockKind::Quote,
            MarkdownBlockKind::CodeBlock => AgentResponseBlockKind::CodeBlock,
            MarkdownBlockKind::Rule => AgentResponseBlockKind::Rule,
            MarkdownBlockKind::TableRow => AgentResponseBlockKind::TableRow,
        },
        styled: styled_text_for(&display_block, repository_actions_enabled),
        plain_text: display_block.text,
        list_marker,
        list_depth,
        code_language: block.code_language.clone(),
        span_count: block.spans.len().min(i32::MAX as usize) as i32,
        line_count: block.text.lines().count().max(1).min(i32::MAX as usize) as i32,
    }
}

impl PreparedAgentResponseBlock {
    fn into_ui_block(self) -> AgentResponseBlock {
        AgentResponseBlock {
            kind: self.kind,
            styled: self.styled,
            plain_text: self.plain_text.into(),
            list_marker: self.list_marker.into(),
            list_depth: self.list_depth,
            code_language: self.code_language.into(),
            span_count: self.span_count,
            line_count: self.line_count,
        }
    }
}

fn list_presentation(block: &MarkdownBlock) -> (MarkdownBlock, String, i32) {
    let mut display = block.clone();
    if block.kind != MarkdownBlockKind::ListItem {
        return (display, String::new(), 0);
    }

    let Some(first) = display.spans.first_mut() else {
        return (display, "•".to_string(), 0);
    };
    let Some((prefix_bytes, marker, depth)) = split_list_prefix(&first.text) else {
        return (display, "•".to_string(), 0);
    };
    first.text.replace_range(..prefix_bytes, "");
    if first.text.is_empty() {
        display.spans.remove(0);
    }
    display.text = display
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    (display, marker, depth)
}

fn split_list_prefix(text: &str) -> Option<(usize, String, i32)> {
    let leading_spaces = text.bytes().take_while(|byte| *byte == b' ').count();
    let depth = (leading_spaces / 2).min(i32::MAX as usize) as i32;
    let rest = &text[leading_spaces..];
    let (marker_bytes, mut marker) = if rest.starts_with("• ") {
        ("• ".len(), "•".to_string())
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !rest[digits..].starts_with(". ") {
            return None;
        }
        (digits + 2, rest[..=digits].to_string())
    };
    let mut consumed = leading_spaces + marker_bytes;
    let after_marker = &text[consumed..];
    if after_marker.starts_with("[x] ") || after_marker.starts_with("[X] ") {
        marker = "✓".to_string();
        consumed += 4;
    } else if after_marker.starts_with("[ ] ") {
        marker = "○".to_string();
        consumed += 4;
    }
    Some((consumed, marker, depth))
}

fn styled_text_for(block: &MarkdownBlock, repository_actions_enabled: bool) -> slint::StyledText {
    if matches!(
        block.kind,
        MarkdownBlockKind::CodeBlock | MarkdownBlockKind::TableRow
    ) {
        return slint::StyledText::from_plain_text(&block.text);
    }

    let mut markdown = String::new();
    for span in &block.spans {
        let kind = if !repository_actions_enabled
            && span.kind == MarkdownInlineKind::Link
            && span
                .href
                .starts_with(crate::agent_response::REPOSITORY_FILE_LINK_PREFIX)
        {
            MarkdownInlineKind::Text
        } else {
            span.kind
        };
        match kind {
            MarkdownInlineKind::Text => markdown.push_str(&escape_inline_markdown(&span.text)),
            MarkdownInlineKind::Strong => {
                markdown.push_str("**");
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push_str("**");
            }
            MarkdownInlineKind::Emphasis => {
                markdown.push('*');
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push('*');
            }
            MarkdownInlineKind::Strikethrough => {
                markdown.push_str("~~");
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push_str("~~");
            }
            MarkdownInlineKind::Code => push_inline_code_markdown(&mut markdown, &span.text),
            MarkdownInlineKind::Link => {
                markdown.push('[');
                markdown.push_str(&escape_inline_markdown(&span.text));
                markdown.push_str("](");
                markdown.push_str(&span.href.replace(')', "\\)"));
                markdown.push(')');
            }
        }
    }
    slint::StyledText::from_markdown(&markdown)
        .unwrap_or_else(|_| slint::StyledText::from_plain_text(&block.text))
}

fn push_inline_code_markdown(markdown: &mut String, text: &str) {
    let fence_length = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or_default()
        .saturating_add(1);
    let fence = "`".repeat(fence_length);
    let pad = text.starts_with('`') || text.ends_with('`');
    markdown.push_str(&fence);
    if pad {
        markdown.push(' ');
    }
    markdown.push_str(text);
    if pad {
        markdown.push(' ');
    }
    markdown.push_str(&fence);
}

fn escape_inline_markdown(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '*' | '_' | '[' | ']' | '`') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use slint::Model;

    use super::*;
    use crate::agent_response::{
        DEFAULT_PROJECTION_MAX_BLOCKS, ProjectionLimits, project_agent_response,
    };

    #[test]
    fn inline_code_markdown_does_not_reintroduce_visible_escape_characters() {
        let mut markdown = String::new();
        push_inline_code_markdown(&mut markdown, r"i-slint-backend-* [dev-dependencies]");
        assert_eq!(markdown, "`i-slint-backend-* [dev-dependencies]`");

        markdown.clear();
        push_inline_code_markdown(&mut markdown, "call(`value`)");
        assert_eq!(markdown, "``call(`value`)``");
    }

    #[test]
    fn list_projection_separates_markers_from_wrapped_body_text() {
        let projection = project_agent_response(
            "task:list:0",
            "- outer item\n  - nested item\n\n3. ordered item\n\n- [x] completed item",
            "List response",
            ProjectionLimits::default(),
        );
        let prepared = prepare_projection(&projection, true);
        let item = conversation_item(
            "task:list:0",
            "assistant",
            "raw markdown",
            "1.2s",
            Some(prepared),
        );

        let outer = item.blocks.row_data(0).expect("outer item");
        let nested = item.blocks.row_data(1).expect("nested item");
        let ordered = item.blocks.row_data(2).expect("ordered item");
        let completed = item.blocks.row_data(3).expect("completed item");
        assert_eq!(outer.list_marker, "•");
        assert_eq!(outer.list_depth, 0);
        assert_eq!(outer.plain_text, "outer item");
        assert_eq!(nested.list_marker, "•");
        assert_eq!(nested.list_depth, 1);
        assert_eq!(nested.plain_text, "nested item");
        assert_eq!(ordered.list_marker, "3.");
        assert_eq!(ordered.plain_text, "ordered item");
        assert_eq!(completed.list_marker, "✓");
        assert_eq!(completed.plain_text, "completed item");
    }

    #[test]
    fn formatted_projection_builds_block_model_without_raw_markdown_markers() {
        let projection = project_agent_response(
            "task:1:0",
            "## Result\n\nUse **cargo check** and `AGENTS.md`.\n\n```rust\nfn main() {}\n```",
            "AI review response",
            ProjectionLimits::default(),
        );
        let prepared = prepare_projection(&projection, true);
        let item = conversation_item(
            "task:1:0",
            "assistant",
            "raw markdown",
            "1.2s",
            Some(prepared),
        );

        assert!(item.formatted);
        assert!(item.preview_available);
        assert_eq!(item.blocks.row_count(), 3);
        assert_eq!(
            item.blocks.row_data(0).expect("heading").plain_text,
            "Result"
        );
        assert_eq!(
            item.blocks.row_data(2).expect("code").kind,
            AgentResponseBlockKind::CodeBlock
        );
    }

    #[test]
    fn truncated_inline_projection_preserves_original_markdown_for_preview() {
        let original = (0..DEFAULT_PROJECTION_MAX_BLOCKS + 8)
            .map(|index| format!("Paragraph {index}: 中文 🧪"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let projection = project_agent_response(
            "task:1:bounded",
            original.as_str(),
            "AI review response",
            ProjectionLimits::default(),
        );

        let item = conversation_item(
            "task:1:bounded",
            "assistant",
            original.as_str(),
            "1.2s",
            Some(prepare_projection(&projection, true)),
        );

        assert_eq!(
            item.blocks.row_count(),
            DEFAULT_PROJECTION_MAX_BLOCKS,
            "the inline model must obey the compact block budget"
        );
        assert!(item.truncated);
        assert!(item.preview_available);
        assert_eq!(item.content.as_str(), original);
    }

    #[test]
    fn user_and_baseline_items_never_mount_formatted_blocks() {
        for role in ["user", "assistant"] {
            let item = plain_conversation_item("message", role, "## literal", "");
            assert!(!item.formatted);
            assert_eq!(item.blocks.row_count(), 0);
            assert!(!item.preview_available);
            assert_eq!(item.content, "## literal");
        }
    }
}
