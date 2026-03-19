use crate::app_runtime::shared_runtime;
use crate::chat::chat::{handle_api_error, send_message_to_server};
use crate::chat::markdown::looks_like_markdown;
use crate::chat::persistence::{StoredChatMessage, StoredMessageRole};
use crate::chat::renderer::{clear_render_cache, render_for_ui};
use crate::chat::session::ChatSessionService;
use crate::{MainWindow, Route};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// 微调参数：适当放宽可用宽度预算，降低中文段落过早换行概率
const WRAP_WIDTH_TUNING_FACTOR: f32 = 1.10;
// 性能参数：避免窗口拖拽时频繁重排导致卡顿
const REFLOW_POLL_INTERVAL_MS: u64 = 120;
const REFLOW_STABLE_POLLS_REQUIRED: u8 = 6; // 约 720ms 稳定后再重排
const REFLOW_MIN_WIDTH_DELTA_PX: i32 = 24; // 宽度变化过小时不触发重排
const LOADING_PHASE_INTERVAL_MS: u64 = 700; // 低频点点动画，降低发送中主线程更新压力
const LOADING_PHASE_ANIMATION_ENABLED: bool = false;
const LOADING_PHASE_PAUSE_AFTER_RESIZE_TICKS: u8 = 10; // 约 1.2s 内暂停点点动画
const VIRTUAL_ROW_ESTIMATED_HEIGHT_PX: f32 = 108.0;
const VIRTUAL_MIN_VISIBLE_ROWS: usize = 8;
const VIRTUAL_OVERSCAN_ROWS: usize = 40;
const VIRTUAL_MAX_WINDOW_ROWS: usize = 56;
const VIRTUAL_OVERSCAN_PX: f32 = 1200.0;
const SCROLL_FORCE_UPDATE_BUCKET_STEP: i32 = 40;
const SCROLL_IDLE_POLLS_REQUIRED: u8 = 3;
const SCROLL_COARSE_UPDATE_COOLDOWN_POLLS: u8 = 3;
const AUTO_LOAD_MORE_TOP_THRESHOLD_PX: f32 = 18.0;
const LOAD_MORE_MIN_ANIMATION_MS: u64 = 500;

thread_local! {
    static ADAPTIVE_ROW_HEIGHT_PX: Cell<f32> = const { Cell::new(VIRTUAL_ROW_ESTIMATED_HEIGHT_PX) };
    static VIRTUAL_LAYOUT_CACHE: RefCell<VirtualLayoutCache> = RefCell::new(VirtualLayoutCache::new());
    static LAST_RENDER_SIGNATURE: RefCell<Option<VirtualRenderSignature>> = const { RefCell::new(None) };
}

fn get_adaptive_row_height_px() -> f32 {
    ADAPTIVE_ROW_HEIGHT_PX.with(Cell::get)
}

fn smooth_adaptive_row_height_px(sample: f32) {
    let clamped = sample.clamp(72.0, 220.0);
    ADAPTIVE_ROW_HEIGHT_PX.with(|cell| {
        let current = cell.get();
        let next = current * 0.86 + clamped * 0.14;
        cell.set(next);
    });
}

fn compute_wrap_width_px(ui: &MainWindow) -> i32 {
    // 与 chat.slint 的断点策略保持一致，按窗口可用宽度估算气泡宽度
    let window_width = ui.get_chat_layout_window_width();
    let nav_width = if ui.get_navigation_drawer_expanded() {
        120.0
    } else {
        56.0
    };
    let chat_content_width = (window_width - nav_width - 24.0).max(320.0);

    let ratio = if chat_content_width < 720.0 {
        0.58
    } else if chat_content_width < 1200.0 {
        0.62
    } else {
        0.66
    };
    // 扣除气泡左右内边距（12px * 2）后，得到 markdown 内容可用宽度
    let bubble_width = (chat_content_width * ratio).clamp(280.0, 960.0);
    let content_width = (bubble_width - 24.0) * WRAP_WIDTH_TUNING_FACTOR;
    content_width.round().clamp(200.0, 936.0) as i32
}

