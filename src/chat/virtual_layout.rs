//! Pure bounded-window layout for Chat history.

use crate::chat::persistence::{StoredChatMessage, StoredMessageRole};
use crate::chat::renderer::projection_for_layout;
use std::cell::{Cell, RefCell};

const VIRTUAL_ROW_ESTIMATED_HEIGHT_PX: f32 = 108.0;
const MESSAGE_SPACING_PX: f32 = 8.0;

thread_local! {
    static ADAPTIVE_ROW_HEIGHT_PX: Cell<f32> = const { Cell::new(VIRTUAL_ROW_ESTIMATED_HEIGHT_PX) };
    static VIRTUAL_LAYOUT_CACHE: RefCell<VirtualLayoutCache> = const { RefCell::new(VirtualLayoutCache::new()) };
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VirtualWindowPolicy {
    min_visible_rows: usize,
    max_window_rows: usize,
    overscan_px: f32,
    fill_window: bool,
    prefer_rows_before: bool,
    align_to_latest: bool,
}

impl VirtualWindowPolicy {
    pub(super) const fn baseline() -> Self {
        Self {
            min_visible_rows: 8,
            max_window_rows: 56,
            overscan_px: 1_200.0,
            fill_window: true,
            prefer_rows_before: false,
            align_to_latest: false,
        }
    }

    pub(super) const fn optimized() -> Self {
        Self {
            min_visible_rows: 8,
            max_window_rows: 32,
            overscan_px: 2_400.0,
            fill_window: false,
            prefer_rows_before: false,
            align_to_latest: false,
        }
    }

    pub(super) const fn direct_scrollbar_drag(moving_down: bool) -> Self {
        Self {
            min_visible_rows: 8,
            max_window_rows: 32,
            overscan_px: 0.0,
            fill_window: false,
            prefer_rows_before: !moving_down,
            align_to_latest: false,
        }
    }

    pub(super) fn for_latest_messages(mut self) -> Self {
        self.align_to_latest = true;
        self.prefer_rows_before = true;
        self
    }
}

pub(super) fn get_adaptive_row_height_px() -> f32 {
    ADAPTIVE_ROW_HEIGHT_PX.with(Cell::get)
}

pub(super) fn full_history_window(
    total: usize,
    row_estimate_px: f32,
) -> (usize, usize, f32, f32, f32) {
    (0, total, 0.0, 0.0, row_estimate_px)
}

pub(super) fn compute_history_window(
    messages: &[StoredChatMessage],
    virtualization_enabled: bool,
    scroll_offset_px: f32,
    viewport_height_px: f32,
    wrap_width_px: i32,
    row_estimate_px: f32,
    policy: VirtualWindowPolicy,
) -> (usize, usize, f32, f32, f32) {
    if virtualization_enabled {
        compute_virtual_window_with_estimates(
            messages,
            scroll_offset_px,
            viewport_height_px,
            wrap_width_px,
            row_estimate_px,
            policy,
        )
    } else {
        full_history_window(messages.len(), row_estimate_px)
    }
}

pub(super) fn smooth_adaptive_row_height_px(sample: f32) {
    let clamped = sample.clamp(72.0, 560.0);
    ADAPTIVE_ROW_HEIGHT_PX.with(|cell| {
        let current = cell.get();
        cell.set(current * 0.86 + clamped * 0.14);
    });
}

pub(super) fn compute_virtual_window_with_estimates(
    messages: &[StoredChatMessage],
    scroll_offset_px: f32,
    viewport_height_px: f32,
    wrap_width_px: i32,
    row_estimate_px: f32,
    policy: VirtualWindowPolicy,
) -> (usize, usize, f32, f32, f32) {
    let total = messages.len();
    if total == 0 {
        return (0, 0, 0.0, 0.0, row_estimate_px);
    }

    VIRTUAL_LAYOUT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let sampled_average = cache.ensure(messages, wrap_width_px);
        let prefix = cache.prefix_slice();
        // A new mode must mount its tail before the UI scrolls to the actual
        // bottom; the previous mode's offset does not locate this history.
        let scroll_offset_px = if policy.align_to_latest {
            (prefix[total] - viewport_height_px).max(0.0)
        } else {
            scroll_offset_px
        };
        let visible_rows = ((viewport_height_px / row_estimate_px.max(72.0)).ceil() as usize)
            .max(policy.min_visible_rows)
            .min(policy.max_window_rows);
        let visible_start = item_at_offset(prefix, scroll_offset_px.max(0.0));
        let visible_end =
            (item_at_offset(prefix, scroll_offset_px + viewport_height_px) + 1).min(total);
        let target_start = (scroll_offset_px - policy.overscan_px).max(0.0);
        let target_end = scroll_offset_px + viewport_height_px + policy.overscan_px;
        let mut start = item_at_offset(prefix, target_start);
        let mut end = (item_at_offset(prefix, target_end) + 1).min(total);

        let desired_rows = if policy.fill_window {
            policy.max_window_rows
        } else {
            visible_rows
        };
        if end.saturating_sub(start) > policy.max_window_rows {
            // Pixel overscan can span far more than the row cap when history
            // contains many short messages. Trim around the visible range,
            // never from the overscan start, or all mounted rows can end above
            // the viewport and expose the virtual bottom spacer.
            start = visible_start;
            end = visible_end;
            if end.saturating_sub(start) > policy.max_window_rows {
                if policy.prefer_rows_before {
                    start = end.saturating_sub(policy.max_window_rows);
                } else {
                    end = (start + policy.max_window_rows).min(total);
                }
            } else {
                expand_window(
                    &mut start,
                    &mut end,
                    policy.max_window_rows,
                    total,
                    policy.prefer_rows_before,
                );
            }
        } else {
            expand_window(
                &mut start,
                &mut end,
                desired_rows,
                total,
                policy.prefer_rows_before,
            );
        }

        let top_spacer = prefix[start];
        let document_height = prefix[total];
        let bottom_spacer = (document_height - prefix[end]).max(0.0);
        (start, end, top_spacer, bottom_spacer, sampled_average)
    })
}

