//! Exact Markdown geometry for the Preview virtual surface.

use std::borrow::Cow;
use std::rc::Rc;

use parley::fontique::GenericFamily;
#[cfg(not(test))]
use parley::fontique::SourceCache;
use parley::style::{
    FontFamily, FontFamilyName, FontStyle, FontWeight, OverflowWrap, TextWrapMode, WordBreak,
};
use parley::{FontContext, LayoutContext, StyleProperty};
use slint::{Model, ModelRc, VecModel};
use usher_virtual_list::{VirtualList, VirtualWindow};

use crate::{
    ArtifactPreviewBlock, ArtifactPreviewBlockKind, ArtifactPreviewInlineKind,
    ArtifactPreviewVirtualRow, ArtifactPreviewVirtualWindow,
};

const MARKDOWN_OVERSCAN_BLOCKS: usize = 3;
const MAX_CONTENT_WIDTH: f32 = 880.0;
const WIDE_HORIZONTAL_PADDING: f32 = 64.0;
const NARROW_HORIZONTAL_PADDING: f32 = 32.0;
const NARROW_BREAKPOINT: f32 = 560.0;
const DOCUMENT_TOP_PADDING: f32 = 20.0;
const DOCUMENT_BOTTOM_PADDING: f32 = 48.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutKey {
    revision: i32,
    content_width_bits: u32,
    font_family: String,
    block_count: usize,
}

pub(super) struct MarkdownVirtualLayoutBridge {
    list: VirtualList,
    layout_key: Option<LayoutKey>,
    window: VirtualWindow,
    rows_model: Rc<VecModel<ArtifactPreviewVirtualRow>>,
    rows: ModelRc<ArtifactPreviewVirtualRow>,
    projected_indices: Vec<usize>,
    content_extent: u64,
    projection_guard: Option<(u64, u64)>,
    projection_commits: u64,
    projection_policy: ProjectionPolicy,
    last_scroll_offset: Option<u64>,
    pending_scroll_activity: Option<(bool, usize)>,
    font_context: FontContext,
    layout_context: LayoutContext<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionPolicy {
    PerScrollEvent,
    BufferedIncremental,
}

impl Default for MarkdownVirtualLayoutBridge {
    fn default() -> Self {
        Self::with_projection_policy(ProjectionPolicy::BufferedIncremental)
    }
}

impl MarkdownVirtualLayoutBridge {
    pub(super) fn from_environment() -> Self {
        let policy =
            if std::env::var("USHER_PREVIEW_SCROLL_PERF_VARIANT").as_deref() == Ok("baseline") {
                ProjectionPolicy::PerScrollEvent
            } else {
                ProjectionPolicy::BufferedIncremental
            };
        Self::with_projection_policy(policy)
    }