fn apply_chat_state_to_ui(
    ui: &MainWindow,
    session: &Arc<Mutex<ChatSessionService>>,
    include_loading_placeholder: bool,
    wrap_width_override_px: Option<i32>,
) {
    let (
        messages,
        can_load_more,
        top_spacer_px,
        bottom_spacer_px,
        render_signature,
        wrap_width_px_for_render,
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
                None,
                wrap_width_override_px.unwrap_or(-1),
            )
        } else {
            let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
            let viewport_height_px =
                (ui.get_chat_layout_window_height() - 180.0).clamp(360.0, 1400.0);
            let wrap_width_px = wrap_width_override_px.unwrap_or_else(|| compute_wrap_width_px(ui));
            let row_estimate_px = get_adaptive_row_height_px();
            let (start, end, top_px, bottom_px, sampled_avg_height_px) =
                compute_virtual_window_with_estimates(
                    all_messages,
                    scroll_offset_px,
                    viewport_height_px,
                    wrap_width_px,
                    row_estimate_px,
                );
            smooth_adaptive_row_height_px(sampled_avg_height_px);
            (
                all_messages[start..end].to_vec(),
                guard.can_load_more(),
                top_px,
                bottom_px,
                Some(VirtualRenderSignature {
                    total_len: total,
                    start,
                    end,
                    wrap_width_px,
                    include_loading_placeholder,
                }),
                wrap_width_px,
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
        let rendered = render_for_ui(
            &messages,
            Some(wrap_width_px_for_render),
            include_loading_placeholder,
        );
        ui.set_chat_messages(rendered.as_slice().into());
    }
    ui.set_chat_can_load_more_history(can_load_more);
    ui.set_chat_virtual_top_spacer(top_spacer_px);
    ui.set_chat_virtual_bottom_spacer(bottom_spacer_px);
}

fn compute_virtual_window_with_estimates(
    messages: &[StoredChatMessage],
    scroll_offset_px: f32,
    viewport_height_px: f32,
    wrap_width_px: i32,
    row_estimate_px: f32,
) -> (usize, usize, f32, f32, f32) {
    let total = messages.len();
    if total == 0 {
        return (0, 0, 0.0, 0.0, row_estimate_px);
    }

    let visible_rows = ((viewport_height_px / row_estimate_px.max(72.0)).ceil() as usize)
        .max(VIRTUAL_MIN_VISIBLE_ROWS);
    let window_rows = (visible_rows + VIRTUAL_OVERSCAN_ROWS * 2)
        .min(VIRTUAL_MAX_WINDOW_ROWS)
        .max(visible_rows);

    VIRTUAL_LAYOUT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let sampled_avg_height_px = cache.ensure(messages, wrap_width_px);
        let prefix = cache.prefix_slice();

        let target_start = (scroll_offset_px - VIRTUAL_OVERSCAN_PX).max(0.0);
        let target_end = scroll_offset_px + viewport_height_px + VIRTUAL_OVERSCAN_PX;

        let mut start = find_first_index_after_offset(prefix, target_start);
        let mut end = find_first_index_after_offset(prefix, target_end);

        if end <= start {
            end = (start + visible_rows).min(total);
        }
        if end.saturating_sub(start) > window_rows {
            end = (start + window_rows).min(total);
        }
        if end.saturating_sub(start) < window_rows {
            let missing = window_rows - end.saturating_sub(start);
            end = (end + missing).min(total);
            let adjusted = end.saturating_sub(window_rows);
            start = start.min(adjusted);
        }

        let top_spacer_px = prefix[start];
        let total_height = prefix[total];
        let bottom_spacer_px = (total_height - prefix[end]).max(0.0);
        (
            start,
            end,
            top_spacer_px,
            bottom_spacer_px,
            sampled_avg_height_px,
        )
    })
}

fn find_first_index_after_offset(prefix: &[f32], target: f32) -> usize {
    // prefix[i] = total height of first i items
    // Return the first item index whose bottom is after target.
    if prefix.len() <= 1 {
        return 0;
    }
    let mut lo = 0usize;
    let mut hi = prefix.len() - 1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if prefix[mid] < target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo.saturating_sub(1).min(prefix.len().saturating_sub(2))
}

fn estimate_message_height_px(message: &StoredChatMessage, wrap_width_px: f32) -> f32 {
    let bubble_width = wrap_width_px.clamp(220.0, 960.0);
    let chars_per_line = (bubble_width / 8.4).clamp(22.0, 90.0);
    let lines_from_len = (message.content.chars().count() as f32 / chars_per_line).ceil();
    let lines_from_breaks = message.content.matches('\n').count() as f32 + 1.0;
    let mut lines = lines_from_len.max(lines_from_breaks);

    let is_markdown = matches!(message.role, StoredMessageRole::Assistant)
        && looks_like_markdown(&message.content);
    if is_markdown {
        let md_boost = message.content.matches("```").count() as f32 * 1.5
            + message.content.matches("- ").count() as f32 * 0.35
            + message.content.matches("# ").count() as f32 * 0.7;
        lines += md_boost.min(16.0);
    }

    let base = match message.role {
        StoredMessageRole::User => 58.0,
        StoredMessageRole::Assistant => 66.0,
    };
    let line_height = if is_markdown { 20.0 } else { 18.0 };
    (base + lines * line_height).clamp(58.0, 560.0)
}

