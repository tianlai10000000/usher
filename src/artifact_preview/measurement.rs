//! Sendable, width-independent input and exact worker-owned Markdown shaping.
use std::borrow::Cow;

use parley::fontique::GenericFamily;
#[cfg(not(test))]
use parley::fontique::SourceCache;
use parley::style::{
    FontFamily, FontFamilyName, FontStyle, FontWeight, LineHeight, OverflowWrap, TextWrapMode,
    WordBreak,
};
use parley::{FontContext, LayoutContext, StyleProperty};

use crate::{ArtifactPreviewBlockKind, ArtifactPreviewInlineKind, ArtifactPreviewInlineSpan};

const MAX_CONTENT_WIDTH: f32 = 800.0;
const WIDE_HORIZONTAL_PADDING: f32 = 48.0;
const NARROW_HORIZONTAL_PADDING: f32 = 32.0;
const NARROW_BREAKPOINT: f32 = 560.0;
const DOCUMENT_TOP_PADDING: f32 = 12.0;
const DOCUMENT_BOTTOM_PADDING: f32 = 32.0;
const GLYPH_SAFETY_HEIGHT: f32 = 4.0;
const BODY_LINE_HEIGHT: f32 = 22.0;
const CODE_LINE_HEIGHT: f32 = 19.0;

#[derive(Clone)]
pub(super) struct MeasurementBlock {
    pub kind: ArtifactPreviewBlockKind,
    pub plain_text: String,
    pub list_depth: i32,
    pub code_language: String,
    pub layout_spans: Vec<ArtifactPreviewInlineSpan>,
}
pub(super) struct Measurer {
    font_context: FontContext,
    layout_context: LayoutContext<()>,
}
impl Default for Measurer {
    fn default() -> Self {
        Self {
            font_context: preview_font_context(),
            layout_context: LayoutContext::new(),
        }
    }
}
#[cfg(not(test))]
fn preview_font_context() -> FontContext {
    FontContext {
        collection: slint::fontique_011::shared_collection(),
        source_cache: SourceCache::default(),
    }
}

#[cfg(test)]
fn preview_font_context() -> FontContext {
    FontContext::new()
}

impl Measurer {
    pub(super) fn collection(&self) -> parley::fontique::Collection {
        self.font_context.collection.clone()
    }
    pub(super) fn with_collection(collection: parley::fontique::Collection) -> Self {
        Self {
            font_context: FontContext {
                collection,
                source_cache: Default::default(),
            },
            layout_context: LayoutContext::new(),
        }
    }
    pub(super) fn measure(
        &mut self,
        blocks: &[MeasurementBlock],
        width: f32,
        font: &str,
        cancelled: impl Fn() -> bool,
    ) -> Option<Vec<u32>> {
        let mut previous = None;
        let mut extents = Vec::with_capacity(blocks.len());
        for (index, block) in blocks.iter().enumerate() {
            if cancelled() {
                return None;
            }
            extents.push(self.block_extent(
                block,
                markdown_block_width(positive(width)),
                font,
                previous,
                index == 0,
                index + 1 == blocks.len(),
            ));
            previous = Some(block.kind);
        }
        (!cancelled()).then_some(extents)
    }
    pub(super) fn block_extent(
        &mut self,
        block: &MeasurementBlock,
        block_width: f32,
        application_font_family: &str,
        previous_kind: Option<ArtifactPreviewBlockKind>,
        first: bool,
        last: bool,
    ) -> u32 {
        let edge_padding = if first { DOCUMENT_TOP_PADDING } else { 0.0 }
            + if last { DOCUMENT_BOTTOM_PADDING } else { 0.0 };
        if block.kind == ArtifactPreviewBlockKind::Rule {
            return to_u32(57.0 + edge_padding).max(1);
        }

        let metrics = block_metrics(block.kind);
        let padding_top = block_padding_top(block.kind, previous_kind, first, &block.code_language);
        if block.kind == ArtifactPreviewBlockKind::CodeBlock {
            let line_count = block.plain_text.as_str().lines().count().max(1) as f32;
            return to_u32(
                line_count * CODE_LINE_HEIGHT + padding_top + metrics.padding_bottom + edge_padding,
            )
            .max(1);
        }
        let content_inset = block_content_inset(block);
        let text_width = if block.kind == ArtifactPreviewBlockKind::ListItem {
            block_width - content_inset
        } else {
            block_width - content_inset * 2.0
        }
        .max(1.0);
        let family = if block.kind == ArtifactPreviewBlockKind::CodeBlock {
            "monospace"
        } else {
            application_font_family
        };
        let text_height = self.measure_text(block, family, metrics.font_size, text_width);
        to_u32(
            text_height + GLYPH_SAFETY_HEIGHT + padding_top + metrics.padding_bottom + edge_padding,
        )
        .max(1)
    }

