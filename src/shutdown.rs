//! Unified graceful-shutdown pipeline.
//!
//! Team conventions (important):
//! - Stages run in fixed order:
//!   1) `pre_shutdown`  -> stop producers / timers / inflight work
//!   2) `background_shutdown` -> join writers and delivery workers off UI
//!   3) `ui_shutdown`   -> detach UI and OS integrations
//!   4) `post_shutdown` -> best-effort tail cleanup
//! - Inside the same stage, task execution order is:
//!   - higher `priority` first
//!   - if equal priority, later registration first (LIFO tie-break)
//! - Prefer priority constants from `crate::constants::shutdown_priority`:
//!   - `PRIORITY_CRITICAL` for strict order-sensitive and safety-critical teardown
//!   - `PRIORITY_HIGH` for high-impact resources that should stop early
//!   - `PRIORITY_NORMAL` as default choice
//!   - `PRIORITY_LOW` for best-effort, non-blocking tail tasks
//!
//! Practical registration guideline:
//! - Start with `*_task(...)` (default normal priority).
//! - Use `*_task_with_priority(...)` only when ordering requirements are clear.
//! - Keep callbacks short and panic-safe; this module already guards panics and logs each step.
//!
//! Shutdown Checklist (for code review):
//! 1) Register cleanup in the correct stage (`pre`/`ui`/`post`) with a documented reason.
//! 2) Choose priority intentionally (default `PRIORITY_NORMAL`; avoid unnecessary escalation).
//! 3) Ensure cleanup is idempotent, non-blocking when possible, and emits useful logs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::{future::Future, pin::Pin};

use crate::constants::shutdown_priority::PRIORITY_NORMAL;

static MAIN_WINDOW: Mutex<Option<slint::Weak<crate::MainWindow>>> = Mutex::new(None);

pub(crate) fn register_main_window(window: &crate::MainWindow) {
    use slint::ComponentHandle;
    *MAIN_WINDOW.lock().unwrap_or_else(|p| p.into_inner()) = Some(window.as_weak());
}

/// Original-thread input gate, before async drains. Keep the cleanup registry
/// open until those drains finish so late lazy-worker registrations are kept.
fn close_ui_admission() {
    use slint::ComponentHandle;
    let window = MAIN_WINDOW
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(window) = window.and_then(|window| window.upgrade()) {
        let _ = window.hide();
    }
}

type CleanupFn = Box<dyn Fn() + Send + Sync + 'static>;
type AsyncCleanupFn = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;
static ASYNC_DRAINS: Mutex<Vec<(&'static str, AsyncCleanupFn)>> = Mutex::new(Vec::new());
static ASYNC_DRAIN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Drain process-owning producers before synchronous persistence/UI teardown.
/// Callbacks run on the shared runtime; the event loop remains responsive.
pub(crate) fn register_async_drain(
    name: &'static str,
    callback: impl FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + 'static,
) {
    ASYNC_DRAINS
        .lock()
        .unwrap()
        .push((name, Box::new(callback)));
}

pub(crate) async fn drain_async_tasks() {
    let _drain = ASYNC_DRAIN_LOCK.lock().await;
    let tasks = std::mem::take(&mut *ASYNC_DRAINS.lock().unwrap());
    for (name, callback) in tasks.into_iter().rev() {
        log::info!("[Shutdown] Draining {name}");
        callback().await;
    }
}

mod pipeline;
use pipeline::{CleanupRegistry, CleanupRun, CleanupTask, ShutdownStage};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum CleanupMode {
    EventLoop,
    BackgroundFallback,
}

impl CleanupMode {
    fn runs_ui_cleanup(self) -> bool {
        self == Self::EventLoop
    }
}

static CLEANUP_REGISTRY: OnceLock<Mutex<CleanupRegistry>> = OnceLock::new();
static CLEANUP_RUN: OnceLock<std::sync::Arc<CleanupRun>> = OnceLock::new();
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static SIGNAL_HANDLERS_INITIALIZED: AtomicBool = AtomicBool::new(false);
static TASK_ORDER_COUNTER: OnceLock<std::sync::atomic::AtomicU64> = OnceLock::new();

fn next_task_order() -> u64 {
    TASK_ORDER_COUNTER
        .get_or_init(|| std::sync::atomic::AtomicU64::new(1))
        .fetch_add(1, Ordering::Relaxed)
}

fn cleanup_registry() -> &'static Mutex<CleanupRegistry> {
    CLEANUP_REGISTRY.get_or_init(|| Mutex::new(CleanupRegistry::default()))
}