fn estimate_prepended_batch_height_px(
    session: &Arc<Mutex<ChatSessionService>>,
    loaded_count: usize,
    wrap_width_px: i32,
) -> f32 {
    if loaded_count == 0 {
        return 0.0;
    }
    let guard = match session.lock() {
        Ok(g) => g,
        Err(e) => {
            log::error!(
                "[Chat::Session] Lock poisoned while estimating prepended height: {}",
                e
            );
            return 0.0;
        }
    };
    guard
        .loaded_messages()
        .iter()
        .take(loaded_count)
        .map(|m| estimate_message_height_px(m, wrap_width_px as f32))
        .sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtualLayoutKey {
    len: usize,
    first_id: i64,
    last_id: i64,
    wrap_width_px: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtualRenderSignature {
    total_len: usize,
    start: usize,
    end: usize,
    wrap_width_px: i32,
    include_loading_placeholder: bool,
}

struct VirtualLayoutCache {
    key: Option<VirtualLayoutKey>,
    prefix: Vec<f32>,
    sampled_avg_height_px: f32,
}

impl VirtualLayoutCache {
    fn new() -> Self {
        Self {
            key: None,
            prefix: Vec::new(),
            sampled_avg_height_px: VIRTUAL_ROW_ESTIMATED_HEIGHT_PX,
        }
    }

    fn ensure(&mut self, messages: &[StoredChatMessage], wrap_width_px: i32) -> f32 {
        let Some(key) = Self::make_key(messages, wrap_width_px) else {
            self.key = None;
            self.prefix.clear();
            self.sampled_avg_height_px = VIRTUAL_ROW_ESTIMATED_HEIGHT_PX;
            return self.sampled_avg_height_px;
        };

        let valid = self.key == Some(key) && self.prefix.len() == key.len + 1;
        if valid {
            return self.sampled_avg_height_px;
        }

        if self.try_incremental_rebuild(messages, key) {
            return self.sampled_avg_height_px;
        }

        self.rebuild(messages, key);
        self.sampled_avg_height_px
    }

    fn prefix_slice(&self) -> &[f32] {
        &self.prefix
    }

    fn rebuild(&mut self, messages: &[StoredChatMessage], key: VirtualLayoutKey) {
        self.prefix.clear();
        self.prefix.reserve(messages.len() + 1);
        self.prefix.push(0.0);

        let mut sampled_sum = 0.0f32;
        for message in messages {
            let h = estimate_message_height_px(message, key.wrap_width_px as f32);
            sampled_sum += h;
            let next = self.prefix.last().copied().unwrap_or(0.0) + h;
            self.prefix.push(next);
        }

        self.sampled_avg_height_px = if messages.is_empty() {
            VIRTUAL_ROW_ESTIMATED_HEIGHT_PX
        } else {
            sampled_sum / messages.len() as f32
        };
        self.key = Some(key);
    }

    fn try_incremental_rebuild(
        &mut self,
        messages: &[StoredChatMessage],
        new_key: VirtualLayoutKey,
    ) -> bool {
        let Some(old_key) = self.key else {
            return false;
        };
        if old_key.wrap_width_px != new_key.wrap_width_px {
            return false;
        }
        if self.prefix.len() != old_key.len + 1 || new_key.len <= old_key.len {
            return false;
        }

        if self.try_prepend_incremental(messages, old_key, new_key) {
            return true;
        }
        if self.try_append_incremental(messages, old_key, new_key) {
            return true;
        }
        false
    }

    fn try_prepend_incremental(
        &mut self,
        messages: &[StoredChatMessage],
        old_key: VirtualLayoutKey,
        new_key: VirtualLayoutKey,
    ) -> bool {
        if old_key.last_id != new_key.last_id || old_key.first_id == new_key.first_id {
            return false;
        }

        let added = new_key.len - old_key.len;
        let Some(anchor) = messages.get(added) else {
            return false;
        };
        if anchor.id != old_key.first_id {
            return false;
        }

        let mut new_prefix = Vec::with_capacity(new_key.len + 1);
        new_prefix.push(0.0);
        let mut added_sum = 0.0f32;
        for message in &messages[..added] {
            let h = estimate_message_height_px(message, new_key.wrap_width_px as f32);
            added_sum += h;
            new_prefix.push(added_sum);
        }
        for old in self.prefix.iter().skip(1) {
            new_prefix.push(added_sum + *old);
        }

        let old_sum = self.sampled_avg_height_px * old_key.len as f32;
        self.sampled_avg_height_px = (old_sum + added_sum) / new_key.len as f32;
        self.prefix = new_prefix;
        self.key = Some(new_key);
        true
    }

    fn try_append_incremental(
        &mut self,
        messages: &[StoredChatMessage],
        old_key: VirtualLayoutKey,
        new_key: VirtualLayoutKey,
    ) -> bool {
        if old_key.first_id != new_key.first_id || old_key.last_id == new_key.last_id {
            return false;
        }
        let Some(anchor) = messages.get(old_key.len.saturating_sub(1)) else {
            return false;
        };
        if anchor.id != old_key.last_id {
            return false;
        }

        let mut tail_sum = 0.0f32;
        let mut running = self.prefix.last().copied().unwrap_or(0.0);
        for message in &messages[old_key.len..] {
            let h = estimate_message_height_px(message, new_key.wrap_width_px as f32);
            tail_sum += h;
            running += h;
            self.prefix.push(running);
        }

        let old_sum = self.sampled_avg_height_px * old_key.len as f32;
        self.sampled_avg_height_px = (old_sum + tail_sum) / new_key.len as f32;
        self.key = Some(new_key);
        true
    }

    fn make_key(messages: &[StoredChatMessage], wrap_width_px: i32) -> Option<VirtualLayoutKey> {
        let first = messages.first()?;
        let last = messages.last()?;
        Some(VirtualLayoutKey {
            len: messages.len(),
            first_id: first.id,
            last_id: last.id,
            wrap_width_px,
        })
    }
}

fn schedule_markdown_reflow_poll_with_interval(
    ui_weak: slint::Weak<MainWindow>,
    session: Arc<Mutex<ChatSessionService>>,
    last_wrap_width_px: Rc<Cell<i32>>,
    observed_scroll_bucket: Rc<Cell<i32>>,
    applied_scroll_bucket: Rc<Cell<i32>>,
    scroll_idle_polls: Rc<Cell<u8>>,
    scroll_coarse_cooldown_polls: Rc<Cell<u8>>,
    pending_precise_scroll_recompute: Rc<Cell<bool>>,
    observed_width_px: Rc<Cell<i32>>,
    pending_reflow_width_px: Rc<Cell<i32>>,
    stable_polls: Rc<Cell<u8>>,
    polling_active: Rc<Cell<bool>>,
    deferred_log_printed: Rc<Cell<bool>>,
    resize_activity_ticks: Rc<Cell<u8>>,
    interval_ms: u64,
) {
    slint::Timer::single_shot(std::time::Duration::from_millis(interval_ms), move || {
        if !polling_active.get() {
            return;
        }

        if let Some(ui) = ui_weak.upgrade() {
            if ui.get_current_route() != Route::Chat {
                polling_active.set(false);
                return;
            }

            if scroll_coarse_cooldown_polls.get() > 0 {
                scroll_coarse_cooldown_polls
                    .set(scroll_coarse_cooldown_polls.get().saturating_sub(1));
            }
            let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
            let is_near_top =
                ui.get_chat_scroll_position().abs() <= AUTO_LOAD_MORE_TOP_THRESHOLD_PX;

            if is_near_top && ui.get_chat_can_load_more_history() && !ui.get_chat_loading_history()
            {
                ui.invoke_chat_load_more_history();
            }

            let row_height = get_adaptive_row_height_px().max(72.0);
            let scroll_bucket = (scroll_offset_px / (row_height / 2.0)).floor() as i32;
            if scroll_bucket != observed_scroll_bucket.get() {
                observed_scroll_bucket.set(scroll_bucket);
                scroll_idle_polls.set(0);
                pending_precise_scroll_recompute.set(true);
                let applied = applied_scroll_bucket.get();
                if applied < 0 {
                    // 首次轮询仅做 bucket 校准，避免额外一次粗更新造成瞬时卡顿。
                    applied_scroll_bucket.set(scroll_bucket);
                    pending_precise_scroll_recompute.set(false);
                    scroll_coarse_cooldown_polls.set(0);
                    schedule_markdown_reflow_poll_with_interval(
                        ui_weak.clone(),
                        session.clone(),
                        last_wrap_width_px.clone(),
                        observed_scroll_bucket.clone(),
                        applied_scroll_bucket.clone(),
                        scroll_idle_polls.clone(),
                        scroll_coarse_cooldown_polls.clone(),
                        pending_precise_scroll_recompute.clone(),
                        observed_width_px.clone(),
                        pending_reflow_width_px.clone(),
                        stable_polls.clone(),
                        polling_active.clone(),
                        deferred_log_printed.clone(),
                        resize_activity_ticks.clone(),
                        REFLOW_POLL_INTERVAL_MS,
                    );
                    return;
                }
                let scroll_delta = (scroll_bucket - applied).abs();
                let should_coarse_update = scroll_delta >= SCROLL_FORCE_UPDATE_BUCKET_STEP;
                if should_coarse_update && scroll_coarse_cooldown_polls.get() == 0 {
                    apply_chat_state_to_ui(
                        &ui,
                        &session,
                        ui.get_chat_is_sending(),
                        Some(compute_wrap_width_px(&ui)),
                    );
                    applied_scroll_bucket.set(scroll_bucket);
                    pending_precise_scroll_recompute.set(false);
                    scroll_coarse_cooldown_polls.set(SCROLL_COARSE_UPDATE_COOLDOWN_POLLS);
                }
            } else if pending_precise_scroll_recompute.get() {
                let next_idle = scroll_idle_polls.get().saturating_add(1);
                scroll_idle_polls.set(next_idle);
                if next_idle >= SCROLL_IDLE_POLLS_REQUIRED {
                    apply_chat_state_to_ui(
                        &ui,
                        &session,
                        ui.get_chat_is_sending(),
                        Some(compute_wrap_width_px(&ui)),
                    );
                    applied_scroll_bucket.set(scroll_bucket);
                    pending_precise_scroll_recompute.set(false);
                    scroll_idle_polls.set(0);
                    scroll_coarse_cooldown_polls.set(0);
                }
            }

            let wrap_width_px = compute_wrap_width_px(&ui);
            if wrap_width_px != observed_width_px.get() {
                observed_width_px.set(wrap_width_px);
                pending_reflow_width_px.set(wrap_width_px);
                stable_polls.set(0);
                resize_activity_ticks.set(LOADING_PHASE_PAUSE_AFTER_RESIZE_TICKS);
            } else if pending_reflow_width_px.get() != -1 {
                stable_polls.set(stable_polls.get().saturating_add(1));
            }
            if resize_activity_ticks.get() > 0 {
                resize_activity_ticks.set(resize_activity_ticks.get().saturating_sub(1));
            }

            let is_window_interacting = crate::is_window_interacting();
            let should_reflow = pending_reflow_width_px.get() != -1
                && stable_polls.get() >= REFLOW_STABLE_POLLS_REQUIRED
                && pending_reflow_width_px.get() != last_wrap_width_px.get()
                && !is_window_interacting;

            if is_window_interacting && pending_reflow_width_px.get() != -1 {
                schedule_markdown_reflow_poll_with_interval(
                    ui_weak.clone(),
                    session.clone(),
                    last_wrap_width_px.clone(),
                    observed_scroll_bucket.clone(),
                    applied_scroll_bucket.clone(),
                    scroll_idle_polls.clone(),
                    scroll_coarse_cooldown_polls.clone(),
                    pending_precise_scroll_recompute.clone(),
                    observed_width_px.clone(),
                    pending_reflow_width_px.clone(),
                    stable_polls.clone(),
                    polling_active.clone(),
                    deferred_log_printed.clone(),
                    resize_activity_ticks.clone(),
                    50,
                );
                return;
            }

            if should_reflow {
                if ui.get_chat_is_sending() {
                    deferred_log_printed.set(true);
                    schedule_markdown_reflow_poll_with_interval(
                        ui_weak.clone(),
                        session.clone(),
                        last_wrap_width_px.clone(),
                        observed_scroll_bucket.clone(),
                        applied_scroll_bucket.clone(),
                        scroll_idle_polls.clone(),
                        scroll_coarse_cooldown_polls.clone(),
                        pending_precise_scroll_recompute.clone(),
                        observed_width_px.clone(),
                        pending_reflow_width_px.clone(),
                        stable_polls.clone(),
                        polling_active.clone(),
                        deferred_log_printed.clone(),
                        resize_activity_ticks.clone(),
                        REFLOW_POLL_INTERVAL_MS,
                    );
                    return;
                }

                deferred_log_printed.set(false);

                let target_width_px = pending_reflow_width_px.get();
                let width_delta = if last_wrap_width_px.get() < 0 {
                    i32::MAX
                } else {
                    (target_width_px - last_wrap_width_px.get()).abs()
                };

                if width_delta >= REFLOW_MIN_WIDTH_DELTA_PX {
                    apply_chat_state_to_ui(
                        &ui,
                        &session,
                        ui.get_chat_is_sending(),
                        Some(target_width_px),
                    );
                    applied_scroll_bucket.set(scroll_bucket);
                    pending_precise_scroll_recompute.set(false);
                    scroll_idle_polls.set(0);
                }

                last_wrap_width_px.set(target_width_px);
                pending_reflow_width_px.set(-1);
                stable_polls.set(0);
            }

            schedule_markdown_reflow_poll_with_interval(
                ui_weak.clone(),
                session.clone(),
                last_wrap_width_px.clone(),
                observed_scroll_bucket.clone(),
                applied_scroll_bucket.clone(),
                scroll_idle_polls.clone(),
                scroll_coarse_cooldown_polls.clone(),
                pending_precise_scroll_recompute.clone(),
                observed_width_px.clone(),
                pending_reflow_width_px.clone(),
                stable_polls.clone(),
                polling_active.clone(),
                deferred_log_printed.clone(),
                resize_activity_ticks.clone(),
                REFLOW_POLL_INTERVAL_MS,
            );
        } else {
            polling_active.set(false);
        }
    });
}

fn start_markdown_reflow_poll_if_needed(
    ui_weak: slint::Weak<MainWindow>,
    session: Arc<Mutex<ChatSessionService>>,
    last_wrap_width_px: Rc<Cell<i32>>,
    observed_scroll_bucket: Rc<Cell<i32>>,
    applied_scroll_bucket: Rc<Cell<i32>>,
    scroll_idle_polls: Rc<Cell<u8>>,
    scroll_coarse_cooldown_polls: Rc<Cell<u8>>,
    pending_precise_scroll_recompute: Rc<Cell<bool>>,
    observed_width_px: Rc<Cell<i32>>,
    pending_reflow_width_px: Rc<Cell<i32>>,
    stable_polls: Rc<Cell<u8>>,
    polling_active: Rc<Cell<bool>>,
    deferred_log_printed: Rc<Cell<bool>>,
    resize_activity_ticks: Rc<Cell<u8>>,
) {
    if polling_active.get() {
        return;
    }
    polling_active.set(true);
    schedule_markdown_reflow_poll_with_interval(
        ui_weak,
        session,
        last_wrap_width_px,
        observed_scroll_bucket,
        applied_scroll_bucket,
        scroll_idle_polls,
        scroll_coarse_cooldown_polls,
        pending_precise_scroll_recompute,
        observed_width_px,
        pending_reflow_width_px,
        stable_polls,
        polling_active,
        deferred_log_printed,
        resize_activity_ticks,
        0,
    );
}

fn schedule_loading_phase_tick(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
    resize_activity_ticks: Rc<Cell<u8>>,
) {
    slint::Timer::single_shot(
        std::time::Duration::from_millis(LOADING_PHASE_INTERVAL_MS),
        move || {
            if !loading_phase_active.load(Ordering::Relaxed) {
                return;
            }

            if let Some(ui) = ui_weak.upgrade() {
                if ui.get_chat_is_sending() && ui.get_current_route() == Route::Chat {
                    if resize_activity_ticks.get() == 0 {
                        let next = (ui.get_chat_loading_phase() + 1) % 3;
                        ui.set_chat_loading_phase(next);
                    }
                }
                schedule_loading_phase_tick(
                    ui_weak.clone(),
                    loading_phase_active.clone(),
                    resize_activity_ticks.clone(),
                );
            } else {
                loading_phase_active.store(false, Ordering::Relaxed);
            }
        },
    );
}

fn start_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
    resize_activity_ticks: Rc<Cell<u8>>,
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
    schedule_loading_phase_tick(ui_weak, loading_phase_active, resize_activity_ticks);
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
    let ui_weak_scroll = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(ui) = ui_weak_scroll.upgrade() {
            ui.set_chat_scroll_position(-99999.0);
        }
    });
}

