//! Pure refresh throttling policy for terminal UI snapshots.

use std::time::{Duration, Instant};

pub(super) fn next_refresh_delay(
    now: Instant,
    last_applied_at: Option<Instant>,
    frame_interval: Duration,
) -> Duration {
    last_applied_at
        .and_then(|last| {
            let elapsed = now.saturating_duration_since(last);
            if elapsed < frame_interval {
                Some(frame_interval - elapsed)
            } else {
                None
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_is_immediate_without_a_previous_frame() {
        assert_eq!(
            next_refresh_delay(Instant::now(), None, Duration::from_millis(16)),
            Duration::ZERO
        );
    }

    #[test]
    fn refresh_waits_only_for_the_remaining_frame_budget() {
        let start = Instant::now();
        let interval = Duration::from_millis(16);
        assert_eq!(
            next_refresh_delay(start + Duration::from_millis(6), Some(start), interval),
            Duration::from_millis(10)
        );
        assert_eq!(
            next_refresh_delay(start + Duration::from_millis(20), Some(start), interval),
            Duration::ZERO
        );
    }
}
