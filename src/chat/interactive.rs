use crate::app_runtime::shared_runtime;
use crate::chat::chat::{handle_api_error, send_message_to_server};
use crate::chat::renderer::{clear_render_cache, record_model_commit, render_for_ui};
use crate::chat::session::ChatSessionService;
use crate::chat::virtual_layout::{
    VirtualRenderSignature, VirtualWindowPolicy, clear_virtual_layout_cache,
    compute_spacers_for_fixed_window, compute_virtual_window_with_estimates,
    current_virtual_document_height_px, get_adaptive_row_height_px, smooth_adaptive_row_height_px,
};
use crate::{MainWindow, Route};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

// 微调参数：适当放宽可用宽度预算，降低中文段落过早换行概率
const WRAP_WIDTH_TUNING_FACTOR: f32 = 1.10;
const LOADING_PHASE_INTERVAL_MS: u64 = 700; // 低频点点动画，降低发送中主线程更新压力
const LOADING_PHASE_ANIMATION_ENABLED: bool = false;
// Fixed buckets keep the decision independent from adaptive row-height updates.
// Move the bounded window before the viewport consumes its pixel overscan, but
// only while motion is observed. Idle polls never replace the message model.
const VIRTUAL_SCROLL_BUCKET_HEIGHT_PX: f32 = 64.0;
const BASELINE_SCROLL_UPDATE_BUCKET_STEP: i32 = 16;
const OPTIMIZED_SCROLL_UPDATE_BUCKET_STEP: i32 = 16;
const AUTO_LOAD_MORE_TOP_THRESHOLD_PX: f32 = 160.0;
const AUTO_LOAD_MORE_REARM_THRESHOLD_PX: f32 = 320.0;
const MOUNTED_EDGE_GUARD_PX: f32 = 2400.0;
const LAYOUT_SETTLE_DELAY_MS: u64 = 80;
const MAX_NEW_ROWS_PER_SCROLL_COMMIT: usize = 2;
const STICK_TO_BOTTOM_THRESHOLD_PX: f32 = 96.0;

fn virtual_scroll_bucket(scroll_offset_px: f32) -> i32 {
    (scroll_offset_px.max(0.0) / VIRTUAL_SCROLL_BUCKET_HEIGHT_PX).floor() as i32
}

fn should_shift_virtual_window(applied_bucket: i32, observed_bucket: i32, step: i32) -> bool {
    (observed_bucket - applied_bucket).abs() >= step
}

fn should_shift_near_mounted_edge(
    geometries: &HashMap<i32, MessageGeometry>,
    scroll_offset: f32,
    viewport_height: f32,
    moving_down: bool,
    has_more_in_direction: bool,
) -> bool {
    if !has_more_in_direction || geometries.is_empty() {
        return false;
    }
    let mounted_top = geometries
        .values()
        .map(|geometry| geometry.y)
        .fold(f32::INFINITY, f32::min);
    let mounted_bottom = geometries
        .values()
        .map(|geometry| geometry.y + geometry.height)
        .fold(f32::NEG_INFINITY, f32::max);
    if moving_down {
        scroll_offset + viewport_height + MOUNTED_EDGE_GUARD_PX >= mounted_bottom
    } else {
        scroll_offset - MOUNTED_EDGE_GUARD_PX <= mounted_top
    }
}

fn limit_incremental_virtual_shift(
    previous: VirtualRenderSignature,
    desired_start: usize,
    desired_end: usize,
    max_new_rows: usize,
) -> (usize, usize) {
    if max_new_rows == 0 || desired_start >= desired_end {
        return (desired_start, desired_end);
    }

    let overlap_start = previous.start.max(desired_start);
    let overlap_end = previous.end.min(desired_end);
    if overlap_start >= overlap_end {
        // A scrollbar jump must cover the destination viewport immediately.
        return (desired_start, desired_end);
    }

    let desired_len = desired_end - desired_start;
    let previous_len = previous.end - previous.start;
    let next_len = desired_len.min(previous_len.saturating_add(max_new_rows));
    if desired_start < previous.start {
        let start = previous
            .start
            .saturating_sub(max_new_rows)
            .max(desired_start);
        return (start, (start + next_len).min(previous.total_len));
    }
    if desired_end > previous.end {
        let end = previous
            .end
            .saturating_add(max_new_rows)
            .min(desired_end)
            .min(previous.total_len);
        return (end.saturating_sub(next_len), end);
    }
    (desired_start, desired_end)
}

fn scroll_update_bucket_step() -> i32 {
    if crate::chat::performance::variant().is_optimized() {
        OPTIMIZED_SCROLL_UPDATE_BUCKET_STEP
    } else {
        BASELINE_SCROLL_UPDATE_BUCKET_STEP
    }
}

