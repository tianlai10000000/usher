//! Pure viewport coverage, bounded window shifts and anchor settlement policy.
use crate::chat::session::ChatMode;
use crate::chat::virtual_layout::VirtualRenderSignature;
use std::collections::HashMap;
const WRAP_WIDTH_TUNING_FACTOR: f32 = 1.10;
pub(super) const VIRTUAL_SCROLL_BUCKET_HEIGHT_PX: f32 = 64.0;
pub(super) const BASELINE_SCROLL_UPDATE_BUCKET_STEP: i32 = 16;
pub(super) const OPTIMIZED_SCROLL_UPDATE_BUCKET_STEP: i32 = 16;
pub(super) const AUTO_LOAD_MORE_TOP_THRESHOLD_PX: f32 = 160.0;
pub(super) const AUTO_LOAD_MORE_REARM_THRESHOLD_PX: f32 = 320.0;
pub(super) const MOUNTED_EDGE_GUARD_PX: f32 = 2400.0;
pub(super) const LAYOUT_SETTLE_DELAY_MS: u64 = 80;
pub(super) const ANCHOR_SETTLE_INTERVAL_MS: u64 = 16;
pub(super) const ANCHOR_STABLE_FRAMES: u8 = 2;
pub(super) const ANCHOR_MAX_FRAMES: u8 = 16;
pub(super) const ANCHOR_EPSILON_PX: f32 = 0.5;
pub(super) const MAX_NEW_ROWS_PER_SCROLL_COMMIT: usize = 2;

pub(super) fn virtual_scroll_bucket(scroll_offset_px: f32) -> i32 {
    (scroll_offset_px.max(0.0) / VIRTUAL_SCROLL_BUCKET_HEIGHT_PX).floor() as i32
}

pub(super) fn should_shift_virtual_window(
    applied_bucket: i32,
    observed_bucket: i32,
    step: i32,
) -> bool {
    (observed_bucket - applied_bucket).abs() >= step
}