    fn with_projection_policy(projection_policy: ProjectionPolicy) -> Self {
        let rows_model = Rc::new(VecModel::default());
        Self {
            list: VirtualList::new(&[], MARKDOWN_OVERSCAN_BLOCKS),
            layout_key: None,
            window: VirtualWindow::default(),
            rows: ModelRc::from(rows_model.clone()),
            rows_model,
            projected_indices: Vec::new(),
            content_extent: 0,
            projection_guard: None,
            projection_commits: 0,
            projection_policy,
            last_scroll_offset: None,
            pending_scroll_activity: None,
            font_context: preview_font_context(),
            layout_context: LayoutContext::new(),
        }
    }
}

#[cfg(not(test))]
fn preview_font_context() -> FontContext {
    FontContext {
        collection: slint::fontique_010::shared_collection(),
        source_cache: SourceCache::default(),
    }
}

#[cfg(test)]
fn preview_font_context() -> FontContext {
    FontContext::new()
}

impl MarkdownVirtualLayoutBridge {
    pub(super) fn layout(
        &mut self,
        revision: i32,
        blocks: &ModelRc<ArtifactPreviewBlock>,
        scroll_offset: f32,
        viewport_extent: f32,
        content_width: f32,
        font_family: &str,
    ) -> ArtifactPreviewVirtualWindow {
        let content_width = positive(content_width);
        let key = LayoutKey {
            revision,
            content_width_bits: content_width.to_bits(),
            font_family: font_family.to_owned(),
            block_count: blocks.row_count(),
        };
        if self.layout_key.as_ref() != Some(&key) {
            let text_width = markdown_block_width(content_width);
            let last_index = blocks.row_count().saturating_sub(1);
            let extents = blocks
                .iter()
                .enumerate()
                .map(|(index, block)| {
                    self.block_extent(
                        &block,
                        text_width,
                        font_family,
                        index == 0,
                        index == last_index,
                    )
                })
                .collect::<Vec<_>>();
            self.list
                .replace_item_extents(&extents, MARKDOWN_OVERSCAN_BLOCKS);
            self.content_extent = extents.iter().map(|extent| u64::from(*extent)).sum();
            self.layout_key = Some(key);
            self.projection_guard = None;
            self.projected_indices.clear();
        }

        let viewport_extent = u64::from(to_u32(viewport_extent).max(1));
        let maximum_offset = self.content_extent.saturating_sub(viewport_extent);
        let scroll_offset = to_u64(scroll_offset).min(maximum_offset);
        let needs_projection = match self.projection_policy {
            ProjectionPolicy::PerScrollEvent => true,
            ProjectionPolicy::BufferedIncremental => self
                .projection_guard
                .is_none_or(|(start, end)| scroll_offset < start || scroll_offset > end),
        };
        if needs_projection {
            self.project_rows(blocks, scroll_offset, viewport_extent);
        }

        let rows = match self.projection_policy {
            ProjectionPolicy::PerScrollEvent => {
                let rows = self.window_rows(blocks);
                ModelRc::new(VecModel::from(rows))
            }
            ProjectionPolicy::BufferedIncremental => self.rows.clone(),
        };

        let output = ArtifactPreviewVirtualWindow {
            rows,
            scroll_offset: scroll_offset.min(f32::MAX as u64) as f32,
            content_height: self.content_extent.min(f32::MAX as u64) as f32,
            projection_revision: self.projection_commits.min(i32::MAX as u64) as i32,
        };
        if self
            .last_scroll_offset
            .is_some_and(|previous| previous != scroll_offset)
        {
            self.pending_scroll_activity = Some((needs_projection, output.rows.row_count()));
        }
        self.last_scroll_offset = Some(scroll_offset);
        output
    }

    pub(super) fn take_scroll_activity(&mut self) -> Option<(bool, usize)> {
        self.pending_scroll_activity.take()
    }

    fn project_rows(
        &mut self,
        blocks: &ModelRc<ArtifactPreviewBlock>,
        scroll_offset: u64,
        viewport_extent: u64,
    ) {
        let (projection_start, projection_end, guard_start, guard_end) =
            match self.projection_policy {
                ProjectionPolicy::PerScrollEvent => {
                    let end = scroll_offset
                        .saturating_add(viewport_extent)
                        .min(self.content_extent);
                    (scroll_offset, end, scroll_offset, scroll_offset)
                }
                ProjectionPolicy::BufferedIncremental => {
                    let end = scroll_offset
                        .saturating_add(viewport_extent)
                        .min(self.content_extent);
                    (scroll_offset, end, scroll_offset, scroll_offset)
                }
            };
        let projection_extent = projection_end
            .saturating_sub(projection_start)
            .max(1)
            .min(u64::from(u32::MAX)) as u32;
        self.list.set_viewport(projection_start, projection_extent);
        self.list.write_window(&mut self.window);
        self.projection_guard = if self.projection_policy == ProjectionPolicy::BufferedIncremental {
            let maximum_offset = self.content_extent.saturating_sub(viewport_extent);
            let mounted_start = self.window.items.first().map_or(0, |item| item.start);
            let mounted_end = self
                .window
                .items
                .last()
                .map_or(self.content_extent, |item| item.end());
            Some((
                mounted_start.min(maximum_offset),
                mounted_end
                    .saturating_sub(viewport_extent)
                    .min(maximum_offset),
            ))
        } else {
            Some((guard_start, guard_end))
        };

        if self.projection_policy == ProjectionPolicy::BufferedIncremental {
            let rows = self.window_rows(blocks);
            let indices = self
                .window
                .items
                .iter()
                .filter(|item| blocks.row_data(item.index).is_some())
                .map(|item| item.index)
                .collect::<Vec<_>>();
            self.update_rows_incrementally(indices, rows);
        }
        self.projection_commits = self.projection_commits.saturating_add(1);
    }