pub(super) fn compute_spacers_for_fixed_window(
    messages: &[StoredChatMessage],
    start: usize,
    end: usize,
    wrap_width_px: i32,
    _optimized: bool,
) -> Option<(f32, f32, f32)> {
    if start > end || end > messages.len() {
        return None;
    }
    VIRTUAL_LAYOUT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let average = cache.ensure(messages, wrap_width_px);
        let prefix = cache.prefix_slice();
        Some((
            prefix[start],
            (prefix[messages.len()] - prefix[end]).max(0.0),
            average,
        ))
    })
}

pub(super) fn current_virtual_document_height_px() -> f32 {
    VIRTUAL_LAYOUT_CACHE.with(|cache| cache.borrow().prefix.last().copied().unwrap_or(0.0))
}

pub(super) fn clear_virtual_layout_cache() {
    VIRTUAL_LAYOUT_CACHE.with(|cache| *cache.borrow_mut() = VirtualLayoutCache::new());
    ADAPTIVE_ROW_HEIGHT_PX.with(|height| height.set(VIRTUAL_ROW_ESTIMATED_HEIGHT_PX));
}

fn expand_window(
    start: &mut usize,
    end: &mut usize,
    desired: usize,
    total: usize,
    prefer_rows_before: bool,
) {
    if end.saturating_sub(*start) >= desired {
        return;
    }
    let missing = desired - end.saturating_sub(*start);
    let before_target = if prefer_rows_before {
        missing.div_ceil(2)
    } else {
        missing / 2
    };
    let before = before_target.min(*start);
    *start -= before;
    *end = (*end + missing - before).min(total);
    if end.saturating_sub(*start) < desired {
        *start = end.saturating_sub(desired);
    }
}