fn update_auto_load_armed(armed: bool, scroll_offset_px: f32) -> (bool, bool) {
    if scroll_offset_px > AUTO_LOAD_MORE_REARM_THRESHOLD_PX {
        (true, false)
    } else if armed && scroll_offset_px <= AUTO_LOAD_MORE_TOP_THRESHOLD_PX {
        (false, true)
    } else {
        (armed, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MessageGeometry {
    y: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VisibleAnchor {
    message_id: i32,
    content_y: f32,
    scroll_offset: f32,
}

fn select_visible_anchor(
    geometries: &HashMap<i32, MessageGeometry>,
    scroll_offset: f32,
) -> Option<VisibleAnchor> {
    geometries
        .iter()
        .filter(|(_, geometry)| geometry.height > 0.0)
        .min_by(|(_, left), (_, right)| {
            let left_rank = if left.y >= scroll_offset - 1.0 { 0 } else { 1 };
            let right_rank = if right.y >= scroll_offset - 1.0 { 0 } else { 1 };
            left_rank.cmp(&right_rank).then_with(|| {
                if left_rank == 0 {
                    left.y.total_cmp(&right.y)
                } else {
                    right.y.total_cmp(&left.y)
                }
            })
        })
        .map(|(message_id, geometry)| VisibleAnchor {
            message_id: *message_id,
            content_y: geometry.y,
            scroll_offset,
        })
}

fn anchored_scroll_offset(
    anchor: VisibleAnchor,
    new_content_y: f32,
    current_scroll_offset: f32,
    max_correction_px: Option<f32>,
) -> Option<f32> {
    if (current_scroll_offset - anchor.scroll_offset).abs() > 1.0 {
        return None;
    }
    let delta = new_content_y - anchor.content_y;
    if max_correction_px.is_some_and(|maximum| delta.abs() > maximum) {
        return None;
    }
    (delta.abs() > 0.5).then_some((anchor.scroll_offset + delta).max(0.0))
}

thread_local! {
    static LAST_RENDER_SIGNATURE: RefCell<Option<VirtualRenderSignature>> = const { RefCell::new(None) };
    static MESSAGE_GEOMETRIES: RefCell<HashMap<i32, MessageGeometry>> = RefCell::new(HashMap::new());
    static PENDING_HISTORY_ANCHOR: Cell<Option<VisibleAnchor>> = const { Cell::new(None) };
    static PENDING_ANCHOR_MAX_CORRECTION_PX: Cell<Option<f32>> = const { Cell::new(None) };
    static ACTIVE_VIRTUAL_WRAP_WIDTH_PX: Cell<i32> = const { Cell::new(0) };
    static MODEL_MUTATION_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
    static CHAT_MESSAGE_MODEL: RefCell<Option<Rc<ChatMessageModelBridge>>> = const { RefCell::new(None) };
    static ANCHOR_SETTLE_REVISION: Cell<u64> = const { Cell::new(0) };
    static ANCHOR_SCROLL_WRITE_IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
    static ANCHOR_SCROLL_WRITE_COUNT: Cell<u64> = const { Cell::new(0) };
}

fn schedule_anchor_compensation(ui_weak: slint::Weak<MainWindow>) {
    let revision = ANCHOR_SETTLE_REVISION.with(|latest| {
        let revision = latest.get().wrapping_add(1);
        latest.set(revision);
        revision
    });
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        if ANCHOR_SETTLE_REVISION.with(Cell::get) != revision
            || MODEL_MUTATION_IN_PROGRESS.with(Cell::get)
        {
            return;
        }
        let Some(anchor) = PENDING_HISTORY_ANCHOR.with(Cell::get) else {
            return;
        };
        let new_content_y = MESSAGE_GEOMETRIES.with(|geometries| {
            geometries
                .borrow()
                .get(&anchor.message_id)
                .map(|geometry| geometry.y)
        });
        PENDING_HISTORY_ANCHOR.with(|pending| pending.set(None));
        let max_correction_px =
            PENDING_ANCHOR_MAX_CORRECTION_PX.with(|maximum| maximum.replace(None));
        let Some(new_content_y) = new_content_y else {
            return;
        };
        if let Some(ui) = ui_weak.upgrade()
            && ui.get_current_route() == Route::Chat
        {
            if ui.get_chat_scrollbar_dragging() {
                clear_pending_anchor();
                return;
            }
            let current_scroll_offset = (-ui.get_chat_scroll_position()).max(0.0);
            let Some(target_offset) = anchored_scroll_offset(
                anchor,
                new_content_y,
                current_scroll_offset,
                max_correction_px,
            ) else {
                return;
            };
            ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(|active| active.set(true));
            ui.set_chat_scroll_position(-target_offset);
            ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(|active| active.set(false));
            ANCHOR_SCROLL_WRITE_COUNT.with(|count| count.set(count.get().wrapping_add(1)));
        }
    });
}

struct ChatMessageModelBridge {
    incremental: bool,
    rows_model: Rc<VecModel<crate::ChatMessage>>,
    mounted_ids: RefCell<Vec<i32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelMutationCounts {
    inserted: usize,
    removed: usize,
    incremental: bool,
}

impl ChatMessageModelBridge {
    fn new(incremental: bool) -> Self {
        Self {
            incremental,
            rows_model: Rc::new(VecModel::default()),
            mounted_ids: RefCell::new(Vec::new()),
        }
    }

    fn bind(&self, ui: &MainWindow) {
        if self.incremental {
            ui.set_chat_messages(ModelRc::from(self.rows_model.clone()));
        }
    }

    fn replace_rows(
        &self,
        ui: &MainWindow,
        next_rows: Vec<crate::ChatMessage>,
        next_top_spacer: f32,
        next_bottom_spacer: f32,
    ) -> ModelMutationCounts {
        let next_ids = next_rows.iter().map(|row| row.id).collect::<Vec<_>>();
        if !self.incremental {
            let removed = ui.get_chat_messages().row_count();
            let inserted = next_rows.len();
            ui.set_chat_messages(next_rows.as_slice().into());
            *self.mounted_ids.borrow_mut() = next_ids;
            return ModelMutationCounts {
                inserted,
                removed,
                incremental: false,
            };
        }

        let old_ids = self.mounted_ids.borrow().clone();
        if old_ids == next_ids {
            ui.set_chat_virtual_top_spacer(next_top_spacer);
            ui.set_chat_virtual_bottom_spacer(next_bottom_spacer);
            return ModelMutationCounts {
                inserted: 0,
                removed: 0,
                incremental: true,
            };
        }

        let old_top_spacer = ui.get_chat_virtual_top_spacer();
        let old_bottom_spacer = ui.get_chat_virtual_bottom_spacer();
        if next_top_spacer > old_top_spacer {
            ui.set_chat_virtual_top_spacer(next_top_spacer);
        }
        if next_bottom_spacer > old_bottom_spacer {
            ui.set_chat_virtual_bottom_spacer(next_bottom_spacer);
        }

        let overlap = longest_contiguous_overlap(&old_ids, &next_ids);
        let (inserted, removed) = if let Some((old_start, next_start, overlap_len)) = overlap {
            for (index, row) in next_rows.iter().take(next_start).cloned().enumerate() {
                self.rows_model.insert(index, row);
            }
            for row in next_rows.iter().skip(next_start + overlap_len).cloned() {
                self.rows_model.push(row);
            }
            for _ in 0..old_start {
                self.rows_model.remove(next_start);
            }
            let old_suffix = old_ids.len().saturating_sub(old_start + overlap_len);
            for _ in 0..old_suffix {
                self.rows_model.remove(next_start + overlap_len);
            }
            (
                next_rows.len().saturating_sub(overlap_len),
                old_ids.len().saturating_sub(overlap_len),
            )
        } else {
            self.rows_model.set_vec(next_rows);
            (next_ids.len(), old_ids.len())
        };
        if next_top_spacer <= old_top_spacer {
            ui.set_chat_virtual_top_spacer(next_top_spacer);
        }
        if next_bottom_spacer <= old_bottom_spacer {
            ui.set_chat_virtual_bottom_spacer(next_bottom_spacer);
        }
        *self.mounted_ids.borrow_mut() = next_ids;
        ModelMutationCounts {
            inserted,
            removed,
            incremental: overlap.is_some(),
        }
    }

    fn clear(&self, ui: &MainWindow) {
        self.mounted_ids.borrow_mut().clear();
        if self.incremental {
            self.rows_model.set_vec(Vec::new());
        } else {
            ui.set_chat_messages([].as_slice().into());
        }
    }
}

fn longest_contiguous_overlap(old: &[i32], next: &[i32]) -> Option<(usize, usize, usize)> {
    let mut best = None;
    for (old_start, old_id) in old.iter().enumerate() {
        for (next_start, next_id) in next.iter().enumerate() {
            if old_id != next_id {
                continue;
            }
            let mut len = 0;
            while old_start + len < old.len()
                && next_start + len < next.len()
                && old[old_start + len] == next[next_start + len]
            {
                len += 1;
            }
            if best.is_none_or(|(_, _, best_len)| len > best_len) {
                best = Some((old_start, next_start, len));
            }
        }
    }
    best
}

fn update_virtual_spacers(ui: &MainWindow, top_spacer: f32, bottom_spacer: f32) {
    let was_active = MODEL_MUTATION_IN_PROGRESS.with(|active| active.replace(true));
    let old_top = ui.get_chat_virtual_top_spacer();
    let old_bottom = ui.get_chat_virtual_bottom_spacer();
    if top_spacer > old_top {
        ui.set_chat_virtual_top_spacer(top_spacer);
    }
    if bottom_spacer > old_bottom {
        ui.set_chat_virtual_bottom_spacer(bottom_spacer);
    }
    if top_spacer <= old_top {
        ui.set_chat_virtual_top_spacer(top_spacer);
    }
    if bottom_spacer <= old_bottom {
        ui.set_chat_virtual_bottom_spacer(bottom_spacer);
    }
    MODEL_MUTATION_IN_PROGRESS.with(|active| active.set(was_active));
}

fn clear_history_anchor_state() {
    MESSAGE_GEOMETRIES.with(|geometries| geometries.borrow_mut().clear());
    clear_pending_anchor();
}

fn clear_pending_anchor() {
    PENDING_HISTORY_ANCHOR.with(|anchor| anchor.set(None));
    PENDING_ANCHOR_MAX_CORRECTION_PX.with(|maximum| maximum.set(None));
}

fn virtual_window_has_more(moving_down: bool) -> bool {
    LAST_RENDER_SIGNATURE.with(|last| {
        last.borrow().is_some_and(|signature| {
            if moving_down {
                signature.end < signature.total_len
            } else {
                signature.start > 0
            }
        })
    })
}

pub(crate) fn virtual_blank_exposure_px(scroll_offset: f32, viewport_height: f32) -> f32 {
    let Some(signature) = LAST_RENDER_SIGNATURE.with(|last| *last.borrow()) else {
        return 0.0;
    };
    MESSAGE_GEOMETRIES.with(|geometries| {
        let geometries = geometries.borrow();
        if geometries.is_empty() {
            return 0.0;
        }
        let mounted_top = geometries
            .values()
            .map(|geometry| geometry.y)
            .fold(f32::INFINITY, f32::min);
        let mounted_bottom = geometries
            .values()
            .map(|geometry| geometry.y + geometry.height)
            .fold(f32::NEG_INFINITY, f32::max);
        let viewport_bottom = scroll_offset + viewport_height;
        let top_blank = if signature.start > 0 {
            (mounted_top - scroll_offset).clamp(0.0, viewport_height)
        } else {
            0.0
        };
        let bottom_blank = if signature.end < signature.total_len {
            (viewport_bottom - mounted_bottom).clamp(0.0, viewport_height)
        } else {
            0.0
        };
        top_blank.max(bottom_blank)
    })
}

fn capture_visible_anchor(ui: &MainWindow, max_correction_px: Option<f32>) {
    if ui.get_chat_scrollbar_dragging() {
        clear_pending_anchor();
        return;
    }
    let model = ui.get_chat_messages();
    let mounted_ids = (0..model.row_count())
        .filter_map(|index| model.row_data(index).map(|message| message.id))
        .collect::<std::collections::HashSet<_>>();
    let scroll_offset = (-ui.get_chat_scroll_position()).max(0.0);
    let anchor = MESSAGE_GEOMETRIES.with(|geometries| {
        let mut geometries = geometries.borrow_mut();
        geometries.retain(|message_id, _| mounted_ids.contains(message_id));
        select_visible_anchor(&geometries, scroll_offset)
    });
    PENDING_HISTORY_ANCHOR.with(|pending| pending.set(anchor));
    PENDING_ANCHOR_MAX_CORRECTION_PX.with(|maximum| {
        maximum.set(anchor.and(max_correction_px));
    });
}

pub(crate) fn anchor_scroll_write_count() -> u64 {
    ANCHOR_SCROLL_WRITE_COUNT.with(Cell::get)
}

fn capture_visible_history_anchor(ui: &MainWindow) {
    capture_visible_anchor(ui, None);
}

fn compute_wrap_width_px(ui: &MainWindow) -> i32 {
    let window_width = ui.get_chat_layout_window_width();
    let nav_width = if ui.get_navigation_drawer_expanded() {
        240.0
    } else {
        0.0
    };
    let chat_content_width = (window_width - nav_width - 24.0).max(320.0);
    compute_wrap_width_for_chat_width(chat_content_width)
}

fn active_wrap_width_px(ui: &MainWindow) -> i32 {
    ACTIVE_VIRTUAL_WRAP_WIDTH_PX.with(|width| {
        let active = width.get();
        if active > 0 {
            active
        } else {
            compute_wrap_width_px(ui)
        }
    })
}

fn compute_wrap_width_for_chat_width(chat_width: f32) -> i32 {
    let ratio = if chat_width < 720.0 {
        0.78
    } else if chat_width < 1200.0 {
        0.68
    } else {
        0.62
    };
    let bubble_width = (chat_width * ratio).clamp(280.0, 960.0);
    let content_width = (bubble_width - 24.0) * WRAP_WIDTH_TUNING_FACTOR;
    content_width.round().clamp(200.0, 936.0) as i32
}

fn apply_chat_state_to_ui(
    ui: &MainWindow,
    session: &Arc<Mutex<ChatSessionService>>,
    include_loading_placeholder: bool,
    wrap_width_override_px: Option<i32>,
    limit_scroll_insertions: bool,
) {
    let (
        messages,
        can_load_more,
        top_spacer_px,
        bottom_spacer_px,
        document_height_px,
        render_signature,
    ) = {
        let guard = match session.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("[Chat::Session] Lock poisoned: {}", e);
                return;
            }
        };

        let all_messages = guard.loaded_messages();
        let total = all_messages.len();

        if total == 0 {
            (
                Vec::new(),
                guard.can_load_more(),
                0.0f32,
                0.0f32,
                0.0f32,
                None,
            )
        } else {
            let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
            let viewport_height_px =
                (ui.get_chat_layout_window_height() - 180.0).clamp(360.0, 1400.0);
            let wrap_width_px = wrap_width_override_px.unwrap_or_else(|| compute_wrap_width_px(ui));
            ACTIVE_VIRTUAL_WRAP_WIDTH_PX.with(|width| width.set(wrap_width_px));
            let row_estimate_px = get_adaptive_row_height_px();
            let window_policy = if crate::chat::performance::variant().is_optimized() {
                VirtualWindowPolicy::optimized()
            } else {
                VirtualWindowPolicy::baseline()
            };
            let (mut start, mut end, mut top_px, mut bottom_px, sampled_avg_height_px) =
                compute_virtual_window_with_estimates(
                    all_messages,
                    scroll_offset_px,
                    viewport_height_px,
                    wrap_width_px,
                    row_estimate_px,
                    window_policy,
                );
            if limit_scroll_insertions
                && crate::chat::performance::variant().is_optimized()
                && let Some(previous) = LAST_RENDER_SIGNATURE.with(|last| *last.borrow())
                && previous.total_len == total
                && previous.include_loading_placeholder == include_loading_placeholder
            {
                let limited = limit_incremental_virtual_shift(
                    previous,
                    start,
                    end,
                    MAX_NEW_ROWS_PER_SCROLL_COMMIT,
                );
                if limited != (start, end) {
                    start = limited.0;
                    end = limited.1;
                    if let Some((limited_top, limited_bottom, _)) = compute_spacers_for_fixed_window(
                        all_messages,
                        start,
                        end,
                        wrap_width_px,
                        true,
                    ) {
                        top_px = limited_top;
                        bottom_px = limited_bottom;
                    }
                }
            }
            smooth_adaptive_row_height_px(sampled_avg_height_px);
            let document_height_px = current_virtual_document_height_px();
            (
                all_messages[start..end].to_vec(),
                guard.can_load_more(),
                top_px,
                bottom_px,
                document_height_px,
                Some(VirtualRenderSignature {
                    total_len: total,
                    start,
                    end,
                    include_loading_placeholder,
                }),
            )
        }
    };

    let should_render = LAST_RENDER_SIGNATURE.with(|last| {
        let mut slot = last.borrow_mut();
        let same = *slot == render_signature;
        if !same {
            *slot = render_signature;
        }
        !same
    });
    if should_render {
        let mounted_ids = messages
            .iter()
            .map(|message| message.id as i32)
            .collect::<std::collections::HashSet<_>>();
        let rendered = render_for_ui(&messages, include_loading_placeholder);
        let mutation_started = Instant::now();
        MODEL_MUTATION_IN_PROGRESS.with(|active| active.set(true));
        let counts = CHAT_MESSAGE_MODEL.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("Chat message model must be initialized on the event loop")
                .replace_rows(ui, rendered, top_spacer_px, bottom_spacer_px)
        });
        MODEL_MUTATION_IN_PROGRESS.with(|active| active.set(false));
        MESSAGE_GEOMETRIES.with(|geometries| {
            geometries
                .borrow_mut()
                .retain(|message_id, _| mounted_ids.contains(message_id));
        });
        record_model_commit(
            mutation_started.elapsed(),
            counts.inserted,
            counts.removed,
            counts.incremental,
        );
        schedule_anchor_compensation(ui.as_weak());
    } else {
        update_virtual_spacers(ui, top_spacer_px, bottom_spacer_px);
        schedule_anchor_compensation(ui.as_weak());
    }
    ui.set_chat_virtual_document_height(document_height_px);
    ui.set_chat_can_load_more_history(can_load_more);
}

