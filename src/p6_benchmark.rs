//! Duration control shared only by explicitly enabled offline P6 fixtures.
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn clock() -> (Instant, Option<Duration>) {
    static CLOCK: OnceLock<(Instant, Option<Duration>)> = OnceLock::new();
    *CLOCK.get_or_init(|| {
        let seconds = std::env::var("USHER_P6_SOAK_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| (30..=3600).contains(s));
        (Instant::now(), seconds.map(Duration::from_secs))
    })
}

pub(crate) fn heartbeat_active(legacy_seconds: u64) -> bool {
    let (start, duration) = clock();
    start.elapsed()
        < duration.map_or(Duration::from_secs(legacy_seconds), |d| {
            d + Duration::from_secs(60)
        })
}

pub(crate) fn next_cycle(completed: usize) -> bool {
    let (start, duration) = clock();
    if let Some(duration) = duration {
        log::info!(
            "[P6][{}] progress cycles={completed} elapsed_ms={}",
            std::env::var("USHER_P6_BENCHMARK").unwrap_or_default(),
            start.elapsed().as_millis()
        );
        completed < 3 || start.elapsed() < duration
    } else {
        completed < 3
    }
}