    fn measure_text(
        &mut self,
        block: &MeasurementBlock,
        family: &str,
        font_size: f32,
        wrap_width: f32,
    ) -> f32 {
        let text = block.plain_text.as_str();
        let families = [
            FontFamilyName::named(family),
            FontFamilyName::Generic(GenericFamily::SansSerif),
            FontFamilyName::Generic(GenericFamily::SystemUi),
        ];
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, text, 1.0, false);
        builder.push_default(FontFamily::List(Cow::Borrowed(&families)));
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::WordBreak(WordBreak::Normal));
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::Wrap));
        if !matches!(
            block.kind,
            ArtifactPreviewBlockKind::Heading1
                | ArtifactPreviewBlockKind::Heading2
                | ArtifactPreviewBlockKind::Heading3
                | ArtifactPreviewBlockKind::Heading4
                | ArtifactPreviewBlockKind::Heading5
                | ArtifactPreviewBlockKind::Heading6
        ) {
            builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(
                BODY_LINE_HEIGHT,
            )));
        }

        if matches!(
            block.kind,
            ArtifactPreviewBlockKind::Heading1
                | ArtifactPreviewBlockKind::Heading2
                | ArtifactPreviewBlockKind::Heading3
                | ArtifactPreviewBlockKind::Heading4
                | ArtifactPreviewBlockKind::Heading5
                | ArtifactPreviewBlockKind::Heading6
        ) {
            builder.push_default(StyleProperty::FontWeight(FontWeight::BOLD));
        }
        for span in block.layout_spans.iter() {
            let start = usize::try_from(span.start.max(0))
                .unwrap_or(0)
                .min(text.len());
            let end = usize::try_from(span.end.max(span.start).max(0))
                .unwrap_or(text.len())
                .min(text.len());
            if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                continue;
            }
            match span.kind {
                ArtifactPreviewInlineKind::Strong => {
                    builder.push(StyleProperty::FontWeight(FontWeight::BOLD), start..end)
                }
                ArtifactPreviewInlineKind::Emphasis => {
                    builder.push(StyleProperty::FontStyle(FontStyle::Italic), start..end)
                }
                ArtifactPreviewInlineKind::Code => builder.push(
                    StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::Generic(
                        GenericFamily::Monospace,
                    ))),
                    start..end,
                ),
                ArtifactPreviewInlineKind::Text
                | ArtifactPreviewInlineKind::Strikethrough
                | ArtifactPreviewInlineKind::Link => {}
            }
        }
        let mut layout = builder.build(text);
        layout.break_all_lines(Some(wrap_width));
        layout.height().max(font_size)
    }
}

#[derive(Clone, Copy)]
struct BlockMetrics {
    font_size: f32,
    padding_bottom: f32,
}

fn block_metrics(kind: ArtifactPreviewBlockKind) -> BlockMetrics {
    match kind {
        ArtifactPreviewBlockKind::Heading1 => BlockMetrics {
            font_size: 24.0,
            padding_bottom: 0.0,
        },
        ArtifactPreviewBlockKind::Heading2 => BlockMetrics {
            font_size: 20.0,
            padding_bottom: 0.0,
        },
        ArtifactPreviewBlockKind::Heading3 => BlockMetrics {
            font_size: 17.0,
            padding_bottom: 0.0,
        },
        ArtifactPreviewBlockKind::Heading4 => BlockMetrics {
            font_size: 15.0,
            padding_bottom: 0.0,
        },
        ArtifactPreviewBlockKind::Heading5 | ArtifactPreviewBlockKind::Heading6 => BlockMetrics {
            font_size: 15.0,
            padding_bottom: 0.0,
        },
        ArtifactPreviewBlockKind::CodeBlock => BlockMetrics {
            font_size: 12.0,
            padding_bottom: 10.0,
        },
        ArtifactPreviewBlockKind::Quote => BlockMetrics {
            font_size: 14.0,
            padding_bottom: 8.0,
        },
        ArtifactPreviewBlockKind::TableRow => BlockMetrics {
            font_size: 12.0,
            padding_bottom: 10.0,
        },
        ArtifactPreviewBlockKind::ListItem => BlockMetrics {
            font_size: 14.0,
            padding_bottom: 0.0,
        },
        ArtifactPreviewBlockKind::Paragraph | ArtifactPreviewBlockKind::Rule => BlockMetrics {
            font_size: 14.0,
            padding_bottom: 0.0,
        },
    }
}