fn clear_chat_ui_state(ui: &MainWindow) {
    CHAT_MESSAGE_MODEL.with(|slot| {
        if let Some(model_bridge) = slot.borrow().as_ref() {
            model_bridge.clear(ui);
        } else {
            ui.set_chat_messages([].as_slice().into());
        }
    });
    ui.set_chat_can_load_more_history(false);
    ui.set_chat_loading_history(false);
    ui.set_chat_history_restoring(false);
    ui.set_chat_virtual_top_spacer(0.0);
    ui.set_chat_virtual_bottom_spacer(0.0);
    ui.set_chat_virtual_document_height(0.0);
    ui.set_chat_loading_phase(0);
}

fn schedule_chat_ui_mount(
    ui_weak: slint::Weak<MainWindow>,
    session: Arc<Mutex<ChatSessionService>>,
    scroll_to_latest: bool,
) {
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        if ui.get_current_route() != Route::Chat {
            return;
        }

        // The conditional Chat component is instantiated during the first layout
        // pass after the route property changes. Re-applying the model on the next
        // event-loop tick ensures its repeaters and ScrollView already exist.
        LAST_RENDER_SIGNATURE.with(|last| {
            *last.borrow_mut() = None;
        });
        apply_chat_state_to_ui(
            &ui,
            &session,
            ui.get_chat_is_sending(),
            Some(active_wrap_width_px(&ui)),
            false,
        );
        ui.set_chat_history_restoring(false);
        if scroll_to_latest {
            scroll_chat_to_bottom(&ui);
        }
    });
}

