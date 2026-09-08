//! Exact Markdown geometry for the Preview virtual surface.

#[cfg(test)]
use super::measurement::block_padding_top;
use super::measurement::{MeasurementBlock, Measurer};
use crate::app_runtime::latest_compute::LatestCompute;
use crate::{ArtifactPreviewBlock, ArtifactPreviewVirtualRow, ArtifactPreviewVirtualWindow};
#[cfg(test)]
use crate::{ArtifactPreviewBlockKind, ArtifactPreviewInlineKind};
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;
use std::sync::Arc;
use usher_virtual_list::{VirtualList, VirtualWindow};

const MARKDOWN_OVERSCAN_BLOCKS: usize = 3;
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
    measurer: Measurer,
    worker: LatestCompute<MeasureRequest, MeasureResult>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    requested: Option<LayoutKey>,
    measurements: Option<(i32, Arc<Vec<MeasurementBlock>>)>,
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
            measurer: Measurer::default(),
            worker: LatestCompute::new(),
            wake: None,
            requested: None,
            measurements: None,
        }
    }
}

struct MeasureRequest {
    key: LayoutKey,
    blocks: Arc<Vec<MeasurementBlock>>,
    collection: parley::fontique::Collection,
}
struct MeasureResult {
    key: LayoutKey,
    extents: Vec<u32>,
}