    fn window_rows(
        &self,
        blocks: &ModelRc<ArtifactPreviewBlock>,
    ) -> Vec<ArtifactPreviewVirtualRow> {
        let rows = self
            .window
            .items
            .iter()
            .filter_map(|item| {
                Some(ArtifactPreviewVirtualRow {
                    start: item.start.min(f32::MAX as u64) as f32,
                    size: item.extent as f32,
                    block: blocks.row_data(item.index)?,
                })
            })
            .collect::<Vec<_>>();
        rows
    }

    fn update_rows_incrementally(
        &mut self,
        next_indices: Vec<usize>,
        next_rows: Vec<ArtifactPreviewVirtualRow>,
    ) {
        if self.projected_indices == next_indices {
            return;
        }
        let overlap_start = self
            .projected_indices
            .first()
            .zip(next_indices.first())
            .map(|(old, next)| (*old).max(*next));
        let overlap_end = self
            .projected_indices
            .last()
            .zip(next_indices.last())
            .map(|(old, next)| (*old).min(*next));
        let Some((overlap_start, overlap_end)) = overlap_start.zip(overlap_end) else {
            self.rows_model.set_vec(next_rows);
            self.projected_indices = next_indices;
            return;
        };
        if overlap_start > overlap_end {
            self.rows_model.set_vec(next_rows);
            self.projected_indices = next_indices;
            return;
        }

        let old_prefix = overlap_start - self.projected_indices[0];
        let new_prefix = overlap_start - next_indices[0];
        let overlap_len = overlap_end - overlap_start + 1;
        for _ in 0..old_prefix {
            self.rows_model.remove(0);
        }
        while self.rows_model.row_count() > overlap_len {
            self.rows_model.remove(self.rows_model.row_count() - 1);
        }
        for (index, row) in next_rows.iter().take(new_prefix).cloned().enumerate() {
            self.rows_model.insert(index, row);
        }
        for row in next_rows.into_iter().skip(new_prefix + overlap_len) {
            self.rows_model.push(row);
        }
        self.projected_indices = next_indices;
    }

    fn block_extent(
        &mut self,
        block: &ArtifactPreviewBlock,
        block_width: f32,
        application_font_family: &str,
        first: bool,
        last: bool,
    ) -> u32 {
        let edge_padding = if first { DOCUMENT_TOP_PADDING } else { 0.0 }
            + if last { DOCUMENT_BOTTOM_PADDING } else { 0.0 };
        if block.kind == ArtifactPreviewBlockKind::Rule {
            return to_u32(16.0 + edge_padding).max(1);
        }

        let metrics = block_metrics(block.kind);
        let text_width = (block_width - metrics.content_inset * 2.0).max(1.0);
        let family = if block.kind == ArtifactPreviewBlockKind::CodeBlock {
            "monospace"
        } else {
            application_font_family
        };
        let text_height = self.measure_text(block, family, metrics.font_size, text_width);
        to_u32(text_height + metrics.padding_top + metrics.padding_bottom + edge_padding).max(1)
    }