fn block_content_inset(block: &MeasurementBlock) -> f32 {
    match block.kind {
        ArtifactPreviewBlockKind::CodeBlock => 12.0,
        ArtifactPreviewBlockKind::Quote => 20.0,
        ArtifactPreviewBlockKind::TableRow => 8.0,
        ArtifactPreviewBlockKind::ListItem => block.list_depth.clamp(0, 2) as f32 * 24.0 + 24.0,
        _ => 0.0,
    }
}

pub(super) fn block_padding_top(
    kind: ArtifactPreviewBlockKind,
    previous: Option<ArtifactPreviewBlockKind>,
    first: bool,
    code_language: &str,
) -> f32 {
    if is_heading(kind) {
        return 20.0;
    }
    match kind {
        ArtifactPreviewBlockKind::CodeBlock => {
            if code_language.is_empty() {
                10.0
            } else {
                32.0
            }
        }
        ArtifactPreviewBlockKind::Quote => 8.0,
        ArtifactPreviewBlockKind::TableRow => 10.0,
        ArtifactPreviewBlockKind::ListItem => match previous {
            Some(ArtifactPreviewBlockKind::ListItem) | None => 0.0,
            Some(previous)
                if previous == ArtifactPreviewBlockKind::Paragraph || is_heading(previous) =>
            {
                10.0
            }
            Some(ArtifactPreviewBlockKind::CodeBlock) => 14.0,
            Some(ArtifactPreviewBlockKind::Quote) => 8.0,
            Some(_) => 0.0,
        },
        ArtifactPreviewBlockKind::Paragraph => {
            if first {
                return 0.0;
            }
            match previous {
                Some(ArtifactPreviewBlockKind::Paragraph) => 20.0,
                Some(ArtifactPreviewBlockKind::ListItem) => 10.0,
                Some(previous) if is_heading(previous) => 10.0,
                Some(ArtifactPreviewBlockKind::CodeBlock) => 14.0,
                Some(ArtifactPreviewBlockKind::Quote) => 8.0,
                Some(ArtifactPreviewBlockKind::Rule) => 0.0,
                Some(_) | None => 11.0,
            }
        }
        ArtifactPreviewBlockKind::Rule => 28.0,
        _ => 11.0,
    }
}

fn is_heading(kind: ArtifactPreviewBlockKind) -> bool {
    matches!(
        kind,
        ArtifactPreviewBlockKind::Heading1
            | ArtifactPreviewBlockKind::Heading2
            | ArtifactPreviewBlockKind::Heading3
            | ArtifactPreviewBlockKind::Heading4
            | ArtifactPreviewBlockKind::Heading5
            | ArtifactPreviewBlockKind::Heading6
    )
}

fn markdown_block_width(content_width: f32) -> f32 {
    let horizontal_padding = if content_width < NARROW_BREAKPOINT {
        NARROW_HORIZONTAL_PADDING
    } else {
        WIDE_HORIZONTAL_PADDING
    };
    (content_width.min(MAX_CONTENT_WIDTH) - horizontal_padding).max(1.0)
}

fn positive(value: f32) -> f32 {
    if value.is_finite() {
        value.max(1.0)
    } else {
        1.0
    }
}

fn to_u32(value: f32) -> u32 {
    if value.is_finite() {
        value.ceil().clamp(0.0, u32::MAX as f32) as u32
    } else {
        0
    }
}