fn register_task(
    stage: ShutdownStage,
    name: &'static str,
    priority: i32,
    callback: impl Fn() + Send + Sync + 'static,
) {
    let mut registry = cleanup_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let stage_tasks = match stage {
        ShutdownStage::Pre => &mut registry.pre_shutdown,
        ShutdownStage::Background => &mut registry.background_shutdown,
        ShutdownStage::Ui => &mut registry.ui_shutdown,
        ShutdownStage::Post => &mut registry.post_shutdown,
    };

    stage_tasks.push(CleanupTask {
        name,
        priority,
        order: next_task_order(),
        callback: Box::new(callback),
    });

    log::info!(
        "[Shutdown] Registered task '{}' in stage '{}' with priority {}",
        name,
        stage.as_str(),
        priority
    );
}

/// Register a task executed before UI shutdown work starts.
#[allow(dead_code)]
pub fn register_pre_shutdown_task(name: &'static str, callback: impl Fn() + Send + Sync + 'static) {
    register_task(ShutdownStage::Pre, name, PRIORITY_NORMAL, callback);
}

/// Register a pre-shutdown task with custom priority.
pub fn register_pre_shutdown_task_with_priority(
    name: &'static str,
    priority: i32,
    callback: impl Fn() + Send + Sync + 'static,
) {
    register_task(ShutdownStage::Pre, name, priority, callback);
}

/// Register a blocking final barrier after producers stop and before UI cleanup.
pub(crate) fn register_background_shutdown_task_with_priority(
    name: &'static str,
    priority: i32,
    callback: impl Fn() + Send + Sync + 'static,
) {
    register_task(ShutdownStage::Background, name, priority, callback);
}

/// Register a task executed during UI shutdown.
#[allow(dead_code)]
pub fn register_ui_shutdown_task(name: &'static str, callback: impl Fn() + Send + Sync + 'static) {
    register_task(ShutdownStage::Ui, name, PRIORITY_NORMAL, callback);
}

/// Register a ui-shutdown task with custom priority.
pub fn register_ui_shutdown_task_with_priority(
    name: &'static str,
    priority: i32,
    callback: impl Fn() + Send + Sync + 'static,
) {
    register_task(ShutdownStage::Ui, name, priority, callback);
}

/// Register a task executed after UI shutdown work.
#[allow(dead_code)]
pub fn register_post_shutdown_task(
    name: &'static str,
    callback: impl Fn() + Send + Sync + 'static,
) {
    register_task(ShutdownStage::Post, name, PRIORITY_NORMAL, callback);
}

/// Register a post-shutdown task with custom priority.
#[allow(dead_code)]
pub fn register_post_shutdown_task_with_priority(
    name: &'static str,
    priority: i32,
    callback: impl Fn() + Send + Sync + 'static,
) {
    register_task(ShutdownStage::Post, name, priority, callback);
}

/// Install signal handlers for graceful shutdown.
/// Handles SIGTERM/SIGINT/SIGHUP/SIGQUIT (where available).
pub fn init_signal_handlers() {
    if SIGNAL_HANDLERS_INITIALIZED.swap(true, Ordering::SeqCst) {
        log::info!("[Shutdown] Signal handlers already initialized, skipping");
        return;
    }

    log::info!("[Shutdown] Initializing signal handlers");
    #[cfg(unix)]
    std::thread::spawn(|| {
        use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
        use signal_hook::iterator::Signals;

        let mut signals = match Signals::new([SIGTERM, SIGINT, SIGHUP, SIGQUIT]) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[Shutdown] Failed to initialize signal handlers: {}", e);
                return;
            }
        };

        log::info!("[Shutdown] Signal handlers are active");
        if let Some(signal) = signals.forever().next() {
            log::info!("[Shutdown] Received OS signal: {}", signal);
            request_shutdown(&format!("signal({})", signal));
        }
    });

    #[cfg(not(unix))]
    log::info!("[Shutdown] OS signal handlers are not enabled on this platform");
}