fn begin_chat_ui_mount(ui: &MainWindow, session: &Arc<Mutex<ChatSessionService>>) {
    let has_history = session
        .lock()
        .map(|guard| !guard.loaded_messages().is_empty())
        .unwrap_or_else(|error| {
            log::error!("[Chat::Session] Lock poisoned while mounting Chat: {error}");
            false
        });
    ui.set_chat_history_restoring(has_history);
}

fn schedule_loading_phase_tick(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    slint::Timer::single_shot(
        std::time::Duration::from_millis(LOADING_PHASE_INTERVAL_MS),
        move || {
            if !loading_phase_active.load(Ordering::Relaxed) {
                return;
            }

            if let Some(ui) = ui_weak.upgrade() {
                if ui.get_chat_is_sending() && ui.get_current_route() == Route::Chat {
                    let next = (ui.get_chat_loading_phase() + 1) % 3;
                    ui.set_chat_loading_phase(next);
                }
                schedule_loading_phase_tick(ui_weak.clone(), loading_phase_active.clone());
            } else {
                loading_phase_active.store(false, Ordering::Relaxed);
            }
        },
    );
}

fn start_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    if !LOADING_PHASE_ANIMATION_ENABLED {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_chat_loading_phase(0);
        }
        loading_phase_active.store(false, Ordering::Relaxed);
        return;
    }
    if loading_phase_active.load(Ordering::Relaxed) {
        return;
    }
    loading_phase_active.store(true, Ordering::Relaxed);
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_chat_loading_phase(0);
    }
    schedule_loading_phase_tick(ui_weak, loading_phase_active);
}

