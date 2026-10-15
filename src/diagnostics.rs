use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use slint::{ComponentHandle, Global};

use crate::settings_feedback::settings_toast;
use crate::{
    CustomComponentType, MainWindow, NotificationDelivery, NotificationManager,
    NotificationOptions, ToastManager, app_paths, app_runtime, platform,
};

const LOG_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);
mod archive;

static EXPORT_ACTIVE: AtomicBool = AtomicBool::new(false);

struct ExportPermit;
impl Drop for ExportPermit {
    fn drop(&mut self) {
        EXPORT_ACTIVE.store(false, Ordering::Release);
    }
}

#[derive(Default)]
pub(crate) struct RunGuard {
    previous_run_interrupted: bool,
}

pub(crate) fn begin_run() -> Result<RunGuard> {
    let Some(data_dir) = app_paths::data_local_dir() else {
        return Ok(RunGuard {
            previous_run_interrupted: false,
        });
    };
    let sentinel = data_dir.join("run/crash-sentinel.json");
    let previous_run_interrupted = sentinel.is_file();
    let record = serde_json::json!({
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "started_at": chrono::Utc::now().to_rfc3339(),
    });
    usher_fs::atomic_write_private(&sentinel, serde_json::to_vec_pretty(&record)?.as_slice())
        .with_context(|| format!("Failed to write crash sentinel: {}", sentinel.display()))?;
    crate::shutdown::register_post_shutdown_task("clear_crash_sentinel", move || {
        if let Err(error) = fs::remove_file(&sentinel)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!("[Diagnostics] Failed to clear crash sentinel: {error}");
        }
    });
    Ok(RunGuard {
        previous_run_interrupted,
    })
}

pub(crate) fn setup(window: &MainWindow, run_guard: RunGuard) {
    if run_guard.previous_run_interrupted {
        NotificationManager::get(window).invoke_warn(NotificationOptions {
            title: crate::i18n::tr("diagnostics.previous_run_title")
                .as_ref()
                .into(),
            message: crate::i18n::tr("diagnostics.previous_run_message")
                .as_ref()
                .into(),
            duration: 8_000,
            icon: "".into(),
            component: CustomComponentType::None,
            actions: Default::default(),
            tag: "previous-run-interrupted".into(),
            source: "Diagnostics".into(),
            category: "diagnostics".into(),
            native_delivery: NotificationDelivery::WhenUnfocused,
        });
    }

    let weak = window.as_weak();
    window.on_open_log_directory(move || {
        let result = log_root().ok_or_else(|| "Log directory is unavailable.".to_string());
        let result = result.and_then(|path| {
            fs::create_dir_all(&path)
                .map_err(|error| format!("Failed to create log directory: {error}"))?;
            platform::open_directory_in_file_manager(&path)
        });
        if let Err(error) = result {
            log::warn!("[Diagnostics] Failed to open log directory: {error}");
            if let Some(window) = weak.upgrade() {
                ToastManager::get(&window).invoke_error(settings_toast(
                    crate::i18n::tr("diagnostics.open_logs_failed").as_ref(),
                ));
            }
        }
    });

    let weak = window.as_weak();
    window.on_export_diagnostics(move || {
        if EXPORT_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let permit = ExportPermit;
        let weak = weak.clone();
        app_runtime::shared_runtime().spawn(async move {
            let result = tokio::task::spawn_blocking(export_bundle)
                .await
                .map_err(anyhow::Error::from)
                .and_then(|result| result);
            let _ = slint::invoke_from_event_loop(move || {
                let _permit = permit;
                let Some(window) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(path) => {
                        ToastManager::get(&window).invoke_success(settings_toast(
                            crate::i18n::tr("diagnostics.exported").as_ref(),
                        ));
                        if let Some(parent) = path.parent()
                            && let Err(error) = platform::open_directory_in_file_manager(parent)
                        {
                            log::warn!("[Diagnostics] Failed to reveal export: {error}");
                        }
                    }
                    Err(error) => {
                        log::error!("[Diagnostics] Export failed: {error:#}");
                        ToastManager::get(&window).invoke_error(settings_toast(
                            crate::i18n::tr("diagnostics.export_failed").as_ref(),
                        ));
                    }
                }
            });
        });
    });

    app_runtime::shared_runtime().spawn(async {
        let result = tokio::task::spawn_blocking(|| {
            if let Some(root) = log_root() {
                prune_old_logs(&root)?;
            }
            if let Some(root) = diagnostics_root() {
                archive::prune(&root)?;
            }
            Ok::<_, io::Error>(())
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("[Diagnostics] Retention cleanup failed: {error}"),
            Err(error) => log::warn!("[Diagnostics] Retention worker failed: {error}"),
        }
    });
}

fn log_root() -> Option<PathBuf> {
    app_paths::data_local_dir().map(|root| root.join("logs"))
}

fn diagnostics_root() -> Option<PathBuf> {
    app_paths::data_local_dir().map(|root| root.join("diagnostics"))
}

fn export_bundle() -> Result<PathBuf> {
    let output_root = diagnostics_root().context("Diagnostic directory is unavailable")?;
    archive::export(&output_root, log_root().as_deref())
}

fn prune_old_logs(root: &Path) -> io::Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    let cutoff = SystemTime::now()
        .checked_sub(LOG_RETENTION)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() && metadata.modified().is_ok_and(|modified| modified < cutoff) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}