fn cleanup_run(trigger: &str) -> std::sync::Arc<CleanupRun> {
    CLEANUP_RUN
        .get_or_init(|| {
            log::info!("[Shutdown] Cleanup pipeline started (trigger: {trigger})");
            let registry =
                std::mem::take(&mut *cleanup_registry().lock().unwrap_or_else(|p| p.into_inner()));
            CleanupRun::new(registry)
        })
        .clone()
}

/// Natural event-loop return still owns cleanup on the original thread. There
/// is no live event loop to block, but final writer joins run on the worker.
pub fn run_cleanup_once(trigger: &str) {
    if CLEANUP_RUN.get().is_some_and(|run| run.is_current_owner()) {
        log::warn!("[Shutdown] Ignored reentrant cleanup from its own callback");
        return;
    }
    close_ui_admission();
    crate::app_runtime::shared_runtime().block_on(drain_async_tasks());
    let run = cleanup_run(trigger);
    run.stop_producers();
    crate::app_runtime::shared_runtime().block_on(run.drain());
    finish_cleanup(&run, CleanupMode::EventLoop);
}

fn finish_cleanup(run: &CleanupRun, mode: CleanupMode) {
    run.finish_with(mode, || {
        #[cfg(target_os = "macos")]
        if mode.runs_ui_cleanup() {
            cleanup_associated_helpers();
        }
        crate::logger::debug_logger::flush_application_log();
    });
}

/// Request graceful shutdown through a single shared entry point.
pub fn request_shutdown(trigger: &str) {
    if SHUTDOWN_REQUESTED.swap(true, Ordering::SeqCst) {
        log::info!(
            "[Shutdown] Shutdown already requested, ignoring duplicate trigger: {}",
            trigger
        );
        return;
    }

    log::info!(
        "[Shutdown] Shutdown requested (trigger: {}), dispatching to UI loop",
        trigger
    );
    let trigger_owned = trigger.to_string();
    let fallback = trigger_owned.clone();
    if slint::invoke_from_event_loop(move || start_shutdown(&trigger_owned)).is_err() {
        crate::app_runtime::shared_runtime().spawn(async move {
            shutdown_without_ui(fallback).await;
        });
    }
}

fn start_shutdown(trigger: &str) {
    close_ui_admission();
    let trigger = trigger.to_owned();
    crate::app_runtime::shared_runtime().spawn(async move {
        drain_async_tasks().await;
        let fallback = trigger.clone();
        if slint::invoke_from_event_loop(move || perform_shutdown(&trigger)).is_err() {
            shutdown_without_ui(fallback).await;
        }
    });
}

async fn shutdown_without_ui(trigger: String) {
    log::error!("[Shutdown] UI dispatch unavailable; completing owned background barriers");
    drain_async_tasks().await;
    let run = cleanup_run(&trigger);
    run.stop_producers();
    run.drain().await;
    finish_cleanup(&run, CleanupMode::BackgroundFallback);
    std::process::exit(0);
}

fn perform_shutdown(trigger: &str) {
    let run = cleanup_run(trigger);
    run.stop_producers();
    crate::app_runtime::shared_runtime().spawn(async move {
        run.drain().await;
        let fallback = run.clone();
        if slint::invoke_from_event_loop(move || {
            finish_cleanup(&run, CleanupMode::EventLoop);
            terminate_application();
        })
        .is_err()
        {
            log::error!("[Shutdown] Final UI cleanup dispatch failed");
            finish_cleanup(&fallback, CleanupMode::BackgroundFallback);
            std::process::exit(0);
        }
    });
}