    fn measure_text(
        &mut self,
        block: &ArtifactPreviewBlock,
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

        if matches!(
            block.kind,
            ArtifactPreviewBlockKind::Heading1
                | ArtifactPreviewBlockKind::Heading2
                | ArtifactPreviewBlockKind::Heading3
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
                ArtifactPreviewInlineKind::Text | ArtifactPreviewInlineKind::Link => {}
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
    content_inset: f32,
    padding_top: f32,
    padding_bottom: f32,
}

fn block_metrics(kind: ArtifactPreviewBlockKind) -> BlockMetrics {
    match kind {
        ArtifactPreviewBlockKind::Heading1 => BlockMetrics {
            font_size: 26.0,
            content_inset: 0.0,
            padding_top: 16.0,
            padding_bottom: 12.0,
        },
        ArtifactPreviewBlockKind::Heading2 => BlockMetrics {
            font_size: 22.0,
            content_inset: 0.0,
            padding_top: 20.0,
            padding_bottom: 12.0,
        },
        ArtifactPreviewBlockKind::Heading3 => BlockMetrics {
            font_size: 18.0,
            content_inset: 0.0,
            padding_top: 16.0,
            padding_bottom: 6.0,
        },
        ArtifactPreviewBlockKind::CodeBlock => BlockMetrics {
            font_size: 12.0,
            content_inset: 12.0,
            padding_top: 12.0,
            padding_bottom: 12.0,
        },
        ArtifactPreviewBlockKind::Quote => BlockMetrics {
            font_size: 15.0,
            content_inset: 16.0,
            padding_top: 8.0,
            padding_bottom: 8.0,
        },
        ArtifactPreviewBlockKind::TableRow => BlockMetrics {
            font_size: 13.0,
            content_inset: 8.0,
            padding_top: 8.0,
            padding_bottom: 8.0,
        },
        ArtifactPreviewBlockKind::ListItem => BlockMetrics {
            font_size: 15.0,
            content_inset: 0.0,
            padding_top: 2.0,
            padding_bottom: 2.0,
        },
        ArtifactPreviewBlockKind::Paragraph | ArtifactPreviewBlockKind::Rule => BlockMetrics {
            font_size: 15.0,
            content_inset: 0.0,
            padding_top: 4.0,
            padding_bottom: 8.0,
        },
    }
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

fn to_u64(value: f32) -> u64 {
    if value.is_finite() {
        value.round().clamp(0.0, u64::MAX as f32) as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(kind: ArtifactPreviewBlockKind, text: &str) -> ArtifactPreviewBlock {
        ArtifactPreviewBlock {
            kind,
            styled: slint::StyledText::from_plain_text(text),
            plain_text: text.into(),
            layout_spans: ModelRc::default(),
        }
    }

    #[test]
    fn content_extent_is_stable_across_scroll_positions() {
        let blocks = ModelRc::new(std::rc::Rc::new(VecModel::from(vec![
            block(
                ArtifactPreviewBlockKind::Heading1,
                "A long heading for wrapping",
            ),
            block(
                ArtifactPreviewBlockKind::Paragraph,
                &"paragraph ".repeat(80),
            ),
            block(
                ArtifactPreviewBlockKind::CodeBlock,
                &"let value = 1;\n".repeat(30),
            ),
            block(
                ArtifactPreviewBlockKind::TableRow,
                &"cell | value | ".repeat(20),
            ),
        ])));
        let mut bridge = MarkdownVirtualLayoutBridge::default();
        let first = bridge.layout(1, &blocks, 0.0, 420.0, 520.0, "sans-serif");
        let middle = bridge.layout(
            1,
            &blocks,
            first.content_height * 0.5,
            420.0,
            520.0,
            "sans-serif",
        );
        let end = bridge.layout(1, &blocks, first.content_height, 420.0, 520.0, "sans-serif");
        assert_eq!(first.content_height, middle.content_height);
        assert_eq!(first.content_height, end.content_height);
        assert_eq!(end.scroll_offset, (end.content_height - 420.0).max(0.0));
    }

    #[test]
    fn normalized_thumb_positions_map_to_normalized_document_offsets() {
        let blocks = ModelRc::new(std::rc::Rc::new(VecModel::from(vec![
            block(ArtifactPreviewBlockKind::Paragraph, &"body ".repeat(400)),
            block(ArtifactPreviewBlockKind::CodeBlock, &"code();\n".repeat(80)),
        ])));
        let mut bridge = MarkdownVirtualLayoutBridge::default();
        let top = bridge.layout(7, &blocks, 0.0, 300.0, 500.0, "sans-serif");
        let maximum = (top.content_height - 300.0).max(0.0);
        for ratio in [0.25_f32, 0.5, 0.75, 1.0] {
            let window = bridge.layout(7, &blocks, maximum * ratio, 300.0, 500.0, "sans-serif");
            let actual = if maximum > 0.0 {
                window.scroll_offset / maximum
            } else {
                0.0
            };
            assert!((actual - ratio).abs() <= 0.001);
        }
    }

    #[test]
    fn buffered_projection_reuses_model_inside_its_guard() {
        let blocks = ModelRc::new(Rc::new(VecModel::from(
            (0..120)
                .map(|index| {
                    block(
                        ArtifactPreviewBlockKind::Paragraph,
                        &format!("row {index} {}", "body ".repeat(12)),
                    )
                })
                .collect::<Vec<_>>(),
        )));
        let mut bridge = MarkdownVirtualLayoutBridge::with_projection_policy(
            ProjectionPolicy::BufferedIncremental,
        );
        let first = bridge.layout(3, &blocks, 600.0, 400.0, 520.0, "sans-serif");
        let commits = bridge.projection_commits;
        let second = bridge.layout(3, &blocks, 720.0, 400.0, 520.0, "sans-serif");

        assert_eq!(first.rows, second.rows);
        assert_eq!(bridge.projection_commits, commits);
    }

    #[test]
    fn buffered_projection_is_bounded_and_updates_after_crossing_guard() {
        let blocks = ModelRc::new(Rc::new(VecModel::from(
            (0..300)
                .map(|index| {
                    block(
                        ArtifactPreviewBlockKind::Paragraph,
                        &format!("row {index} {}", "body ".repeat(8)),
                    )
                })
                .collect::<Vec<_>>(),
        )));
        let mut bridge = MarkdownVirtualLayoutBridge::with_projection_policy(
            ProjectionPolicy::BufferedIncremental,
        );
        let first = bridge.layout(5, &blocks, 0.0, 360.0, 520.0, "sans-serif");
        let commits = bridge.projection_commits;
        let next = bridge.layout(5, &blocks, 2_000.0, 360.0, 520.0, "sans-serif");

        assert_eq!(first.rows, next.rows);
        assert!(bridge.projection_commits > commits);
        assert!(next.rows.row_count() < blocks.row_count() / 3);
        assert!(next.rows.iter().any(|row| row.start <= 2_000.0));
        assert!(next.rows.iter().any(|row| row.start + row.size >= 2_360.0));
    }

    #[test]
    fn per_event_policy_replaces_the_model_for_ab_baseline() {
        let blocks = ModelRc::new(Rc::new(VecModel::from(vec![
            block(ArtifactPreviewBlockKind::Paragraph, &"body ".repeat(200)),
            block(ArtifactPreviewBlockKind::Paragraph, &"tail ".repeat(200)),
        ])));
        let mut bridge =
            MarkdownVirtualLayoutBridge::with_projection_policy(ProjectionPolicy::PerScrollEvent);
        let first = bridge.layout(9, &blocks, 0.0, 300.0, 500.0, "sans-serif");
        let second = bridge.layout(9, &blocks, 80.0, 300.0, 500.0, "sans-serif");

        assert_ne!(first.rows, second.rows);
        assert_eq!(bridge.projection_commits, 2);
    }
}
