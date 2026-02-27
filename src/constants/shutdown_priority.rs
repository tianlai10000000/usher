#![allow(dead_code)]

/// Shutdown task priority levels.
///
/// Higher value means higher priority (executed earlier within the same stage).
/// If priority is equal, later-registered tasks run first (LIFO for tie-break).
///
/// Recommended usage template for business modules:
/// - `PRIORITY_CRITICAL`:
///   - Must run first to avoid data corruption or resource leaks.
///   - Examples: stop background workers that may still write state,
///     flush critical data, release exclusive process/file locks.
/// - `PRIORITY_HIGH`:
///   - Strongly recommended to run early, but not hard-fatal if delayed.
///   - Examples: stop schedulers/timers, cancel long-running network tasks,
///     detach OS-level integrations (tray/hotkey/hooks).
/// - `PRIORITY_NORMAL`:
///   - Default choice for most cleanup tasks.
///   - Examples: close optional services, clear caches, free in-memory objects.
/// - `PRIORITY_LOW`:
///   - Best-effort cleanup that can run last.
///   - Examples: telemetry tail flush, optional metrics/reporting, debug-only teardown.
///
/// Selection rules:
/// 1. Start with `PRIORITY_NORMAL`.
/// 2. Upgrade to `PRIORITY_HIGH` when a task blocks or interferes with other cleanup.
/// 3. Use `PRIORITY_CRITICAL` only for truly order-sensitive tasks.
/// 4. Use `PRIORITY_LOW` for non-essential tasks that should never block shutdown.
///
/// Example:
/// ```ignore
/// use crate::constants::shutdown_priority::{PRIORITY_HIGH, PRIORITY_LOW};
///
/// shutdown::register_pre_shutdown_task_with_priority("stop_worker_pool", PRIORITY_HIGH, || {
///     worker_pool::stop_all();
/// });
///
/// shutdown::register_post_shutdown_task_with_priority("flush_metrics", PRIORITY_LOW, || {
///     metrics::flush();
/// });
/// ```
pub const PRIORITY_CRITICAL: i32 = 1000;
pub const PRIORITY_HIGH: i32 = 100;
pub const PRIORITY_NORMAL: i32 = 0;
pub const PRIORITY_LOW: i32 = -100;
