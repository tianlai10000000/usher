use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use slint::{ComponentHandle, Global};

use crate::{
    CustomComponentType, MainWindow, NotificationDelivery, NotificationManager,
    NotificationOptions, ToastManager, app_paths, app_runtime, platform, settings_toast,
};

const LOG_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const DIAGNOSTIC_ARCHIVE_LIMIT: usize = 5;
const DIAGNOSTIC_FILE_LIMIT: u64 = 8 * 1024 * 1024;
const DIAGNOSTIC_TOTAL_LIMIT: u64 = 64 * 1024 * 1024;

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
        let weak = weak.clone();
        app_runtime::shared_runtime().spawn(async move {
            let result = tokio::task::spawn_blocking(export_bundle)
                .await
                .map_err(anyhow::Error::from)
                .and_then(|result| result);
            let _ = slint::invoke_from_event_loop(move || {
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
                prune_diagnostic_archives(&root)?;
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
    fs::create_dir_all(&output_root)?;
    let output = output_root.join(format!(
        "usher-diagnostics-{}-{}.tar.gz",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"),
        std::process::id()
    ));
    let file = create_private_archive(&output)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "application": "Usher",
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "contents": "Bounded application and tracing logs only; settings and user data are excluded."
    }))?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    archive.append_data(&mut header, "manifest.json", manifest.as_slice())?;

    let mut included = 0u64;
    if let Some(root) = log_root() {
        for path in collect_log_files(&root)? {
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file()
                || metadata.len() > DIAGNOSTIC_FILE_LIMIT
                || included.saturating_add(metadata.len()) > DIAGNOSTIC_TOTAL_LIMIT
            {
                continue;
            }
            let relative = path.strip_prefix(&root)?;
            archive.append_path_with_name(&path, Path::new("logs").join(relative))?;
            included = included.saturating_add(metadata.len());
        }
    }
    archive.finish()?;
    Ok(output)
}

fn create_private_archive(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    configure_private_mode(&mut options);
    options.open(path)
}

#[cfg(unix)]
fn configure_private_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    options.mode(0o600);
}

#[cfg(not(unix))]
fn configure_private_mode(_options: &mut OpenOptions) {}

fn collect_log_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "log")
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
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

fn prune_diagnostic_archives(root: &Path) -> io::Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    let mut archives = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("usher-diagnostics-") && name.ends_with(".tar.gz")
                })
        })
        .collect::<Vec<_>>();
    archives.sort_by_key(|entry| entry.file_name());
    let remove_count = archives.len().saturating_sub(DIAGNOSTIC_ARCHIVE_LIMIT);
    for entry in archives.into_iter().take(remove_count) {
        fs::remove_file(entry.path())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{collect_log_files, prune_diagnostic_archives};

    #[test]
    fn diagnostic_collection_ignores_symlinks_and_orders_regular_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("2026-08-30")).unwrap();
        std::fs::write(root.path().join("2026-08-30/usher.log"), "log").unwrap();
        std::fs::write(root.path().join("2026-08-30/usher_tracing.log"), "trace").unwrap();
        std::fs::write(root.path().join("2026-08-30/settings.toml"), "secret").unwrap();

        let files = collect_log_files(root.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files[0] < files[1]);
        assert!(files.iter().all(|path| path.extension().unwrap() == "log"));
    }

    #[test]
    fn diagnostic_archive_retention_keeps_the_newest_five() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..7 {
            std::fs::write(
                root.path()
                    .join(format!("usher-diagnostics-{index}.tar.gz")),
                "x",
            )
            .unwrap();
        }
        std::fs::write(root.path().join("keep-me.txt"), "x").unwrap();
        prune_diagnostic_archives(root.path()).unwrap();
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 6);
        assert!(root.path().join("keep-me.txt").exists());
        assert!(!root.path().join("usher-diagnostics-0.tar.gz").exists());
        assert!(root.path().join("usher-diagnostics-6.tar.gz").exists());
    }

    #[cfg(unix)]
    #[test]
    fn diagnostic_archives_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("usher-diagnostics-test.tar.gz");
        super::create_private_archive(&path).unwrap();

        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
