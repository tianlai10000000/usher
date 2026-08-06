//! Framework-neutral text search and match navigation primitives.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchCase {
    Sensitive,
    Insensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMatch {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisRange {
    pub start: f32,
    pub extent: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollViewport {
    /// Positive content offset at the leading edge of the viewport.
    pub offset: f32,
    pub extent: f32,
    pub content_extent: f32,
}

/// Result of one measured reveal reconciliation pass.
///
/// This mirrors the convergence used by dynamic virtualizers: estimates may
/// mount the target, but measured geometry is reconciled on subsequent layout
/// frames until both the target offset and the actual viewport are stable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RevealConvergence {
    pub target_offset: f32,
    pub stable_frames: u8,
    pub complete: bool,
}

/// Sub-pixel tolerance used while a platform scroll view rounds offsets.
pub const REVEAL_OFFSET_TOLERANCE: f32 = 1.01;

/// Two observations keep a transient pre-layout measurement from completing a
/// reveal request. The final write still uses the exact computed offset.
pub const REVEAL_REQUIRED_STABLE_FRAMES: u8 = 2;

/// Finds non-overlapping matches in source order, matching browser Find-next
/// progression. Unicode lowercase expansion is mapped back to original byte
/// and character boundaries instead of indexing the original with folded bytes.
pub fn find_non_overlapping(haystack: &str, needle: &str, match_case: MatchCase) -> Vec<TextMatch> {
    if needle.is_empty() || haystack.is_empty() {
        return Vec::new();
    }
    match match_case {
        MatchCase::Sensitive => find_sensitive(haystack, needle),
        MatchCase::Insensitive => find_insensitive(haystack, needle),
    }
}

/// Returns the adjacent result with browser-style wraparound.
pub fn step_match_index(
    current: Option<usize>,
    match_count: usize,
    direction: SearchDirection,
) -> Option<usize> {
    if match_count == 0 {
        return None;
    }
    Some(match direction {
        SearchDirection::Forward => current
            .filter(|index| *index < match_count)
            .map_or(0, |index| (index + 1) % match_count),
        SearchDirection::Backward => current.map_or(match_count - 1, |index| {
            if index == 0 || index >= match_count {
                match_count - 1
            } else {
                index - 1
            }
        }),
    })
}

/// Chromium/Blink-style CenterIfNeeded on one scroll axis.
///
/// A fully visible active match preserves the viewport exactly. A partially or
/// fully hidden match is centered, then clamped at the content boundaries.
pub fn center_if_needed(target: AxisRange, viewport: ScrollViewport) -> f32 {
    let viewport_extent = viewport.extent.max(1.0);
    let content_extent = viewport.content_extent.max(viewport_extent);
    let maximum_offset = content_extent - viewport_extent;
    let current_offset = viewport.offset.clamp(0.0, maximum_offset);
    let target_extent = target.extent.max(1.0);
    let target_end = target.start + target_extent;
    if target.start >= current_offset && target_end <= current_offset + viewport_extent {
        return current_offset;
    }
    (target.start + target_extent / 2.0 - viewport_extent / 2.0).clamp(0.0, maximum_offset)
}

/// Reconciles a measured target rectangle against the current viewport.
///
/// `last_target_offset` and `stable_frames` belong to the active reveal
/// request. Callers must reset both when the target identity or layout epoch
/// changes, and must reject measurements from older requests before calling
/// this function.
pub fn converge_center_if_needed(
    target: AxisRange,
    viewport: ScrollViewport,
    last_target_offset: Option<f32>,
    stable_frames: u8,
) -> RevealConvergence {
    let target_offset = center_if_needed(target, viewport);
    let target_unchanged =
        last_target_offset.is_some_and(|last| approximately_equal(last, target_offset));
    let viewport_at_target = approximately_equal(viewport.offset, target_offset);
    let stable_frames = if target_unchanged && viewport_at_target {
        stable_frames.saturating_add(1)
    } else {
        0
    };

    RevealConvergence {
        target_offset,
        stable_frames,
        complete: stable_frames >= REVEAL_REQUIRED_STABLE_FRAMES,
    }
}

fn approximately_equal(left: f32, right: f32) -> bool {
    (left - right).abs() < REVEAL_OFFSET_TOLERANCE
}

fn find_sensitive(haystack: &str, needle: &str) -> Vec<TextMatch> {
    let mut matches = Vec::new();
    let mut search_from = 0usize;
    while let Some(relative) = haystack[search_from..].find(needle) {
        let start_byte = search_from + relative;
        let end_byte = start_byte + needle.len();
        matches.push(to_text_match(haystack, start_byte, end_byte));
        search_from = end_byte;
    }
    matches
}

fn find_insensitive(haystack: &str, needle: &str) -> Vec<TextMatch> {
    let (folded_haystack, boundaries) = fold_with_boundaries(haystack);
    let folded_needle = needle.to_lowercase();
    if folded_needle.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut search_from = 0usize;
    while let Some(relative) = folded_haystack[search_from..].find(&folded_needle) {
        let folded_start = search_from + relative;
        let folded_end = folded_start + folded_needle.len();
        let start = boundaries
            .binary_search_by_key(&folded_start, |boundary| boundary.folded_byte)
            .ok()
            .map(|index| boundaries[index]);
        let end = boundaries
            .binary_search_by_key(&folded_end, |boundary| boundary.folded_byte)
            .ok()
            .map(|index| boundaries[index]);
        if let (Some(start), Some(end)) = (start, end) {
            matches.push(TextMatch {
                start_byte: start.original_byte,
                end_byte: end.original_byte,
                start_char: start.original_char,
                end_char: end.original_char,
            });
        }
        search_from = folded_end.max(folded_start + 1);
    }
    matches
}

#[derive(Debug, Clone, Copy)]
struct FoldBoundary {
    folded_byte: usize,
    original_byte: usize,
    original_char: usize,
}

fn fold_with_boundaries(value: &str) -> (String, Vec<FoldBoundary>) {
    let mut folded = String::new();
    let mut boundaries = Vec::with_capacity(value.chars().count() + 1);
    for (original_char, (original_byte, character)) in value.char_indices().enumerate() {
        boundaries.push(FoldBoundary {
            folded_byte: folded.len(),
            original_byte,
            original_char,
        });
        folded.extend(character.to_lowercase());
    }
    boundaries.push(FoldBoundary {
        folded_byte: folded.len(),
        original_byte: value.len(),
        original_char: value.chars().count(),
    });
    (folded, boundaries)
}

fn to_text_match(haystack: &str, start_byte: usize, end_byte: usize) -> TextMatch {
    TextMatch {
        start_byte,
        end_byte,
        start_char: haystack[..start_byte].chars().count(),
        end_char: haystack[..end_byte].chars().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_matches_are_non_overlapping_and_source_ordered() {
        let matches = find_non_overlapping("find find find", "find", MatchCase::Sensitive);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[1].start_byte, 5);
        assert_eq!(matches[2].start_char, 10);
    }

    #[test]
    fn insensitive_matches_map_folded_unicode_to_original_boundaries() {
        let matches = find_non_overlapping("İSTANBUL istanbul", "i̇stanbul", MatchCase::Insensitive);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            &"İSTANBUL istanbul"[matches[0].start_byte..matches[0].end_byte],
            "İSTANBUL"
        );
    }

    #[test]
    fn adjacent_navigation_wraps_in_both_directions() {
        assert_eq!(
            step_match_index(Some(2), 3, SearchDirection::Forward),
            Some(0)
        );
        assert_eq!(
            step_match_index(Some(0), 3, SearchDirection::Backward),
            Some(2)
        );
        assert_eq!(step_match_index(None, 3, SearchDirection::Forward), Some(0));
        assert_eq!(
            step_match_index(Some(4), 3, SearchDirection::Forward),
            Some(0)
        );
        assert_eq!(step_match_index(None, 0, SearchDirection::Forward), None);
    }

    #[test]
    fn center_if_needed_preserves_visible_and_centers_hidden_targets() {
        let viewport = ScrollViewport {
            offset: 100.0,
            extent: 200.0,
            content_extent: 800.0,
        };
        assert_eq!(
            center_if_needed(
                AxisRange {
                    start: 140.0,
                    extent: 22.0
                },
                viewport
            ),
            100.0
        );
        assert_eq!(
            center_if_needed(
                AxisRange {
                    start: 400.0,
                    extent: 22.0
                },
                viewport
            ),
            311.0
        );
        assert_eq!(
            center_if_needed(
                AxisRange {
                    start: 790.0,
                    extent: 22.0
                },
                ScrollViewport {
                    offset: 0.0,
                    ..viewport
                }
            ),
            600.0
        );
    }

    #[test]
    fn measured_reveal_waits_for_consecutive_stable_layout_frames() {
        let target = AxisRange {
            start: 400.0,
            extent: 22.0,
        };
        let viewport = ScrollViewport {
            offset: 100.0,
            extent: 200.0,
            content_extent: 800.0,
        };

        let first = converge_center_if_needed(target, viewport, None, 0);
        assert_eq!(first.target_offset, 311.0);
        assert_eq!(first.stable_frames, 0);
        assert!(!first.complete);

        let second = converge_center_if_needed(
            target,
            ScrollViewport {
                offset: first.target_offset,
                ..viewport
            },
            Some(first.target_offset),
            first.stable_frames,
        );
        assert_eq!(second.stable_frames, 1);
        assert!(!second.complete);

        let third = converge_center_if_needed(
            target,
            ScrollViewport {
                offset: second.target_offset,
                ..viewport
            },
            Some(second.target_offset),
            second.stable_frames,
        );
        assert_eq!(third.stable_frames, REVEAL_REQUIRED_STABLE_FRAMES);
        assert!(third.complete);
    }

    #[test]
    fn measured_reveal_resets_when_reflow_moves_the_target() {
        let viewport = ScrollViewport {
            offset: 311.0,
            extent: 200.0,
            content_extent: 900.0,
        };
        let result = converge_center_if_needed(
            AxisRange {
                start: 500.0,
                extent: 22.0,
            },
            viewport,
            Some(311.0),
            1,
        );

        assert_eq!(result.target_offset, 411.0);
        assert_eq!(result.stable_frames, 0);
        assert!(!result.complete);
    }
}