fn stop_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    loading_phase_active.store(false, Ordering::Relaxed);
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_chat_loading_phase(0);
    }
}

fn scroll_chat_to_bottom(ui: &MainWindow) {
    ui.set_chat_scroll_to_bottom_revision(ui.get_chat_scroll_to_bottom_revision().wrapping_add(1));
}

/// 设置聊天交互逻辑和事件回调
pub fn setup_interactive(ui: &MainWindow) {
    log::info!("[Chat::Interactive] Setting up interactive callbacks...");
    let runtime = shared_runtime();

    let mut session = match ChatSessionService::bootstrap_default() {
        Ok(s) => s,
        Err(e) => {
            log::error!("[Chat::Storage] Failed to initialize chat storage: {}", e);
            return;
        }
    };
    session.install_performance_fixture();
    let session = Arc::new(Mutex::new(session));
    let model_bridge = Rc::new(ChatMessageModelBridge::new(
        crate::chat::performance::variant().is_optimized(),
    ));
    model_bridge.bind(ui);
    CHAT_MESSAGE_MODEL.with(|slot| *slot.borrow_mut() = Some(model_bridge));

    {
        if let Ok(guard) = session.lock() {
            guard.log_storage_path();
            log::info!(
                "[Chat::History] bootstrap loaded_messages={} can_load_more={}",
                guard.loaded_messages().len(),
                guard.can_load_more()
            );
        }
    }

    let ui_weak = ui.as_weak();
    let has_entered_chat_once = Rc::new(Cell::new(false));
    let loading_phase_active = Arc::new(AtomicBool::new(false));
    let loading_phase_active_send = loading_phase_active.clone();
    let observed_scroll_bucket = Rc::new(Cell::new(-1));
    let applied_scroll_bucket = Rc::new(Cell::new(-1));
    let auto_load_armed = Rc::new(Cell::new(true));
    let virtual_commit_in_progress = Rc::new(Cell::new(false));
    let virtual_commit_scheduled = Rc::new(Cell::new(false));
    let layout_revision = Rc::new(Cell::new(0u64));
    let settled_layout_size = Rc::new(Cell::new((0i32, 0i32)));
    let layout_stick_to_bottom = Rc::new(Cell::new(false));

    let ui_weak_geometry = ui_weak.clone();
    ui.on_chat_message_geometry(move |message_id, y, height, position_changed| {
        MESSAGE_GEOMETRIES.with(|geometries| {
            geometries
                .borrow_mut()
                .insert(message_id, MessageGeometry { y, height });
        });

        if !position_changed {
            return;
        }
        let Some(anchor) = PENDING_HISTORY_ANCHOR.with(Cell::get) else {
            return;
        };
        if anchor.message_id != message_id {
            return;
        }
        schedule_anchor_compensation(ui_weak_geometry.clone());
    });

    ui.on_chat_scrollbar_dragging_changed(move |dragging| {
        if dragging {
            clear_pending_anchor();
        }
    });

    let ui_weak_layout = ui_weak.clone();
    let session_layout = session.clone();
    let layout_revision_callback = layout_revision.clone();
    let settled_layout_size_callback = settled_layout_size.clone();
    let observed_scroll_bucket_layout = observed_scroll_bucket.clone();
    let applied_scroll_bucket_layout = applied_scroll_bucket.clone();
    let layout_stick_to_bottom_callback = layout_stick_to_bottom.clone();
    ui.on_chat_layout_metrics_changed(move |width, height| {
        let size = (width.round() as i32, height.round() as i32);
        if let Some(ui) = ui_weak_layout.upgrade()
            && ui.get_chat_messages().row_count() > 0
            && ui.get_chat_bottom_distance() <= STICK_TO_BOTTOM_THRESHOLD_PX
        {
            layout_stick_to_bottom_callback.set(true);
        }
        let revision = layout_revision_callback.get().wrapping_add(1);
        layout_revision_callback.set(revision);
        let ui_weak = ui_weak_layout.clone();
        let session = session_layout.clone();
        let latest_revision = layout_revision_callback.clone();
        let settled_size = settled_layout_size_callback.clone();
        let observed_bucket = observed_scroll_bucket_layout.clone();
        let applied_bucket = applied_scroll_bucket_layout.clone();
        let stick_to_bottom_on_settle = layout_stick_to_bottom_callback.clone();
        slint::Timer::single_shot(
            std::time::Duration::from_millis(LAYOUT_SETTLE_DELAY_MS),
            move || {
                if latest_revision.get() != revision || settled_size.get() == size {
                    return;
                }
                let Some(ui) = ui_weak.upgrade() else {
                    return;
                };
                if ui.get_current_route() != Route::Chat || crate::is_window_interacting() {
                    return;
                }
                let stick_to_bottom = stick_to_bottom_on_settle.replace(false)
                    || ui.get_chat_bottom_distance() <= STICK_TO_BOTTOM_THRESHOLD_PX;
                if stick_to_bottom {
                    clear_pending_anchor();
                } else {
                    capture_visible_history_anchor(&ui);
                }
                settled_size.set(size);
                apply_chat_state_to_ui(
                    &ui,
                    &session,
                    ui.get_chat_is_sending(),
                    Some(compute_wrap_width_for_chat_width(width)),
                    false,
                );
                if stick_to_bottom {
                    scroll_chat_to_bottom(&ui);
                }
                let bucket = virtual_scroll_bucket((-ui.get_chat_scroll_position()).max(0.0));
                observed_bucket.set(bucket);
                applied_bucket.set(bucket);
            },
        );
    });

    // Chat's Slint tree and model are route-scoped. The Rust session remains the
    // source of truth and reconstructs the bounded UI model when Chat is entered.
    if ui.get_current_route() == Route::Chat {
        has_entered_chat_once.set(true);
        begin_chat_ui_mount(ui, &session);
        schedule_chat_ui_mount(ui.as_weak(), session.clone(), true);
    } else {
        clear_chat_ui_state(ui);
    }

    // 处理加载更多历史回调
    let ui_weak_load_more = ui_weak.clone();
    let session_load_more = session.clone();
    let runtime_load_more = runtime.clone();
    ui.on_chat_load_more_history(move || {
        if let Some(ui) = ui_weak_load_more.upgrade() {
            if ui.get_chat_loading_history() {
                return;
            }

            ui.set_chat_loading_history(true);
            log::debug!("[Chat::History] Start loading older messages...");
            let session_for_ui = session_load_more.clone();
            let session_for_task = session_load_more.clone();
            let ui_weak_for_task = ui_weak_load_more.clone();
            runtime_load_more.spawn(async move {
                let io_result = tokio::task::spawn_blocking(move || {
                    let mut guard = session_for_task
                        .lock()
                        .map_err(|e| format!("session lock poisoned: {e}"))?;
                    guard
                        .load_more_history()
                        .map_err(|e| format!("load history failed: {e}"))
                })
                .await;

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak_for_task.upgrade() else {
                        return;
                    };

                    match io_result {
                        Ok(Ok(loaded_count)) => {
                            if ui.get_current_route() == Route::Chat {
                                if loaded_count > 0 {
                                    capture_visible_history_anchor(&ui);
                                }
                                ui.set_chat_loading_history(false);
                                let wrap_width_px = active_wrap_width_px(&ui);
                                apply_chat_state_to_ui(
                                    &ui,
                                    &session_for_ui,
                                    ui.get_chat_is_sending(),
                                    Some(wrap_width_px),
                                    false,
                                );
                            }
                            log::debug!("[Chat::History] History page loaded with visible anchor");
                        }
                        Ok(Err(e)) => {
                            PENDING_HISTORY_ANCHOR.with(|pending| pending.set(None));
                            ui.set_chat_loading_history(false);
                            log::error!("[Chat::Storage] Failed to load older history: {}", e);
                        }
                        Err(e) => {
                            PENDING_HISTORY_ANCHOR.with(|pending| pending.set(None));
                            ui.set_chat_loading_history(false);
                            log::error!("[Chat::Storage] Background load task join failed: {}", e);
                        }
                    }
                });
            });
        }
    });

    // 处理发送消息回调
    let session_send = session.clone();
    let runtime_send = runtime.clone();
    ui.on_chat_send_message(move |text: slint::SharedString| {
        let ui_weak_local = ui_weak.clone();

        let Some(ui) = ui_weak_local.upgrade() else {
            log::error!("[Chat::Interactive] Failed to upgrade UI weak reference");
            return;
        };

        let text_str = text.to_string();
        if text_str.trim().is_empty() {
            return;
        }
        if ui.get_chat_is_sending() {
            return;
        }

        let append_result = {
            let mut guard = match session_send.lock() {
                Ok(g) => g,
                Err(e) => {
                    log::error!("[Chat::Session] Lock poisoned: {}", e);
                    return;
                }
            };
            guard.append_user_message(text_str.clone())
        };

        if let Err(e) = append_result {
            log::error!("[Chat::Storage] Failed to persist user message: {}", e);
            return;
        }

        ui.set_chat_input_text("".into());
        ui.set_chat_is_sending(true);

        start_loading_phase_animation(ui.as_weak(), loading_phase_active_send.clone());

        apply_chat_state_to_ui(
            &ui,
            &session_send,
            true,
            Some(active_wrap_width_px(&ui)),
            false,
        );
        scroll_chat_to_bottom(&ui);

        // 异步发送消息到服务器并处理响应
        let ui_weak_clone = ui_weak.clone();
        let session_response = session_send.clone();
        let loading_phase_active_response = loading_phase_active_send.clone();

        runtime_send.spawn(async move {
            let result = send_message_to_server(&text_str).await;

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak_clone.upgrade() else {
                    return;
                };

                loading_phase_active_response.store(false, Ordering::Relaxed);

                let mut guard = match session_response.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        log::error!("[Chat::Session] Lock poisoned: {}", e);
                        ui.set_chat_is_sending(false);
                        ui.set_chat_loading_phase(0);
                        return;
                    }
                };

                match result {
                    Ok(response) => {
                        if let Err(e) = guard.append_assistant_message(response) {
                            log::error!(
                                "[Chat::Storage] Failed to persist assistant message: {}",
                                e
                            );
                        }
                    }
                    Err(err) => {
                        let friendly = handle_api_error(&err);
                        if let Err(e) = guard.append_assistant_error(friendly) {
                            log::error!("[Chat::Storage] Failed to persist error message: {}", e);
                        }
                    }
                }
                drop(guard);

                ui.set_chat_is_sending(false);
                ui.set_chat_loading_phase(0);
                if ui.get_current_route() == Route::Chat {
                    apply_chat_state_to_ui(
                        &ui,
                        &session_response,
                        false,
                        Some(active_wrap_width_px(&ui)),
                        false,
                    );
                    scroll_chat_to_bottom(&ui);
                }
            });
        });
    });

    // Scroll virtualization is driven by actual viewport changes. This keeps a
    // final delayed poll from replacing the model after wheel motion stops.
    let initial_scroll_bucket = {
        let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
        virtual_scroll_bucket(scroll_offset_px)
    };
    observed_scroll_bucket.set(initial_scroll_bucket);
    applied_scroll_bucket.set(initial_scroll_bucket);

    let ui_weak_scroll = ui.as_weak();
    let session_scroll = session.clone();
    let observed_scroll_bucket_scroll = observed_scroll_bucket.clone();
    let applied_scroll_bucket_scroll = applied_scroll_bucket.clone();
    let auto_load_armed_scroll = auto_load_armed.clone();
    let virtual_commit_in_progress_scroll = virtual_commit_in_progress.clone();
    let virtual_commit_scheduled_scroll = virtual_commit_scheduled.clone();
    let layout_stick_to_bottom_scroll = layout_stick_to_bottom.clone();
    ui.on_chat_scroll_position_changed(move |position| {
        let Some(ui) = ui_weak_scroll.upgrade() else {
            return;
        };
        if ui.get_current_route() != Route::Chat
            || ui.get_chat_history_restoring()
            || crate::is_window_interacting()
            || virtual_commit_in_progress_scroll.get()
            || MODEL_MUTATION_IN_PROGRESS.with(Cell::get)
            || ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(Cell::get)
            || PENDING_HISTORY_ANCHOR.with(Cell::get).is_some()
        {
            return;
        }

        let scroll_offset_px = (-position).max(0.0);
        if ui.get_chat_bottom_distance() > STICK_TO_BOTTOM_THRESHOLD_PX {
            layout_stick_to_bottom_scroll.set(false);
        }
        let (next_auto_load_armed, should_auto_load) =
            update_auto_load_armed(auto_load_armed_scroll.get(), scroll_offset_px);
        auto_load_armed_scroll.set(next_auto_load_armed);
        if should_auto_load && ui.get_chat_can_load_more_history() && !ui.get_chat_loading_history()
        {
            ui.invoke_chat_load_more_history();
        }

        let scroll_bucket = virtual_scroll_bucket(scroll_offset_px);
        if scroll_bucket == observed_scroll_bucket_scroll.get() {
            return;
        }
        observed_scroll_bucket_scroll.set(scroll_bucket);
        let applied = applied_scroll_bucket_scroll.get();
        if applied < 0 {
            applied_scroll_bucket_scroll.set(scroll_bucket);
            return;
        }
        let moving_down = scroll_bucket > applied;
        let viewport_height = (ui.get_chat_layout_window_height() - 180.0).clamp(360.0, 1400.0);
        let near_mounted_edge = crate::chat::performance::variant().is_optimized()
            && MESSAGE_GEOMETRIES.with(|geometries| {
                should_shift_near_mounted_edge(
                    &geometries.borrow(),
                    scroll_offset_px,
                    viewport_height,
                    moving_down,
                    virtual_window_has_more(moving_down),
                )
            });
        if !near_mounted_edge
            && !should_shift_virtual_window(applied, scroll_bucket, scroll_update_bucket_step())
        {
            return;
        }
        if virtual_commit_scheduled_scroll.replace(true) {
            return;
        }

        let ui_weak = ui_weak_scroll.clone();
        let session = session_scroll.clone();
        let applied_bucket = applied_scroll_bucket_scroll.clone();
        let commit_scheduled = virtual_commit_scheduled_scroll.clone();
        let commit_in_progress = virtual_commit_in_progress_scroll.clone();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            commit_scheduled.set(false);
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if ui.get_current_route() != Route::Chat
                || ui.get_chat_history_restoring()
                || crate::is_window_interacting()
                || commit_in_progress.get()
                || MODEL_MUTATION_IN_PROGRESS.with(Cell::get)
                || ANCHOR_SCROLL_WRITE_IN_PROGRESS.with(Cell::get)
                || PENDING_HISTORY_ANCHOR.with(Cell::get).is_some()
            {
                return;
            }

            let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
            let scroll_bucket = virtual_scroll_bucket(scroll_offset_px);
            let applied = applied_bucket.get();
            if applied < 0 {
                applied_bucket.set(scroll_bucket);
                return;
            }
            let moving_down = scroll_bucket > applied;
            let viewport_height = (ui.get_chat_layout_window_height() - 180.0).clamp(360.0, 1400.0);
            let near_mounted_edge = crate::chat::performance::variant().is_optimized()
                && MESSAGE_GEOMETRIES.with(|geometries| {
                    should_shift_near_mounted_edge(
                        &geometries.borrow(),
                        scroll_offset_px,
                        viewport_height,
                        moving_down,
                        virtual_window_has_more(moving_down),
                    )
                });
            if !near_mounted_edge
                && !should_shift_virtual_window(applied, scroll_bucket, scroll_update_bucket_step())
            {
                return;
            }

            if crate::chat::performance::variant().is_optimized()
                || ui.get_chat_scrollbar_dragging()
            {
                clear_pending_anchor();
            } else {
                capture_visible_anchor(&ui, Some(MOUNTED_EDGE_GUARD_PX));
            }
            applied_bucket.set(scroll_bucket);
            commit_in_progress.set(true);
            apply_chat_state_to_ui(
                &ui,
                &session,
                ui.get_chat_is_sending(),
                Some(active_wrap_width_px(&ui)),
                true,
            );
            // Model assignment can synchronously emit geometry callbacks, so
            // guard only that stack. The zero-delay scheduler coalesces input
            // without leaving an idle poll after wheel motion stops.
            commit_in_progress.set(false);
        });
    });

    let ui_weak_route = ui.as_weak();
    let session_route = session.clone();
    let observed_scroll_bucket_route = observed_scroll_bucket.clone();
    let applied_scroll_bucket_route = applied_scroll_bucket.clone();
    let auto_load_armed_route = auto_load_armed.clone();
    let loading_phase_active_route = loading_phase_active.clone();
    let has_entered_chat_once_route = has_entered_chat_once.clone();

    ui.on_chat_route_visible_changed(move |visible| {
        if visible {
            let first_entry = !has_entered_chat_once_route.replace(true);
            if let Some(ui) = ui_weak_route.upgrade() {
                begin_chat_ui_mount(&ui, &session_route);
            }
            schedule_chat_ui_mount(ui_weak_route.clone(), session_route.clone(), first_entry);
            if let Some(ui) = ui_weak_route.upgrade() {
                let bucket = virtual_scroll_bucket((-ui.get_chat_scroll_position()).max(0.0));
                observed_scroll_bucket_route.set(bucket);
                applied_scroll_bucket_route.set(bucket);
                auto_load_armed_route.set(true);
            }
        } else {
            clear_history_anchor_state();
            clear_virtual_layout_cache();
            stop_loading_phase_animation(ui_weak_route.clone(), loading_phase_active_route.clone());
            LAST_RENDER_SIGNATURE.with(|last| {
                *last.borrow_mut() = None;
            });
            clear_render_cache();
            if let Some(ui) = ui_weak_route.upgrade() {
                clear_chat_ui_state(&ui);
            }
        }
    });

    log::info!("[Chat::Interactive] Interactive callbacks initialized successfully");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_scroll_bucket_changes_only_with_scroll_position() {
        assert_eq!(virtual_scroll_bucket(0.0), 0);
        assert_eq!(virtual_scroll_bucket(63.9), 0);
        assert_eq!(virtual_scroll_bucket(64.0), 1);
        assert_eq!(virtual_scroll_bucket(1_024.0), 16);
    }

    #[test]
    fn contiguous_overlap_preserves_the_stable_middle_window() {
        assert_eq!(
            longest_contiguous_overlap(&[10, 11, 12, 13], &[12, 13, 14, 15]),
            Some((2, 0, 2))
        );
        assert_eq!(
            longest_contiguous_overlap(&[12, 13, 14, 15], &[10, 11, 12, 13]),
            Some((0, 2, 2))
        );
        assert_eq!(longest_contiguous_overlap(&[1, 2], &[3, 4]), None);
        assert_eq!(longest_contiguous_overlap(&[], &[1]), None);
    }

    #[test]
    fn virtual_window_shift_requires_the_full_pixel_threshold() {
        assert!(!should_shift_virtual_window(0, 15, 16));
        assert!(should_shift_virtual_window(0, 16, 16));
        assert!(!should_shift_virtual_window(30, 15, 16));
        assert!(should_shift_virtual_window(30, 14, 16));
        assert!(!should_shift_virtual_window(0, 63, 64));
        assert!(should_shift_virtual_window(0, 64, 64));
        assert!(!should_shift_virtual_window(0, 127, 128));
        assert!(should_shift_virtual_window(0, 128, 128));
    }

    fn render_signature(start: usize, end: usize) -> VirtualRenderSignature {
        VirtualRenderSignature {
            total_len: 100,
            start,
            end,
            include_loading_placeholder: false,
        }
    }

    #[test]
    fn incremental_shift_caps_new_rows_without_shrinking_the_overlap_to_one_row() {
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 17, 30, 2),
            (12, 25)
        );
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 3, 16, 2),
            (8, 21)
        );
    }

    #[test]
    fn incremental_shift_keeps_deletions_and_non_overlapping_jumps_exact() {
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 12, 20, 2),
            (12, 20)
        );
        assert_eq!(
            limit_incremental_virtual_shift(render_signature(10, 23), 30, 38, 2),
            (30, 38)
        );
    }

    #[test]
    fn visible_anchor_prefers_the_first_fully_visible_message() {
        let geometries = HashMap::from([
            (
                10,
                MessageGeometry {
                    y: 80.0,
                    height: 90.0,
                },
            ),
            (
                11,
                MessageGeometry {
                    y: 178.0,
                    height: 60.0,
                },
            ),
            (
                12,
                MessageGeometry {
                    y: 246.0,
                    height: 70.0,
                },
            ),
        ]);

        assert_eq!(
            select_visible_anchor(&geometries, 150.0),
            Some(VisibleAnchor {
                message_id: 11,
                content_y: 178.0,
                scroll_offset: 150.0,
            })
        );
    }

    #[test]
    fn prepend_compensation_keeps_the_anchor_at_the_same_viewport_pixel() {
        let anchor = VisibleAnchor {
            message_id: 11,
            content_y: 178.0,
            scroll_offset: 150.0,
        };

        let next_offset = anchored_scroll_offset(anchor, 438.0, 150.0, None).unwrap();
        assert_eq!(next_offset, 410.0);
        assert_eq!(anchor.content_y - anchor.scroll_offset, 438.0 - next_offset);
        assert_eq!(anchored_scroll_offset(anchor, 178.2, 150.0, None), None);
    }

    #[test]
    fn stale_or_implausible_virtual_anchor_cannot_override_new_scroll_input() {
        let anchor = VisibleAnchor {
            message_id: 11,
            content_y: 10_000.0,
            scroll_offset: 9_800.0,
        };

        assert_eq!(
            anchored_scroll_offset(anchor, 10_200.0, 10_120.0, Some(2_400.0)),
            None
        );
        assert_eq!(
            anchored_scroll_offset(anchor, 20_000.0, 9_800.0, Some(2_400.0)),
            None
        );
        assert_eq!(
            anchored_scroll_offset(anchor, 10_200.0, 9_800.0, Some(2_400.0)),
            Some(10_000.0)
        );
    }

    #[test]
    fn mounted_edge_guard_shifts_before_the_viewport_enters_a_spacer() {
        let geometries = HashMap::from([
            (
                20,
                MessageGeometry {
                    y: 10_000.0,
                    height: 180.0,
                },
            ),
            (
                21,
                MessageGeometry {
                    y: 10_188.0,
                    height: 220.0,
                },
            ),
        ]);

        assert!(!should_shift_near_mounted_edge(
            &geometries,
            7_000.0,
            720.0,
            true,
            true,
        ));
        assert!(should_shift_near_mounted_edge(
            &geometries,
            8_000.0,
            720.0,
            true,
            true,
        ));
        assert!(!should_shift_near_mounted_edge(
            &geometries,
            8_000.0,
            720.0,
            true,
            false,
        ));
    }

    #[test]
    fn blank_exposure_is_zero_when_real_rows_cover_the_viewport() {
        LAST_RENDER_SIGNATURE.with(|last| {
            *last.borrow_mut() = Some(VirtualRenderSignature {
                total_len: 10,
                start: 2,
                end: 6,
                include_loading_placeholder: false,
            });
        });
        MESSAGE_GEOMETRIES.with(|geometries| {
            *geometries.borrow_mut() = HashMap::from([(
                3,
                MessageGeometry {
                    y: 1_000.0,
                    height: 2_000.0,
                },
            )]);
        });
        assert_eq!(virtual_blank_exposure_px(1_200.0, 720.0), 0.0);
        assert_eq!(virtual_blank_exposure_px(2_800.0, 720.0), 520.0);
        clear_history_anchor_state();
        LAST_RENDER_SIGNATURE.with(|last| *last.borrow_mut() = None);
    }

    #[test]
    fn automatic_history_load_requires_leaving_and_reentering_the_top_zone() {
        assert_eq!(update_auto_load_armed(true, 150.0), (false, true));
        assert_eq!(update_auto_load_armed(false, 0.0), (false, false));
        assert_eq!(update_auto_load_armed(false, 321.0), (true, false));
        assert_eq!(update_auto_load_armed(true, 159.0), (false, true));
    }
}