fn item_at_offset(prefix: &[f32], target: f32) -> usize {
    if prefix.len() <= 1 {
        return 0;
    }
    let mut low = 0usize;
    let mut high = prefix.len() - 1;
    while low < high {
        let middle = (low + high) / 2;
        if prefix[middle] <= target {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low.saturating_sub(1).min(prefix.len() - 2)
}

fn estimate_message_height_px(message: &StoredChatMessage, wrap_width_px: f32) -> f32 {
    let bubble_width = wrap_width_px.clamp(220.0, 960.0);
    let text_width = (bubble_width - 24.0).max(1.0);
    let (projection, preview_available) = projection_for_layout(message);
    let measured_text_height = crate::chat::text_selection::measure_wrapped_system_text_height(
        projection.as_ref(),
        text_width,
        14.0,
    )
    .ceil();
    let timestamp_height = 14.0;
    let preview_height = if preview_available { 26.0 } else { 0.0 };
    let vertical_padding = 16.0;
    let role_minimum: f32 = match message.role {
        StoredMessageRole::User => 48.0,
        StoredMessageRole::Assistant => 58.0,
    };
    (measured_text_height + timestamp_height + preview_height + vertical_padding)
        .max(role_minimum)
        .min(4_000.0)
        + MESSAGE_SPACING_PX
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
    pub(super) include_loading_placeholder: bool,
}

struct VirtualLayoutCache {
    key: Option<VirtualLayoutKey>,
    prefix: Vec<f32>,
    sampled_avg_height_px: f32,
}

impl VirtualLayoutCache {
    const fn new() -> Self {
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
        if self.key == Some(key) && self.prefix.len() == key.len + 1 {
            return self.sampled_avg_height_px;
        }
        if !self.try_incremental_rebuild(messages, key) {
            self.rebuild(messages, key);
        }
        self.sampled_avg_height_px
    }

    fn prefix_slice(&self) -> &[f32] {
        &self.prefix
    }

    fn rebuild(&mut self, messages: &[StoredChatMessage], key: VirtualLayoutKey) {
        self.prefix.clear();
        self.prefix.reserve(messages.len() + 1);
        self.prefix.push(0.0);
        let mut sum = 0.0;
        for message in messages {
            sum += estimate_message_height_px(message, key.wrap_width_px as f32);
            self.prefix.push(sum);
        }
        self.sampled_avg_height_px = sum / messages.len() as f32;
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
        if old_key.wrap_width_px != new_key.wrap_width_px
            || self.prefix.len() != old_key.len + 1
            || new_key.len <= old_key.len
        {
            return false;
        }
        self.try_prepend(messages, old_key, new_key) || self.try_append(messages, old_key, new_key)
    }

    fn try_prepend(
        &mut self,
        messages: &[StoredChatMessage],
        old_key: VirtualLayoutKey,
        new_key: VirtualLayoutKey,
    ) -> bool {
        let added = new_key.len - old_key.len;
        if old_key.last_id != new_key.last_id
            || messages.get(added).map(|message| message.id) != Some(old_key.first_id)
        {
            return false;
        }
        let mut prefix = Vec::with_capacity(new_key.len + 1);
        prefix.push(0.0);
        let mut added_sum = 0.0;
        for message in &messages[..added] {
            added_sum += estimate_message_height_px(message, new_key.wrap_width_px as f32);
            prefix.push(added_sum);
        }
        prefix.extend(self.prefix.iter().skip(1).map(|old| added_sum + old));
        let old_sum = self.sampled_avg_height_px * old_key.len as f32;
        self.sampled_avg_height_px = (old_sum + added_sum) / new_key.len as f32;
        self.prefix = prefix;
        self.key = Some(new_key);
        true
    }

    fn try_append(
        &mut self,
        messages: &[StoredChatMessage],
        old_key: VirtualLayoutKey,
        new_key: VirtualLayoutKey,
    ) -> bool {
        if old_key.first_id != new_key.first_id
            || messages
                .get(old_key.len.saturating_sub(1))
                .map(|message| message.id)
                != Some(old_key.last_id)
        {
            return false;
        }
        let mut tail_sum = 0.0;
        let mut running = self.prefix.last().copied().unwrap_or(0.0);
        for message in &messages[old_key.len..] {
            let height = estimate_message_height_px(message, new_key.wrap_width_px as f32);
            tail_sum += height;
            running += height;
            self.prefix.push(running);
        }
        let old_sum = self.sampled_avg_height_px * old_key.len as f32;
        self.sampled_avg_height_px = (old_sum + tail_sum) / new_key.len as f32;
        self.key = Some(new_key);
        true
    }

    fn make_key(messages: &[StoredChatMessage], wrap_width_px: i32) -> Option<VirtualLayoutKey> {
        Some(VirtualLayoutKey {
            len: messages.len(),
            first_id: messages.first()?.id,
            last_id: messages.last()?.id,
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
            elapsed_ms: None,
        }
    }

    #[test]
    fn empty_history_has_an_empty_window() {
        assert_eq!(
            compute_virtual_window_with_estimates(
                &[],
                0.0,
                600.0,
                640,
                108.0,
                VirtualWindowPolicy::optimized(),
            ),
            (0, 0, 0.0, 0.0, 108.0)
        );
    }

    #[test]
    fn production_history_window_never_exposes_virtual_spacers() {
        let messages = (0..180)
            .map(|id| {
                let content = if id % 3 == 0 {
                    "large message line\n".repeat(120)
                } else {
                    format!("short message {id}")
                };
                message(id, content)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            compute_history_window(
                &messages,
                false,
                48_000.0,
                680.0,
                640,
                108.0,
                VirtualWindowPolicy::optimized(),
            ),
            (0, 180, 0.0, 0.0, 108.0)
        );
    }

    #[test]
    fn production_enabled_history_window_stays_bounded_on_the_real_scroll_path() {
        // Virtualization is now enabled by default in production (no fixture
        // data involved): the enabled path must return the same bounded window
        // as the virtual computation instead of mounting the full history.
        let messages = (0..200)
            .map(|id| message(id, format!("message {id}")))
            .collect::<Vec<_>>();

        clear_virtual_layout_cache();
        let expected = compute_virtual_window_with_estimates(
            &messages,
            6_000.0,
            720.0,
            640,
            108.0,
            VirtualWindowPolicy::optimized(),
        );

        clear_virtual_layout_cache();
        let actual = compute_history_window(
            &messages,
            true,
            6_000.0,
            720.0,
            640,
            108.0,
            VirtualWindowPolicy::optimized(),
        );

        assert_eq!(actual, expected);
        let (start, end, top_spacer, bottom_spacer, _) = actual;
        assert!(start > 0, "scrolled window must not start at the top");
        assert!(end - start <= 32, "window must stay bounded");
        assert!(end < messages.len(), "window must not mount the full tail");
        assert!(top_spacer > 0.0);
        assert!(bottom_spacer > 0.0);
        assert!(current_virtual_document_height_px() > top_spacer + bottom_spacer);
    }

    #[test]
    fn optimized_history_keeps_the_render_window_bounded() {
        clear_virtual_layout_cache();
        let messages = (0..200)
            .map(|id| message(id, format!("message {id}")))
            .collect::<Vec<_>>();
        let (start, end, top, bottom, _) = compute_virtual_window_with_estimates(
            &messages,
            8_000.0,
            720.0,
            640,
            108.0,
            VirtualWindowPolicy::optimized(),
        );
        assert!(start < end);
        assert!(end - start <= 32);
        assert!(top > 0.0);
        assert!(bottom >= 0.0);
    }

    #[test]
    fn optimized_row_cap_keeps_the_estimated_viewport_mounted() {
        clear_virtual_layout_cache();
        let messages = (0..200)
            .map(|id| message(id, "short row"))
            .collect::<Vec<_>>();
        let scroll_offset = 6_000.0;
        let viewport_height = 720.0;
        let (start, end, top, bottom, _) = compute_virtual_window_with_estimates(
            &messages,
            scroll_offset,
            viewport_height,
            640,
            108.0,
            VirtualWindowPolicy::optimized(),
        );
        let document_height = current_virtual_document_height_px();

        assert!(end - start <= 32);
        assert!(top <= scroll_offset);
        assert!(document_height - bottom >= scroll_offset + viewport_height);
    }

    #[test]
    fn direct_scrollbar_drag_covers_a_tall_viewport_without_normal_overscan() {
        clear_virtual_layout_cache();
        let messages = (0..200)
            .map(|id| message(id, "short row"))
            .collect::<Vec<_>>();
        let scroll_offset = 4_800.0;
        let viewport_height = 1_400.0;
        let (start, end, top, bottom, _) = compute_virtual_window_with_estimates(
            &messages,
            scroll_offset,
            viewport_height,
            640,
            108.0,
            VirtualWindowPolicy::direct_scrollbar_drag(false),
        );
        let document_height = current_virtual_document_height_px();

        assert!(end - start <= 32);
        assert!(top <= scroll_offset);
        assert!(document_height - bottom >= scroll_offset + viewport_height);
    }

    #[test]
    fn direct_scrollbar_drag_covers_a_large_row_viewport() {
        clear_virtual_layout_cache();
        let large_content = "large message line\n".repeat(120);
        let messages = (0..40)
            .map(|id| message(id, large_content.clone()))
            .collect::<Vec<_>>();
        let scroll_offset = 12_000.0;
        let viewport_height = 680.0;
        let (start, end, top, bottom, _) = compute_virtual_window_with_estimates(
            &messages,
            scroll_offset,
            viewport_height,
            640,
            560.0,
            VirtualWindowPolicy::direct_scrollbar_drag(false),
        );
        let document_height = current_virtual_document_height_px();

        assert!(end - start <= 8);
        assert!(top <= scroll_offset);
        assert!(document_height - bottom >= scroll_offset + viewport_height);
    }

    #[test]
    fn direct_drag_expansion_biases_the_extra_row_toward_motion() {
        let (mut up_start, mut up_end) = (10, 12);
        expand_window(&mut up_start, &mut up_end, 5, 30, true);
        assert_eq!((up_start, up_end), (8, 13));

        let (mut down_start, mut down_end) = (10, 12);
        expand_window(&mut down_start, &mut down_end, 5, 30, false);
        assert_eq!((down_start, down_end), (9, 14));
    }

    #[test]
    fn fixed_window_spacers_share_the_document_extent() {
        clear_virtual_layout_cache();
        let messages = (0..20)
            .map(|id| message(id, "message body"))
            .collect::<Vec<_>>();
        let (top, bottom, _) =
            compute_spacers_for_fixed_window(&messages, 5, 10, 640, true).unwrap();
        assert!(top > 0.0);
        assert!(bottom > 0.0);
        assert!(current_virtual_document_height_px() > top + bottom);
    }

    #[test]
    fn final_window_has_no_virtual_bottom_extent() {
        clear_virtual_layout_cache();
        let messages = (0..20)
            .map(|id| message(id, "message body"))
            .collect::<Vec<_>>();
        let (top, bottom, _) =
            compute_spacers_for_fixed_window(&messages, 12, messages.len(), 640, true).unwrap();

        assert!(top > 0.0);
        assert_eq!(bottom, 0.0);
    }
}