/// 设置聊天交互逻辑和事件回调
pub fn setup_interactive(ui: &MainWindow) {
    log::info!("[Chat::Interactive] Setting up interactive callbacks...");
    let runtime = shared_runtime();

    let session = match ChatSessionService::bootstrap_default() {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            log::error!("[Chat::Storage] Failed to initialize chat storage: {}", e);
            return;
        }
    };

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
    let resize_activity_ticks = Rc::new(Cell::new(0u8));
    let resize_activity_ticks_send = resize_activity_ticks.clone();

    // 初始化首屏消息：若当前就在 Chat，首次进入直接定位到最新；否则仅做轻量初始化
    if ui.get_current_route() == Route::Chat {
        has_entered_chat_once.set(true);
        ui.set_chat_scroll_position(-99999.0);
        apply_chat_state_to_ui(ui, &session, false, Some(compute_wrap_width_px(ui)));
        scroll_chat_to_bottom(ui);
    } else {
        apply_chat_state_to_ui(ui, &session, false, None);
    }
    ui.set_chat_loading_history(false);

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
                let load_start = Instant::now();
                let io_result = tokio::task::spawn_blocking(move || {
                    let mut guard = session_for_task
                        .lock()
                        .map_err(|e| format!("session lock poisoned: {e}"))?;
                    guard
                        .load_more_history()
                        .map_err(|e| format!("load history failed: {e}"))
                })
                .await;
                let elapsed = load_start.elapsed();
                let min_duration = Duration::from_millis(LOAD_MORE_MIN_ANIMATION_MS);
                if elapsed < min_duration {
                    tokio::time::sleep(min_duration - elapsed).await;
                }

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak_for_task.upgrade() else {
                        return;
                    };

                    match io_result {
                        Ok(Ok(loaded_count)) => {
                            let wrap_width_px = compute_wrap_width_px(&ui);
                            let prepended_height_px = estimate_prepended_batch_height_px(
                                &session_for_ui,
                                loaded_count,
                                wrap_width_px,
                            );
                            // 触发加载后把视图锚定到“新增这批消息的底部”，避免立即连续触发下一批。
                            if prepended_height_px > 0.0 {
                                let target_offset = (prepended_height_px - 12.0).max(0.0);
                                ui.set_chat_scroll_position(-target_offset);
                            }
                            apply_chat_state_to_ui(
                                &ui,
                                &session_for_ui,
                                ui.get_chat_is_sending(),
                                Some(wrap_width_px),
                            );
                            log::debug!("[Chat::History] Older messages loaded and rendered");
                        }
                        Ok(Err(e)) => {
                            log::error!("[Chat::Storage] Failed to load older history: {}", e);
                        }
                        Err(e) => {
                            log::error!("[Chat::Storage] Background load task join failed: {}", e);
                        }
                    }

                    ui.set_chat_loading_history(false);
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

        start_loading_phase_animation(
            ui.as_weak(),
            loading_phase_active_send.clone(),
            resize_activity_ticks_send.clone(),
        );

        apply_chat_state_to_ui(&ui, &session_send, true, Some(compute_wrap_width_px(&ui)));
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
                        let content = format!("❌ {}", friendly);
                        if let Err(e) = guard.append_assistant_message(content) {
                            log::error!("[Chat::Storage] Failed to persist error message: {}", e);
                        }
                    }
                }
                drop(guard);

                ui.set_chat_is_sending(false);
                ui.set_chat_loading_phase(0);
                apply_chat_state_to_ui(
                    &ui,
                    &session_response,
                    false,
                    Some(compute_wrap_width_px(&ui)),
                );
                scroll_chat_to_bottom(&ui);
            });
        });
    });

    // 处理复制到剪贴板回调
    let ui_weak_copy = ui.as_weak();
    ui.on_chat_copy_to_clipboard(move |index: i32, text: slint::SharedString| {
        let text_str = text.to_string();

        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                if clipboard.set_text(&text_str).is_ok() {
                    if let Some(ui) = ui_weak_copy.upgrade() {
                        ui.set_chat_copied_message_index(index);

                        let ui_weak_timer = ui.as_weak();
                        slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
                            if let Some(ui) = ui_weak_timer.upgrade() {
                                ui.set_chat_copied_message_index(-1);
                            }
                        });
                    }
                }
            }
            Err(e) => {
                log::error!("[Chat::Copy] FAILED to create clipboard instance: {}", e);
            }
        }
    });

    // 处理打开链接回调（来自 Markdown 渲染块）
    ui.on_chat_open_link(move |url: slint::SharedString| {
        let url = url.to_string();
        if url.trim().is_empty() {
            return;
        }

        match webbrowser::open(&url) {
            Ok(_) => log::info!("[Chat::Link] Opened URL: {}", url),
            Err(e) => log::error!("[Chat::Link] Failed to open URL '{}': {}", url, e),
        }
    });

    // 仅在 Chat 页面可见时启动轮询；离开 Chat 立即停止
    let last_wrap_width_px = Rc::new(Cell::new(-1));
    let observed_scroll_bucket = Rc::new(Cell::new(-1));
    let applied_scroll_bucket = Rc::new(Cell::new(-1));
    let scroll_idle_polls = Rc::new(Cell::new(0u8));
    let scroll_coarse_cooldown_polls = Rc::new(Cell::new(0u8));
    let pending_precise_scroll_recompute = Rc::new(Cell::new(false));
    let observed_width_px = Rc::new(Cell::new(-1));
    let pending_reflow_width_px = Rc::new(Cell::new(-1));
    let stable_polls = Rc::new(Cell::new(0u8));
    let polling_active = Rc::new(Cell::new(false));
    let deferred_log_printed = Rc::new(Cell::new(false));

    // 预初始化 scroll bucket，避免首次拖动时触发一次“未初始化 -> 粗更新”峰值。
    let initial_scroll_bucket = {
        let row_height = get_adaptive_row_height_px().max(72.0);
        let scroll_offset_px = (-ui.get_chat_scroll_position()).max(0.0);
        (scroll_offset_px / (row_height / 2.0)).floor() as i32
    };
    observed_scroll_bucket.set(initial_scroll_bucket);
    applied_scroll_bucket.set(initial_scroll_bucket);

    if ui.get_current_route() == Route::Chat {
        start_markdown_reflow_poll_if_needed(
            ui.as_weak(),
            session.clone(),
            last_wrap_width_px.clone(),
            observed_scroll_bucket.clone(),
            applied_scroll_bucket.clone(),
            scroll_idle_polls.clone(),
            scroll_coarse_cooldown_polls.clone(),
            pending_precise_scroll_recompute.clone(),
            observed_width_px.clone(),
            pending_reflow_width_px.clone(),
            stable_polls.clone(),
            polling_active.clone(),
            deferred_log_printed.clone(),
            resize_activity_ticks.clone(),
        );
    }

    let ui_weak_route = ui.as_weak();
    let session_route = session.clone();
    let last_wrap_width_px_route = last_wrap_width_px.clone();
    let observed_scroll_bucket_route = observed_scroll_bucket.clone();
    let applied_scroll_bucket_route = applied_scroll_bucket.clone();
    let scroll_idle_polls_route = scroll_idle_polls.clone();
    let scroll_coarse_cooldown_polls_route = scroll_coarse_cooldown_polls.clone();
    let pending_precise_scroll_recompute_route = pending_precise_scroll_recompute.clone();
    let observed_width_px_route = observed_width_px.clone();
    let pending_reflow_width_px_route = pending_reflow_width_px.clone();
    let stable_polls_route = stable_polls.clone();
    let polling_active_route = polling_active.clone();
    let deferred_log_printed_route = deferred_log_printed.clone();
    let resize_activity_ticks_route = resize_activity_ticks.clone();
    let loading_phase_active_route = loading_phase_active.clone();
    let has_entered_chat_once_route = has_entered_chat_once.clone();

    ui.on_chat_route_visible_changed(move |visible| {
        if visible {
            if let Some(ui) = ui_weak_route.upgrade() {
                if !has_entered_chat_once_route.get() {
                    // 首次进入 Chat：优先显示最新消息并定位到底部
                    has_entered_chat_once_route.set(true);
                    ui.set_chat_scroll_position(-99999.0);
                    apply_chat_state_to_ui(
                        &ui,
                        &session_route,
                        ui.get_chat_is_sending(),
                        Some(compute_wrap_width_px(&ui)),
                    );
                    scroll_chat_to_bottom(&ui);
                } else {
                    // 非首次进入：保持并恢复上次滚动位置
                    apply_chat_state_to_ui(
                        &ui,
                        &session_route,
                        ui.get_chat_is_sending(),
                        Some(compute_wrap_width_px(&ui)),
                    );
                }
            }
            start_markdown_reflow_poll_if_needed(
                ui_weak_route.clone(),
                session_route.clone(),
                last_wrap_width_px_route.clone(),
                observed_scroll_bucket_route.clone(),
                applied_scroll_bucket_route.clone(),
                scroll_idle_polls_route.clone(),
                scroll_coarse_cooldown_polls_route.clone(),
                pending_precise_scroll_recompute_route.clone(),
                observed_width_px_route.clone(),
                pending_reflow_width_px_route.clone(),
                stable_polls_route.clone(),
                polling_active_route.clone(),
                deferred_log_printed_route.clone(),
                resize_activity_ticks_route.clone(),
            );
        } else {
            polling_active_route.set(false);
            deferred_log_printed_route.set(false);
            pending_precise_scroll_recompute_route.set(false);
            scroll_idle_polls_route.set(0);
            scroll_coarse_cooldown_polls_route.set(0);
            resize_activity_ticks_route.set(0);
            stop_loading_phase_animation(ui_weak_route.clone(), loading_phase_active_route.clone());
            LAST_RENDER_SIGNATURE.with(|last| {
                *last.borrow_mut() = None;
            });
            clear_render_cache();
        }
    });

    log::info!("[Chat::Interactive] Interactive callbacks initialized successfully");
}
