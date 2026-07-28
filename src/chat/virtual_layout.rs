//! Pure bounded-window layout for Chat history.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};

use crate::chat::markdown::looks_like_markdown;
use crate::chat::persistence::{StoredChatMessage, StoredMessageRole};
use crate::chat::session::ChatSessionService;

const VIRTUAL_ROW_ESTIMATED_HEIGHT_PX: f32 = 108.0;
const VIRTUAL_MIN_VISIBLE_ROWS: usize = 8;
const VIRTUAL_OVERSCAN_ROWS: usize = 40;
const VIRTUAL_MAX_WINDOW_ROWS: usize = 56;
const VIRTUAL_OVERSCAN_PX: f32 = 1200.0;

thread_local! {
    static ADAPTIVE_ROW_HEIGHT_PX: Cell<f32> = const { Cell::new(VIRTUAL_ROW_ESTIMATED_HEIGHT_PX) };
    static VIRTUAL_LAYOUT_CACHE: RefCell<VirtualLayoutCache> = RefCell::new(VirtualLayoutCache::new());
}

pub(super) fn get_adaptive_row_height_px() -> f32 {
    ADAPTIVE_ROW_HEIGHT_PX.with(Cell::get)
}

pub(super) fn smooth_adaptive_row_height_px(sample: f32) {
    let clamped = sample.clamp(72.0, 220.0);
    ADAPTIVE_ROW_HEIGHT_PX.with(|cell| {
        let current = cell.get();
        let next = current * 0.86 + clamped * 0.14;
        cell.set(next);
    });
}

pub(super) fn compute_virtual_window_with_estimates(
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

pub(super) fn estimate_prepended_batch_height_px(
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
pub(super) struct VirtualRenderSignature {
    pub(super) total_len: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) wrap_width_px: i32,
    pub(super) include_loading_placeholder: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::persistence::StoredMessageStatus;

    fn message(id: i64, content: impl Into<String>) -> StoredChatMessage {
        StoredChatMessage {
            id,
            role: StoredMessageRole::Assistant,
            status: StoredMessageStatus::Normal,
            content: content.into(),
            timestamp: "12:00".to_string(),
        }
    }

    #[test]
    fn empty_history_has_an_empty_window() {
        assert_eq!(
            compute_virtual_window_with_estimates(&[], 0.0, 600.0, 640, 108.0),
            (0, 0, 0.0, 0.0, 108.0)
        );
    }

    #[test]
    fn large_history_keeps_the_render_window_bounded() {
        let messages = (0..200)
            .map(|id| message(id, format!("message {id}")))
            .collect::<Vec<_>>();
        let (start, end, top, bottom, _) =
            compute_virtual_window_with_estimates(&messages, 8_000.0, 720.0, 640, 108.0);

        assert!(start < end);
        assert!(end - start <= VIRTUAL_MAX_WINDOW_ROWS);
        assert!(top > 0.0);
        assert!(bottom >= 0.0);
    }

    #[test]
    fn narrower_layout_estimates_more_height_for_long_messages() {
        let messages = vec![message(1, "long text ".repeat(120))];
        let (_, _, _, _, narrow) =
            compute_virtual_window_with_estimates(&messages, 0.0, 600.0, 260, 108.0);
        let (_, _, _, _, wide) =
            compute_virtual_window_with_estimates(&messages, 0.0, 600.0, 900, 108.0);

        assert!(narrow > wide);
    }
}