pub(super) fn should_shift_near_mounted_edge(
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

pub(super) fn should_shift_near_estimated_edge(
    top_spacer_px: f32,
    bottom_spacer_px: f32,
    document_height_px: f32,
    scroll_offset_px: f32,
    viewport_height_px: f32,
    moving_down: bool,
    has_more_in_direction: bool,
) -> bool {
    if !has_more_in_direction || document_height_px <= 0.0 {
        return false;
    }
    let mounted_top = top_spacer_px.max(0.0);
    let mounted_bottom = (document_height_px - bottom_spacer_px).max(mounted_top);
    // This is the geometry-callback fallback, not a second overscan guard.
    // Trigger only at the estimated boundary so it cannot multiply commits.
    if moving_down {
        scroll_offset_px + viewport_height_px + 1.0 >= mounted_bottom
    } else {
        scroll_offset_px - 1.0 <= mounted_top
    }
}

pub(super) fn estimated_window_covers_viewport(
    top_spacer_px: f32,
    bottom_spacer_px: f32,
    document_height_px: f32,
    scroll_offset_px: f32,
    viewport_height_px: f32,
) -> bool {
    const COVERAGE_TOLERANCE_PX: f32 = 1.0;
    let mounted_bottom = (document_height_px - bottom_spacer_px).max(top_spacer_px);
    top_spacer_px <= scroll_offset_px + COVERAGE_TOLERANCE_PX
        && mounted_bottom + COVERAGE_TOLERANCE_PX >= scroll_offset_px + viewport_height_px
}

pub(super) fn limit_incremental_virtual_shift(
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

pub(super) fn scroll_update_bucket_step() -> i32 {
    if crate::chat::performance::variant().is_optimized() {
        OPTIMIZED_SCROLL_UPDATE_BUCKET_STEP
    } else {
        BASELINE_SCROLL_UPDATE_BUCKET_STEP
    }
}

pub(super) fn update_auto_load_armed(armed: bool, scroll_offset_px: f32) -> (bool, bool) {
    if scroll_offset_px > AUTO_LOAD_MORE_REARM_THRESHOLD_PX {
        (true, false)
    } else if armed && scroll_offset_px <= AUTO_LOAD_MORE_TOP_THRESHOLD_PX {
        (false, true)
    } else {
        (armed, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MessageGeometry {
    pub(super) y: f32,
    pub(super) height: f32,
    pub(super) revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct VisibleAnchor {
    pub(super) message_id: i32,
    pub(super) content_y: f32,
    pub(super) scroll_offset: f32,
    pub(super) geometry_revision: u64,
}

pub(super) fn select_visible_anchor(
    geometries: &HashMap<i32, MessageGeometry>,
    scroll_offset: f32,
    viewport_height: f32,
) -> Option<VisibleAnchor> {
    let viewport_bottom = scroll_offset + viewport_height.max(1.0);
    geometries
        .iter()
        .filter(|(_, geometry)| geometry.height > 0.0)
        .min_by(|(left_id, left), (right_id, right)| {
            let rank = |geometry: &MessageGeometry| {
                let bottom = geometry.y + geometry.height;
                if geometry.y >= scroll_offset - 1.0 && bottom <= viewport_bottom + 1.0 {
                    0
                } else if bottom > scroll_offset && geometry.y < viewport_bottom {
                    1
                } else if geometry.y >= viewport_bottom {
                    2
                } else {
                    3
                }
            };
            let left_rank = rank(left);
            let right_rank = rank(right);
            left_rank
                .cmp(&right_rank)
                .then_with(|| {
                    if left_rank == 3 {
                        right.y.total_cmp(&left.y)
                    } else {
                        left.y.total_cmp(&right.y)
                    }
                })
                .then_with(|| left_id.cmp(right_id))
        })
        .map(|(message_id, geometry)| VisibleAnchor {
            message_id: *message_id,
            content_y: geometry.y,
            scroll_offset,
            geometry_revision: geometry.revision,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnchorPurpose {
    HistoryPrepend,
    SettledLayout,
    VirtualWindow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct AnchorSettle {
    pub(super) generation: u64,
    pub(super) mode: ChatMode,
    pub(super) purpose: AnchorPurpose,
    pub(super) message_id: i32,
    pub(super) viewport_y: f32,
    pub(super) original_scroll_offset: f32,
    pub(super) expected_scroll_offset: f32,
    pub(super) capture_geometry_revision: u64,
    pub(super) input_revision: u64,
    pub(super) last_observed_y: Option<f32>,
    pub(super) stable_frames: u8,
    pub(super) frames_left: u8,
    pub(super) max_correction_px: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum AnchorStep {
    Wait,
    Write(f32),
    Complete,
    Cancel,
}

impl AnchorSettle {
    pub(super) fn new(
        generation: u64,
        mode: ChatMode,
        purpose: AnchorPurpose,
        anchor: VisibleAnchor,
        input_revision: u64,
        max_correction_px: Option<f32>,
    ) -> Self {
        Self {
            generation,
            mode,
            purpose,
            message_id: anchor.message_id,
            viewport_y: anchor.content_y - anchor.scroll_offset,
            original_scroll_offset: anchor.scroll_offset,
            expected_scroll_offset: anchor.scroll_offset,
            capture_geometry_revision: anchor.geometry_revision,
            input_revision,
            last_observed_y: None,
            stable_frames: 0,
            frames_left: ANCHOR_MAX_FRAMES,
            max_correction_px,
        }
    }

    pub(super) fn step(
        &mut self,
        mode: ChatMode,
        input_revision: u64,
        current_scroll_offset: f32,
        observation: Option<MessageGeometry>,
    ) -> AnchorStep {
        if self.mode != mode
            || self.input_revision != input_revision
            || (current_scroll_offset - self.expected_scroll_offset).abs() > 1.0
            || self.frames_left == 0
        {
            return AnchorStep::Cancel;
        }
        self.frames_left -= 1;

        let Some(observation) = observation else {
            return AnchorStep::Wait;
        };
        if observation.revision <= self.capture_geometry_revision {
            return AnchorStep::Wait;
        }

        let target = (observation.y - self.viewport_y).max(0.0);
        if self
            .max_correction_px
            .is_some_and(|maximum| (target - self.original_scroll_offset).abs() > maximum)
        {
            return AnchorStep::Cancel;
        }

        if (target - current_scroll_offset).abs() > ANCHOR_EPSILON_PX {
            self.expected_scroll_offset = target;
            self.last_observed_y = Some(observation.y);
            self.stable_frames = 0;
            return AnchorStep::Write(target);
        }

        let geometry_is_stable = self
            .last_observed_y
            .is_some_and(|previous| (previous - observation.y).abs() <= ANCHOR_EPSILON_PX);
        self.expected_scroll_offset = target;
        self.last_observed_y = Some(observation.y);
        if geometry_is_stable {
            self.stable_frames = self.stable_frames.saturating_add(1);
            if self.stable_frames >= ANCHOR_STABLE_FRAMES {
                return AnchorStep::Complete;
            }
        } else {
            self.stable_frames = 0;
        }
        AnchorStep::Wait
    }
}

pub(super) fn shifted_prepend_window(
    previous: VirtualRenderSignature,
    total_len: usize,
    anchor_index: usize,
) -> Option<(usize, usize)> {
    if total_len <= previous.total_len || previous.start >= previous.end {
        return None;
    }
    let inserted = total_len - previous.total_len;
    let old_start = previous.start.checked_add(inserted)?;
    let old_end = previous.end.checked_add(inserted)?;
    if old_end > total_len || !(old_start..old_end).contains(&anchor_index) {
        return None;
    }
    // Materialize the page boundary as well as the reading anchor. Hiding the
    // entire page in a spacer preserves a coordinate but leaves nothing to read.
    // Keep room for the old visible rows even if the new page contains tall rows.
    let start = old_start
        .saturating_sub(inserted.min(16))
        .max(anchor_index.saturating_sub(16));
    let end = old_end.min(start + 32).max(anchor_index + 1);
    Some((start, end))
}

pub(super) fn compute_wrap_width_for_chat_width(chat_width: f32) -> i32 {
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