impl MarkdownVirtualLayoutBridge {
    pub(super) fn enable_worker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Arc::new(wake));
    }
    pub(super) fn notification_acknowledger(&self) -> impl Fn() + Send + Sync + 'static {
        let worker = self.worker.clone();
        move || worker.acknowledge()
    }

    pub(super) fn reset_document(&mut self) {
        self.worker.cancel();
        self.requested = None;
        self.measurements = None;
        self.list
            .replace_item_extents(&[], MARKDOWN_OVERSCAN_BLOCKS);
        self.layout_key = None;
        self.window = VirtualWindow::default();
        self.rows_model.set_vec(Vec::new());
        self.projected_indices.clear();
        self.content_extent = 0;
        self.projection_guard = None;
        self.last_scroll_offset = None;
        self.pending_scroll_activity = None;
        self.measurer = Measurer::default();
    }

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
        if let Some(result) = self.worker.take_output() {
            if result.key == key {
                self.install_extents(result.key, result.extents);
            } else if self.requested.as_ref() == Some(&result.key) {
                self.requested = None;
            }
        }
        if self.layout_key.as_ref() == Some(&key)
            && self
                .requested
                .as_ref()
                .is_some_and(|requested| requested != &key)
        {
            self.worker.cancel();
            self.requested = None;
        }
        if self.layout_key.as_ref() != Some(&key) && self.requested.as_ref() != Some(&key) {
            let document_changed = self
                .layout_key
                .as_ref()
                .is_none_or(|old| old.revision != revision);
            let input = if let Some(model) = blocks
                .as_any()
                .downcast_ref::<super::projection_model::MarkdownModel>()
            {
                model.measurements.clone()
            } else if let Some((cached_revision, input)) = &self.measurements {
                if *cached_revision == revision {
                    input.clone()
                } else {
                    snapshot(blocks)
                }
            } else {
                snapshot(blocks)
            };
            self.measurements = Some((revision, input.clone()));
            if let Some(wake) = self.wake.clone() {
                self.requested = Some(key.clone());
                if document_changed {
                    self.rows_model.set_vec(Vec::new());
                    self.projected_indices.clear();
                    self.content_extent = 0;
                    self.projection_guard = None;
                    self.layout_key = None;
                }
                self.worker.submit(
                    MeasureRequest {
                        key: key.clone(),
                        blocks: input,
                        collection: self.measurer.collection(),
                    },
                    |request, token| {
                        let started = std::time::Instant::now();
                        let mut measurer = Measurer::with_collection(request.collection);
                        let extents = measurer.measure(
                            &request.blocks,
                            f32::from_bits(request.key.content_width_bits),
                            &request.key.font_family,
                            || token.cancelled(),
                        )?;
                        if super::benchmark::enabled() {
                            log::info!(
                                "[P5][60] measure_us={} blocks={}",
                                started.elapsed().as_micros(),
                                extents.len()
                            );
                        }
                        Some(MeasureResult {
                            key: request.key,
                            extents,
                        })
                    },
                    move || wake(),
                );
            } else {
                let extents = self
                    .measurer
                    .measure(&input, content_width, font_family, || false)
                    .unwrap();
                self.install_extents(key.clone(), extents);
            }
        }
        if self.layout_key.is_none() {
            return ArtifactPreviewVirtualWindow {
                rows: self.rows.clone(),
                loading: true,
                layout_width: content_width,
                ..Default::default()
            };
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
            loading: self.layout_key.as_ref() != Some(&key),
            layout_width: self
                .layout_key
                .as_ref()
                .map_or(content_width, |key| f32::from_bits(key.content_width_bits)),
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
        self.window
            .items
            .iter()
            .filter_map(|item| {
                Some(ArtifactPreviewVirtualRow {
                    index: i32::try_from(item.index).unwrap_or(i32::MAX),
                    start: item.start.min(f32::MAX as u64) as f32,
                    size: item.extent as f32,
                    block: blocks.row_data(item.index)?,
                })
            })
            .collect::<Vec<_>>()
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

    fn install_extents(&mut self, key: LayoutKey, extents: Vec<u32>) {
        self.requested = None;
        self.list
            .replace_item_extents(&extents, MARKDOWN_OVERSCAN_BLOCKS);
        self.content_extent = extents.iter().map(|extent| u64::from(*extent)).sum();
        self.layout_key = Some(key);
        self.projection_guard = None;
        self.projected_indices.clear();
    }
    #[cfg(test)]
    fn block_extent(
        &mut self,
        block: &ArtifactPreviewBlock,
        width: f32,
        font: &str,
        previous: Option<ArtifactPreviewBlockKind>,
        first: bool,
        last: bool,
    ) -> u32 {
        self.measurer.block_extent(
            &measurement_block(block),
            width,
            font,
            previous,
            first,
            last,
        )
    }
}
impl Drop for MarkdownVirtualLayoutBridge {
    fn drop(&mut self) {
        self.worker.cancel();
    }
}
fn measurement_block(block: &ArtifactPreviewBlock) -> MeasurementBlock {
    MeasurementBlock {
        kind: block.kind,
        plain_text: block.plain_text.to_string(),
        list_depth: block.list_depth,
        code_language: block.code_language.to_string(),
        layout_spans: block.layout_spans.iter().collect(),
    }
}
fn snapshot(blocks: &ModelRc<ArtifactPreviewBlock>) -> Arc<Vec<MeasurementBlock>> {
    Arc::new(
        blocks
            .iter()
            .map(|block| measurement_block(&block))
            .collect(),
    )
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

    #[test]
    fn worker_matches_exact_layout_and_rejects_replaced_document() {
        let blocks = ModelRc::new(VecModel::from(vec![
            block(
                ArtifactPreviewBlockKind::Paragraph,
                &"English 中文 **text** ".repeat(200),
            ),
            block(
                ArtifactPreviewBlockKind::CodeBlock,
                &"code();\n".repeat(12000),
            ),
        ]));
        let mut expected = MarkdownVirtualLayoutBridge::default();
        let precise = expected.layout(2, &blocks, 0.0, 420.0, 520.0, "sans-serif");
        let mut bridge = MarkdownVirtualLayoutBridge::default();
        let (notify, ready) = std::sync::mpsc::channel();
        let acknowledge = bridge.notification_acknowledger();
        bridge.enable_worker(move || {
            notify.send(()).unwrap();
        });
        let first = bridge.layout(1, &blocks, 0.0, 420.0, 300.0, "sans-serif");
        assert!(first.loading);
        bridge.layout(2, &blocks, 0.0, 420.0, 520.0, "sans-serif");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            ready
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
            acknowledge();
            let result = bridge.layout(2, &blocks, 0.0, 420.0, 520.0, "sans-serif");
            if !result.loading {
                assert_eq!(result.content_height, precise.content_height);
                assert_eq!(result.layout_width, 520.0);
                break;
            }
        }
        bridge.reset_document();
        assert!(bridge.worker.take_output().is_none());
        assert_eq!(bridge.rows.row_count(), 0);
    }

    #[test]
    fn returning_to_current_width_cancels_pending_width_request() {
        let blocks = ModelRc::new(VecModel::from(vec![block(
            ArtifactPreviewBlockKind::Paragraph,
            "body",
        )]));
        let mut bridge = MarkdownVirtualLayoutBridge::default();
        bridge.layout(1, &blocks, 0.0, 420.0, 520.0, "sans-serif");
        let mut stale = bridge.layout_key.clone().unwrap();
        stale.content_width_bits = 300.0f32.to_bits();
        bridge.requested = Some(stale);
        let current = bridge.layout(1, &blocks, 0.0, 420.0, 520.0, "sans-serif");
        assert!(!current.loading);
        assert!(bridge.requested.is_none());
        let resized = bridge.layout(1, &blocks, 0.0, 420.0, 300.0, "sans-serif");
        assert_eq!(resized.layout_width, 300.0);
    }

    fn block(kind: ArtifactPreviewBlockKind, text: &str) -> ArtifactPreviewBlock {
        ArtifactPreviewBlock {
            kind,
            styled: slint::StyledText::from_plain_text(text),
            list_marker: "".into(),
            list_depth: 0,
            code_lines: ModelRc::default(),
            code_language: "".into(),
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
    fn short_cjk_quote_does_not_reserve_excessive_bottom_space() {
        let quote = block(
            ArtifactPreviewBlockKind::Quote,
            "注意：appId 和 universalLink 必须由微信开放平台申请并配置，否则无法正常唤起微信。",
        );
        let mut bridge = MarkdownVirtualLayoutBridge::default();

        let extent = bridge.block_extent(&quote, 520.0, ".AppleSystemUIFont", None, false, false);

        assert!(extent <= 100, "short quote reserved {extent}px");
    }

    #[test]
    fn mixed_script_heading_reserves_glyph_safety_height() {
        let heading = block(
            ArtifactPreviewBlockKind::Heading2,
            "从 Cargo.lock 变化推测 / Mixed glyph baseline",
        );
        let mut bridge = MarkdownVirtualLayoutBridge::default();

        let extent = bridge.block_extent(&heading, 520.0, ".AppleSystemUIFont", None, false, false);

        assert!(
            extent >= 42,
            "mixed-script heading reserved only {extent}px"
        );
    }

    #[test]
    fn fenced_code_extent_includes_compact_header_and_bottom_padding() {
        let mut code = block(
            ArtifactPreviewBlockKind::CodeBlock,
            "cargo fmt --all\ncargo check --locked\ncargo test --lib --locked\ngit diff --check",
        );
        code.code_language = "sh".into();
        let mut bridge = MarkdownVirtualLayoutBridge::default();

        let extent = bridge.block_extent(
            &code,
            520.0,
            ".AppleSystemUIFont",
            Some(ArtifactPreviewBlockKind::Paragraph),
            false,
            false,
        );

        assert_eq!(extent, 4 * 19 + 32 + 10);
    }

    #[test]
    fn virtual_spacing_mirrors_the_codex_preview_component_tokens() {
        assert_eq!(
            block_padding_top(
                ArtifactPreviewBlockKind::Paragraph,
                Some(ArtifactPreviewBlockKind::Paragraph),
                false,
                "",
            ),
            20.0
        );
        assert_eq!(
            block_padding_top(
                ArtifactPreviewBlockKind::ListItem,
                Some(ArtifactPreviewBlockKind::Paragraph),
                false,
                "",
            ),
            10.0
        );
        assert_eq!(
            block_padding_top(
                ArtifactPreviewBlockKind::ListItem,
                Some(ArtifactPreviewBlockKind::ListItem),
                false,
                "",
            ),
            0.0
        );
        assert_eq!(
            block_padding_top(
                ArtifactPreviewBlockKind::Paragraph,
                Some(ArtifactPreviewBlockKind::ListItem),
                false,
                "",
            ),
            10.0
        );
        assert_eq!(
            block_padding_top(
                ArtifactPreviewBlockKind::CodeBlock,
                Some(ArtifactPreviewBlockKind::Paragraph),
                false,
                "",
            ),
            10.0
        );
        assert_eq!(
            block_padding_top(
                ArtifactPreviewBlockKind::CodeBlock,
                Some(ArtifactPreviewBlockKind::Paragraph),
                false,
                "rust",
            ),
            32.0
        );
    }

    #[test]
    fn narrow_cjk_quote_with_inline_code_does_not_reserve_an_extra_line() {
        let mut quote = block(
            ArtifactPreviewBlockKind::Quote,
            "注意：注册操作必须在调用其他企业微信功能（如 share()、isInstalled() 或\nstartup()）之前完成，否则可能导致功能调用失败。",
        );
        let spans = ["share()", "isInstalled()", "startup()"]
            .into_iter()
            .map(|code| {
                let start = quote.plain_text.as_str().find(code).expect("inline code") as i32;
                crate::ArtifactPreviewInlineSpan {
                    kind: ArtifactPreviewInlineKind::Code,
                    start,
                    end: start + code.len() as i32,
                }
            })
            .collect::<Vec<_>>();
        quote.layout_spans = ModelRc::new(Rc::new(VecModel::from(spans)));
        let mut bridge = MarkdownVirtualLayoutBridge::default();

        // A roughly 355px docked panel leaves a 323px Markdown block after
        // the narrow document padding used by the Slint surface.
        let extent = bridge.block_extent(&quote, 323.0, ".AppleSystemUIFont", None, false, false);

        assert!(
            (106..=110).contains(&extent),
            "narrow quote must mirror the four-line 22px UI rhythm, got {extent}px"
        );
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
        let (guard_start, guard_end) = bridge.projection_guard.expect("projection guard");
        let inside_guard = guard_start.saturating_add(guard_end) / 2;
        let second = bridge.layout(3, &blocks, inside_guard as f32, 400.0, 520.0, "sans-serif");

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

    #[test]
    fn reset_document_clears_rows_extents_and_layout_caches() {
        let blocks = ModelRc::new(Rc::new(VecModel::from(vec![
            block(ArtifactPreviewBlockKind::Paragraph, &"body ".repeat(200)),
            block(ArtifactPreviewBlockKind::CodeBlock, &"code();\n".repeat(40)),
        ])));
        let mut bridge = MarkdownVirtualLayoutBridge::default();
        let window = bridge.layout(11, &blocks, 0.0, 300.0, 500.0, "sans-serif");
        let stable_rows = window.rows.clone();

        assert!(stable_rows.row_count() > 0);
        assert!(bridge.content_extent > 0);
        assert!(bridge.layout_key.is_some());

        bridge.reset_document();

        assert_eq!(stable_rows.row_count(), 0);
        assert_eq!(bridge.rows_model.row_count(), 0);
        assert!(bridge.projected_indices.is_empty());
        assert_eq!(bridge.content_extent, 0);
        assert!(bridge.layout_key.is_none());
        assert!(bridge.projection_guard.is_none());
        assert!(bridge.last_scroll_offset.is_none());
        assert!(bridge.pending_scroll_activity.is_none());
        assert!(bridge.window.items.is_empty());
    }
}