fn terminate_application() {
    log::info!("[Shutdown] Requesting event loop exit for clean teardown");
    if slint::quit_event_loop().is_err() {
        log::warn!("[Shutdown] quit_event_loop failed, force exiting");
        std::process::exit(0);
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(2));
        log::warn!("[Shutdown] Event loop did not stop in time, force exiting");
        std::process::exit(0);
    });
}

/// Terminate any XPC helper processes that macOS spawned on behalf of this app.
///
/// macOS automatically launches helper services (e.g. AutoFill via
/// `SafariPlatformSupport`) when a Cocoa window contains text inputs.
/// These helpers are parented to launchd (PID 1), so they survive a normal
/// `process::exit`. Activity Monitor shows them as `<ServiceType> (<AppName>)`,
/// but `ps` lists them under their real binary path — making them invisible to
/// naïve string matching.
///
/// We use `NSWorkspace.runningApplications` which exposes the same
/// `localizedName` as Activity Monitor, then dynamically derive our own
/// process name from `NSProcessInfo` (no hard-coded strings).
///
/// Important: keep this inside the normal shutdown pipeline. Calling AppKit from
/// `atexit` is unsafe because Cocoa may already be partially torn down, which can
/// turn a normal Quit into a macOS "unexpectedly quit" crash report.
#[cfg(target_os = "macos")]
fn cleanup_associated_helpers() {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use std::ffi::CStr;

    unsafe {
        let process_info_class = match Class::get("NSProcessInfo") {
            Some(cls) => cls,
            None => return,
        };
        let process_info: *mut Object = msg_send![process_info_class, processInfo];
        if process_info.is_null() {
            return;
        }
        let our_name_ns: *mut Object = msg_send![process_info, processName];
        if our_name_ns.is_null() {
            return;
        }
        let our_name_cstr: *const std::os::raw::c_char = msg_send![our_name_ns, UTF8String];
        if our_name_cstr.is_null() {
            return;
        }
        let our_name = CStr::from_ptr(our_name_cstr)
            .to_string_lossy()
            .to_lowercase();
        let our_pid = std::process::id() as i32;

        let workspace_class = match Class::get("NSWorkspace") {
            Some(cls) => cls,
            None => return,
        };
        let workspace: *mut Object = msg_send![workspace_class, sharedWorkspace];
        if workspace.is_null() {
            return;
        }
        let apps: *mut Object = msg_send![workspace, runningApplications];
        if apps.is_null() {
            return;
        }

        let count: usize = msg_send![apps, count];
        let mut terminated = 0u32;

        for i in 0..count {
            let app: *mut Object = msg_send![apps, objectAtIndex: i];
            if app.is_null() {
                continue;
            }

            let pid: i32 = msg_send![app, processIdentifier];
            if pid == our_pid {
                continue;
            }

            let name: *mut Object = msg_send![app, localizedName];
            if name.is_null() {
                continue;
            }
            let name_cstr: *const std::os::raw::c_char = msg_send![name, UTF8String];
            if name_cstr.is_null() {
                continue;
            }
            let name_str = CStr::from_ptr(name_cstr).to_string_lossy();

            if name_str.to_lowercase().contains(&our_name) {
                let ok: bool = msg_send![app, forceTerminate];
                log::info!(
                    "[Shutdown] Terminated helper '{}' (PID {}, ok: {})",
                    name_str,
                    pid,
                    ok
                );
                terminated += 1;
            }
        }

        if terminated > 0 {
            log::info!("[Shutdown] Cleaned up {} helper process(es)", terminated);
        } else {
            log::info!("[Shutdown] No associated helper processes found");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CleanupMode;

    #[test]
    fn background_fallback_never_runs_ui_cleanup() {
        assert!(CleanupMode::EventLoop.runs_ui_cleanup());
        assert!(!CleanupMode::BackgroundFallback.runs_ui_cleanup());
    }
}
