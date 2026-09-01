use anyhow::{Context, Result};
use self_update::cargo_crate_version;
use semver::Version;
use slint::{ComponentHandle, Global, ModelRc, SharedString, Timer, VecModel};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;

use crate::http::{self, RequestOptions};
use crate::settings_page::{SettingsPageBridge, mirror_update_checking};
use crate::{
    CustomComponentType, MainWindow, NotificationDelivery, NotificationManager,
    NotificationOptions, UpdateLogic, app_runtime::shared_runtime,
};

#[derive(Default, Clone)]
pub struct UpdateState {
    pub available: bool,
    pub version: String,
    pub progress: f32,
    pub error: String,
    // Download status
    pub is_downloading: bool,
    pub download_progress: f32,
    pub download_status: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Progress callback type
pub type ProgressCallback = Arc<dyn Fn(f32, u64, u64, &str) + Send + Sync>;

/// Cancel flag
pub type CancelFlag = Arc<AtomicBool>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateChannel {
    Stable,
    Alpha,
}

impl UpdateChannel {
    fn configured() -> Self {
        if crate::settings::get_settings_manager().get().update_channel == "alpha" {
            Self::Alpha
        } else {
            Self::Stable
        }
    }

    fn accepts(self, version: &Version) -> bool {
        self == Self::Alpha || version.pre.is_empty()
    }
}

pub(super) fn update_check_error_message_key(error: &str) -> &'static str {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("404")
        || normalized.contains("not found")
        || normalized.contains("no available release")
    {
        "update.release_not_found"
    } else if normalized.contains("network")
        || normalized.contains("reqwest")
        || normalized.contains("connect")
        || normalized.contains("timed out")
        || normalized.contains("error sending request")
    {
        "update.network_failed"
    } else {
        "update.check_failed"
    }
}

fn create_updater() -> Result<Box<dyn self_update::update::ReleaseUpdate>> {
    log::debug!(
        "Creating updater, repo: tianlai10000000/usher, current version: {}",
        cargo_crate_version!()
    );
    self_update::backends::github::Update::configure()
        .repo_owner("tianlai10000000")
        .repo_name("usher")
        .bin_name("usher") // Windows can be changed to "myapp.exe"
        .current_version(cargo_crate_version!())
        .build()
        .context("Failed to configure updater")
}

fn select_release(
    releases: Vec<self_update::update::Release>,
    channel: UpdateChannel,
) -> Option<self_update::update::Release> {
    releases
        .into_iter()
        .filter_map(|release| {
            Version::parse(&release.version)
                .ok()
                .map(|version| (version, release))
        })
        .filter(|(version, _)| channel.accepts(version))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, release)| release)
}

fn selected_release(
    updater: &dyn self_update::update::ReleaseUpdate,
    current_version: &str,
) -> Result<Option<self_update::update::Release>> {
    let releases = updater
        .get_latest_releases(current_version)
        .context("Failed to get available releases")?;
    Ok(select_release(releases, UpdateChannel::configured()))
}

pub async fn check_for_update(state: &Arc<Mutex<UpdateState>>) -> Result<bool> {
    log::debug!(" check_for_update started");

    let state_clone = state.clone();
    // Execute synchronous operations in blocking thread pool
    let result = tokio::task::spawn_blocking(move || -> Result<(bool, String), anyhow::Error> {
        let updater = create_updater().map_err(|e| {
            log::debug!(" Failed to create updater: {}", e);
            e
        })?;
        log::debug!(" Updater created successfully");

        // Get latest version information
        log::debug!(" Getting latest version information...");
        let Some(latest_release) = selected_release(updater.as_ref(), cargo_crate_version!()).map_err(|e| {
            log::debug!(" Failed to get latest version: {}", e);
            let error_msg = e.to_string();
            // Provide more user-friendly error messages
            if error_msg.contains("404") || error_msg.contains("Not Found") {
                anyhow::anyhow!(
                    "Repository not found or no available release versions. Please check if the repository exists and has created releases."
                )
            } else if error_msg.contains("NetworkError") {
                anyhow::anyhow!(
                    "Network connection failed, please check your network connection and retry. Failure reason: {}",
                    error_msg
                )
            } else {
                anyhow::anyhow!("Failed to check for updates: {}", error_msg)
            }
        })? else {
            return Ok((true, String::new()));
        };
        let current_version_str = cargo_crate_version!();
        log::debug!(
            "Current version: {}, Latest version: {}",
            current_version_str, latest_release.version
        );

        // Use semver for semantic version comparison
        let current_ver = Version::parse(current_version_str).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse current version '{}': {}",
                current_version_str,
                e
            )
        })?;

        let latest_ver = Version::parse(&latest_release.version).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse remote version '{}': {}",
                latest_release.version,
                e
            )
        })?;

        // If current version >= latest version, consider it up to date
        if current_ver >= latest_ver {
            log::debug!(
                "Already up to date (current: {} >= remote: {})",
                current_ver, latest_ver
            );
            return Ok((true, String::new()));
        }

        log::info!(
            "New version found: {} (current: {})",
            latest_ver, current_ver
        );
        Ok((false, latest_release.version))
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Update check error: {}", e))?;

    let (is_up_to_date, version) = result;

    if !is_up_to_date {
        let mut s = state_clone
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        s.available = true;
        s.version = version;
    }

    Ok(is_up_to_date)
}

/// Update download with progress callback (using real download progress)
///
/// # Arguments
/// * `state` - Update state
/// * `progress_callback` - Progress callback function (progress: f32, downloaded: u64, total: u64, status: &str)
/// * `cancel_flag` - Cancel flag
pub async fn perform_update_with_progress(
    state: &Arc<Mutex<UpdateState>>,
    progress_callback: ProgressCallback,
    cancel_flag: CancelFlag,
) -> Result<()> {
    log::debug!(" perform_update_with_progress started");

    // Set download status
    {
        let mut s = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        s.is_downloading = true;
        s.download_progress = 0.0;
        s.download_status = crate::i18n::tr("update.preparing_download").into_owned();
    }

    progress_callback(
        0.0,
        0,
        0,
        crate::i18n::tr("update.preparing_download").as_ref(),
    );

    // Check if cancelled
    if cancel_flag.load(Ordering::Relaxed) {
        log::debug!(" Download cancelled (preparation phase)");
        return Err(anyhow::anyhow!("Download cancelled"));
    }

    let state_clone = state.clone();
    let progress_callback_clone = progress_callback.clone();

    // Get release information (in blocking thread)
    let selected_version = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .version
        .clone();
    let release_info =
        tokio::task::spawn_blocking(move || -> Result<(String, String), anyhow::Error> {
            progress_callback_clone(
                0.05,
                0,
                0,
                crate::i18n::tr("update.connecting").as_ref(),
            );

            let updater = create_updater()?;

            progress_callback_clone(
                0.1,
                0,
                0,
                crate::i18n::tr("update.fetching_release").as_ref(),
            );

            let releases = updater.get_latest_releases(cargo_crate_version!())?;
            let latest_release = if selected_version.is_empty() {
                select_release(releases, UpdateChannel::configured())
            } else {
                releases
                    .into_iter()
                    .find(|release| release.version == selected_version)
            }
            .context("The selected update release is no longer available")?;
            log::debug!(" Got latest version: {}", latest_release.version);

            // Get download URL for current platform
            let target = self_update::get_target();
            log::debug!(" Current platform: {}", target);
            log::debug!(
                "Available assets list: {:?}",
                latest_release
                    .assets
                    .iter()
                    .map(|a| &a.name)
                    .collect::<Vec<_>>()
            );

            // Find matching asset
            // self_update requires tar.gz or zip format, prioritize these formats
            // Support multiple naming formats:
            // 1. Full target triple (e.g., aarch64-apple-darwin.tar.gz)
            // 2. Simplified format (e.g., arm64, x64)
            // 3. apple-darwin generic match
            // First try to find tar.gz format (self_update preferred)
            let asset = latest_release
                .assets
                .iter()
                .filter(|a| {
                    let name_lower = a.name.to_lowercase();
                    name_lower.ends_with(".tar.gz") || name_lower.ends_with(".zip")
                })
                .find(|a| {
                    let name_lower = a.name.to_lowercase();
                    let target_lower = target.to_lowercase();

                    // Exact match for full target
                    name_lower.contains(&target_lower) ||
                    // Match apple-darwin
                    name_lower.contains("apple-darwin") ||
                    // Match simplified format: aarch64 -> arm64, x86_64 -> x64
                    (target_lower.contains("aarch64") && name_lower.contains("arm64")) ||
                    (target_lower.contains("x86_64") && (name_lower.contains("x64") || name_lower.contains("x86_64")))
                })
                .or_else(|| {
                    // If no exact match for tar.gz/zip found, try to find any archive containing platform-related keywords
                    latest_release
                        .assets
                        .iter()
                        .filter(|a| {
                            let name_lower = a.name.to_lowercase();
                            name_lower.ends_with(".tar.gz") || name_lower.ends_with(".zip")
                        })
                        .find(|a| {
                            let name_lower = a.name.to_lowercase();
                            name_lower.contains("darwin")
                                || name_lower.contains("macos")
                                || name_lower.contains("osx")
                        })
                })
                .or_else(|| {
                    // If still not found, try to find any tar.gz or zip file
                    latest_release.assets.iter().find(|a| {
                        let name_lower = a.name.to_lowercase();
                        name_lower.ends_with(".tar.gz") || name_lower.ends_with(".zip")
                    })
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No suitable asset found for platform: {}. Available assets: {:?}",
                        target,
                        latest_release
                            .assets
                            .iter()
                            .map(|a| &a.name)
                            .collect::<Vec<_>>()
                    )
                })?;

            log::debug!(" Download URL: {}", asset.download_url);
            Ok((asset.download_url.clone(), latest_release.version))
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {}", e))??;

    let (download_url, version) = release_info;

    // Check if cancelled
    if cancel_flag.load(Ordering::Relaxed) {
        log::debug!(" Download cancelled (after getting version info)");
        return Err(anyhow::anyhow!("Download cancelled"));
    }

    let downloading_version = crate::i18n::tr_args(
        "update.downloading_release",
        &[("version", version.as_str())],
    );
    progress_callback(0.15, 0, 0, &downloading_version);

    // Use the shared HTTP pipeline while retaining reqwest streaming.
    let mut response = http::send(
        http::get(&download_url)
            .header("User-Agent", format!("usher/{}", cargo_crate_version!()))
            .header("Accept", "application/octet-stream"),
        // The updater already owns progress and notification UX.
        RequestOptions::new("Download update")
            .timeout(Duration::from_secs(10 * 60))
            .no_retry(),
    )
    .await
    .context("Failed to start download")?;

    let total_size = response.content_length().unwrap_or(0);
    log::debug!(" File size: {} bytes", total_size);

    // Create temporary file to save downloaded content
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("usher_update_{}.tar.gz", version));
    let mut temp_file = tokio::fs::File::create(&temp_file_path)
        .await
        .context("Failed to create temp file")?;

    // Stream download and report progress
    let mut downloaded: u64 = 0;
    use tokio::io::AsyncWriteExt;

    while let Some(chunk) = response
        .chunk()
        .await
        .context("Failed to read download chunk")?
    {
        // Check if cancelled
        if cancel_flag.load(Ordering::Relaxed) {
            log::debug!(" Download cancelled");
            // Clean up temporary file
            let _ = tokio::fs::remove_file(&temp_file_path).await;
            return Err(anyhow::anyhow!("Download cancelled"));
        }

        temp_file
            .write_all(&chunk)
            .await
            .context("Failed to write chunk")?;

        downloaded += chunk.len() as u64;

        // Calculate progress (15% - 85% for download)
        let progress = if total_size > 0 {
            0.15 + (downloaded as f32 / total_size as f32) * 0.70
        } else {
            0.5 // If total size is unknown, show 50%
        };

        // Format downloaded size
        let downloaded_mb = format!("{:.1}", downloaded as f64 / 1_048_576.0);
        let total_mb = format!("{:.1}", total_size as f64 / 1_048_576.0);
        let status = if total_size > 0 {
            crate::i18n::tr_args(
                "update.downloading_progress",
                &[
                    ("downloaded_mb", downloaded_mb.as_str()),
                    ("total_mb", total_mb.as_str()),
                ],
            )
        } else {
            crate::i18n::tr_args(
                "update.downloading_amount",
                &[("downloaded_mb", downloaded_mb.as_str())],
            )
        };

        progress_callback(progress, downloaded, total_size, &status);

        // Update state
        {
            let mut s = state_clone
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            s.download_progress = progress;
            s.downloaded_bytes = downloaded;
            s.total_bytes = total_size;
            s.download_status = status;
        }
    }

    temp_file.flush().await?;
    drop(temp_file);

    log::debug!(" Download complete, starting installation...");
    progress_callback(
        0.85,
        downloaded,
        total_size,
        crate::i18n::tr("update.download_installing").as_ref(),
    );

    // Check if cancelled
    if cancel_flag.load(Ordering::Relaxed) {
        let _ = tokio::fs::remove_file(&temp_file_path).await;
        return Err(anyhow::anyhow!("Download cancelled"));
    }

    // Execute installation in blocking thread
    let temp_file_path_clone = temp_file_path.clone();
    let progress_callback_for_install = progress_callback.clone();
    let state_for_install = state_clone.clone();

    let install_result = tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
        progress_callback_for_install(
            0.90,
            downloaded,
            total_size,
            crate::i18n::tr("update.extracting").as_ref(),
        );

        // Record current executable path (for debugging)
        let current_exe =
            std::env::current_exe().context("Failed to get current executable path")?;
        log::debug!(" Current executable path: {:?}", current_exe);

        let exe_str = current_exe.to_string_lossy();
        if exe_str.contains(".app/") {
            log::debug!(" Detected .app bundle, will replace executable file in bundle");
        } else {
            log::debug!(" No .app bundle detected, will directly replace executable file");
        }

        // Use downloaded file for installation, instead of re-downloading
        log::debug!(
            "Installing update from local file: {:?}",
            temp_file_path_clone
        );

        let extract_dir = tempfile::Builder::new()
            .prefix("usher_update_extract_")
            .tempdir()
            .context("Failed to create secure extraction directory")?;
        let extract_path = extract_dir.path();

        log::debug!("Extracting to temporary directory: {:?}", extract_path);

        // Use tar and flate2 to manually extract tar.gz
        use flate2::read::GzDecoder;
        use std::fs::File;
        use tar::Archive;

        // Open tar.gz file
        let tar_gz_file =
            File::open(&temp_file_path_clone).context("Failed to open tar.gz file")?;
        let decompressor = GzDecoder::new(tar_gz_file);
        let mut archive = Archive::new(decompressor);

        // First list all files in tar.gz (for debugging)
        let tar_gz_file_list =
            File::open(&temp_file_path_clone).context("Failed to open tar.gz file for listing")?;
        let decompressor_list = GzDecoder::new(tar_gz_file_list);
        let mut archive_list = Archive::new(decompressor_list);

        log::debug!(" File list in tar.gz:");
        let mut found_usher = false;
        let mut usher_path_in_archive = String::new();
        for entry_result in archive_list
            .entries()
            .context("Failed to read archive entries")?
        {
            if let Ok(entry) = entry_result
                && let Ok(path) = entry.path()
            {
                let path_str = path.to_string_lossy().to_string();
                log::debug!("   - {}", path_str);
                // Check if it's the usher file (may have path prefix)
                if path_str == "usher" || path_str.ends_with("/usher") {
                    found_usher = true;
                    usher_path_in_archive = path_str;
                }
            }
        }

        if !found_usher {
            return Err(anyhow::anyhow!(
                "Could not find 'usher' in tar.gz archive. Archive path: {:?}",
                temp_file_path_clone
            ));
        }

        log::debug!(" Found usher in tar.gz: {}", usher_path_in_archive);

        // Extract all files to temporary directory
        archive
            .unpack(extract_path)
            .context("Failed to extract tar.gz archive")?;

        log::debug!(" Extraction complete, looking for usher file...");

        // Find extracted usher file
        let new_exe_path = if extract_path.join("usher").exists() {
            extract_path.join("usher")
        } else {
            // Try to find in subdirectories
            let mut found_path = None;
            fn find_usher_recursive(dir: &std::path::Path) -> Option<std::path::PathBuf> {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.file_name().map(|n| n == "usher").unwrap_or(false)
                        {
                            return Some(path);
                        } else if path.is_dir()
                            && let Some(found) = find_usher_recursive(&path)
                        {
                            return Some(found);
                        }
                    }
                }
                None
            }

            if let Some(path) = find_usher_recursive(extract_path) {
                found_path = Some(path);
            }

            if let Some(path) = found_path {
                path
            } else {
                // List extracted files for debugging
                log::debug!(" Extracted file list:");
                if let Ok(entries) = std::fs::read_dir(extract_path) {
                    for entry in entries.flatten() {
                        log::debug!("   - {:?}", entry.path());
                    }
                }
                return Err(anyhow::anyhow!(
                    "Could not find 'usher' after extracting archive. Archive path: {:?}",
                    temp_file_path_clone
                ));
            }
        };

        log::debug!("Found extracted executable file: {:?}", new_exe_path);

        // Set executable permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&new_exe_path, std::fs::Permissions::from_mode(0o755))
                .context("Failed to set executable permissions")?;
        }

        progress_callback_for_install(
            0.95,
            downloaded,
            total_size,
            crate::i18n::tr("update.installing").as_ref(),
        );

        // Replace current executable file
        // Use self_replace to safely replace executable file
        use self_update::self_replace;
        self_replace::self_replace(&new_exe_path).context("Failed to replace executable")?;

        log::debug!(" Executable file replaced successfully");

        // Verify updated path
        if let Ok(updated_exe) = std::env::current_exe() {
            log::debug!("After update, executable file path: {:?}", updated_exe);
        }

        progress_callback_for_install(
            1.0,
            downloaded,
            total_size,
            crate::i18n::tr("update.complete").as_ref(),
        );

        // Update state
        {
            let mut s = state_for_install
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            s.download_progress = 1.0;
            s.download_status = crate::i18n::tr("update.complete_restarting").into_owned();
        }

        // TempDir removes the extraction directory when this worker exits.
        let _ = std::fs::remove_file(&temp_file_path_clone);

        // Brief delay to let user see completion status
        std::thread::sleep(std::time::Duration::from_millis(800));

        progress_callback_for_install(
            1.0,
            downloaded,
            total_size,
            crate::i18n::tr("update.restarting").as_ref(),
        );

        // Restart application
        restart_application()?;

        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?;

    // Clean up temporary file (if still exists)
    let _ = tokio::fs::remove_file(&temp_file_path).await;

    // Reset download status
    {
        let mut s = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        s.is_downloading = false;
    }

    install_result
}

/// Perform update (original method, kept for compatibility)
#[allow(dead_code)]
pub async fn perform_update(_state: &Arc<Mutex<UpdateState>>) -> Result<()> {
    // Execute synchronous operations in blocking thread pool
    tokio::task::spawn_blocking(move || -> Result<(), anyhow::Error> {
        let updater = create_updater()?;
        updater.update()?;

        // Restart program after update completes
        restart_application()?;

        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("Task join error: {}", e))?
    .map_err(|e| anyhow::anyhow!("Update error: {}", e))?;

    Ok(())
}

/// Restart application
fn restart_application() -> Result<()> {
    log::debug!(" Preparing to restart application...");

    // Get current executable file path
    let current_exe = std::env::current_exe().context("Failed to resolve current executable")?;
    log::debug!(" Current executable file: {:?}", current_exe);

    // Get command line arguments (excluding first argument, which is program name)
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The replacement process performs the same single-instance check during
    // startup. Release our lock before spawning it, otherwise it exits before
    // this process reaches graceful shutdown.
    crate::single_instance::release_instance_lock()
        .context("Failed to release the single-instance lock for restart")?;

    let spawn_result = (|| -> std::io::Result<()> {
        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            Command::new("cmd")
                .args(["/c", "start", "", &current_exe.to_string_lossy()])
                .args(&args)
                .spawn()?;
        }

        #[cfg(target_os = "macos")]
        {
            use std::process::Command;
            // macOS: Use open command to reopen application
            // If it's a .app bundle, use open; otherwise directly run executable file
            let exe_path = current_exe.to_string_lossy();
            if exe_path.contains(".app/") {
                // Find .app directory
                if let Some(app_pos) = exe_path.find(".app/") {
                    let app_path = &exe_path[..app_pos + 4];
                    log::debug!(" Restarting macOS app: {}", app_path);
                    Command::new("open").arg("-n").arg(app_path).spawn()?;
                }
            } else {
                // Directly run executable file
                log::debug!(" Directly running executable file: {}", exe_path);
                Command::new(&*exe_path).args(&args).spawn()?;
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            // Linux: Directly launch new process
            Command::new(&current_exe).args(&args).spawn()?;
        }

        Ok(())
    })();

    if let Err(error) = spawn_result {
        // Keep the current process protected if launching the replacement
        // failed. A failed reacquire is logged separately, while the original
        // spawn error remains the user-facing failure.
        match crate::single_instance::acquire_instance_lock() {
            Ok(true) => {}
            Ok(false) => {
                log::error!(
                    " Another process acquired the instance lock after restart failure; shutting down this instance"
                );
                crate::shutdown::request_shutdown("update-restart-lock-lost");
            }
            Err(lock_error) => {
                log::error!(
                    " Failed to reacquire instance lock after restart failure: {lock_error}"
                );
            }
        }
        return Err(error).context("Failed to launch replacement process");
    }

    // Gracefully stop current process to allow cleanup hooks to run.
    log::debug!(" Requesting graceful shutdown for restart");
    crate::shutdown::request_shutdown("update-restart");
    Ok(())
}

/// Reset update state
pub fn reset_update_state(state: &Arc<Mutex<UpdateState>>) {
    let mut s = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    s.is_downloading = false;
    s.download_progress = 0.0;
    s.download_status = String::new();
    s.downloaded_bytes = 0;
    s.total_bytes = 0;
    s.error = String::new();
}

/// Updater context returned from setup_updater
/// Contains the runtime and state needed for auto-update control
pub struct UpdaterContext {
    pub rt: Arc<Runtime>,
    pub state: Arc<Mutex<UpdateState>>,
}

/// Setup updater - initialize all update-related callbacks and logic
///
/// This function sets up:
/// - Manual update check callback (on_check_update)
/// - Install update callback (on_install)
/// - Cancel download callback (on_cancel_download)
///
/// Note: Auto-update initialization and callback should be done in lib.rs
/// after settings are loaded, using the returned UpdaterContext.
pub fn setup_updater(
    app: &MainWindow,
    settings_page: &SettingsPageBridge,
) -> Result<UpdaterContext> {
    let state = Arc::new(Mutex::new(UpdateState::default()));
    let rt = shared_runtime();
    let app_weak = app.as_weak();

    // Global cancel flag
    let cancel_flag: CancelFlag = Arc::new(AtomicBool::new(false));

    // ========= Manual update check callback ========= //
    let state_clone_check = state.clone();
    let app_weak_clone_check = app_weak.clone();
    let rt_clone_check = rt.clone();
    let settings_page_check = settings_page.clone();
    app.on_check_update(move || {
        log::debug!(" check-update callback called");
        let state = state_clone_check.clone();
        let ui_weak = app_weak_clone_check.clone();
        let rt = rt_clone_check.clone();
        let settings_page = settings_page_check.clone();

        // Operation dual-write: host UpdateLogic + isolated Settings page.
        // Do not rely on Slint `changed` alone for begin/end.
        if let Some(ui) = ui_weak.upgrade() {
            mirror_update_checking(&ui, &settings_page, true);
        }

        rt.spawn(async move {
            log::debug!(" Starting update check...");
            // Reset state
            {
                let mut s = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                s.available = false;
                s.version.clear();
                s.error.clear();
                s.progress = 0.0;
            }

            log::debug!(" Calling check_for_update...");
            let result = check_for_update(&state).await;
            log::debug!("check_for_update completed, result: {:?}", result.is_ok());
            let (is_up_to_date, version) = {
                match &result {
                    Ok(up_to_date) => {
                        let v = if !*up_to_date {
                            let s = state
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            s.version.clone()
                        } else {
                            String::new()
                        };
                        log::debug!(" is_up_to_date: {}, version: {}", *up_to_date, v);
                        (*up_to_date, v)
                    }
                    Err(_) => (false, String::new()),
                }
            };

            match result {
                Ok(_) => {
                    let version_clone = version.clone();
                    log::debug!(
                        "Processing result: is_up_to_date={}, version={}",
                        is_up_to_date,
                        version_clone
                    );

                    if is_up_to_date {
                        // Current version is the latest
                        let ui_weak_for_completion = ui_weak.clone();
                        let settings_page_for_completion = settings_page.clone();
                        let ui_weak_for_logic = ui_weak.clone();

                        slint::invoke_from_event_loop(move || {
                            let Some(ui) = ui_weak_for_completion.upgrade() else {
                                settings_page_for_completion.set_update_checking(false);
                                return;
                            };
                            // Clear busy before feedback so the control is
                            // interactive when Notify appears.
                            mirror_update_checking(&ui, &settings_page_for_completion, false);
                            let version = cargo_crate_version!();
                            NotificationManager::get(&ui).invoke_success(NotificationOptions {
                                message: crate::i18n::tr_args(
                                    "update.latest_message",
                                    &[("version", version)],
                                )
                                .into(),
                                title: crate::i18n::tr("update.latest_title").as_ref().into(),
                                duration: 4000,
                                icon: "".into(),
                                component: CustomComponentType::None,
                                actions: Default::default(),
                                tag: "software-update".into(),
                                source: "Updater".into(),
                                category: "software_update".into(),
                                native_delivery: NotificationDelivery::WhenUnfocused,
                            });
                        })
                        .ok();

                        // Use Timer to delay logic reset, ensure execution in next event loop iteration
                        Timer::single_shot(std::time::Duration::from_millis(100), move || {
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak_for_logic.upgrade() {
                                    {
                                        let logic = ui.global::<UpdateLogic>();
                                        logic.set_error("".into());
                                        logic.set_update_available(false);
                                    } // logic borrow released here
                                }
                            })
                            .ok();
                        });
                    } else {
                        log::debug!(" Entering branch with new version available");
                        // New version available
                        let ui_weak_for_completion = ui_weak.clone();
                        let version_for_all = version.clone();
                        let settings_page_for_completion = settings_page.clone();

                        // Manual checks report every outcome through the shared
                        // Primer Notify instead of opening a separate modal.
                        slint::invoke_from_event_loop(move || {
                            let Some(ui) = ui_weak_for_completion.upgrade() else {
                                settings_page_for_completion.set_update_checking(false);
                                return;
                            };
                            log::debug!(" Setting state");
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_update_available(true);
                            logic.set_new_version(version_for_all.clone().into());
                            drop(logic);
                            mirror_update_checking(&ui, &settings_page_for_completion, false);
                            log::debug!(
                                "Set update-available=true, new-version={}",
                                version_for_all
                            );

                            let current_version = cargo_crate_version!();
                            let actions: ModelRc<SharedString> =
                                ModelRc::new(Rc::new(VecModel::from(vec![
                                    SharedString::from(crate::i18n::tr("update.later").as_ref()),
                                    SharedString::from(
                                        crate::i18n::tr("update.update_now").as_ref(),
                                    ),
                                ])));
                            NotificationManager::get(&ui).invoke_info(NotificationOptions {
                                title: crate::i18n::tr("update.available_title").as_ref().into(),
                                message: crate::i18n::tr_args(
                                    "update.available_message",
                                    &[
                                        ("new_version", version_for_all.as_str()),
                                        ("current_version", current_version),
                                    ],
                                )
                                .into(),
                                duration: 0,
                                icon: "".into(),
                                component: CustomComponentType::ActionsNotify,
                                actions,
                                tag: "manual-update".into(),
                                source: "Updater".into(),
                                category: "software_update".into(),
                                native_delivery: NotificationDelivery::WhenUnfocused,
                            });
                        })
                        .ok();
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    log::debug!(" Update check error: {}", error_msg);
                    // Keep technical details in the log. The Notify uses a
                    // concise, provider-neutral recovery message.
                    let friendly_message_key = update_check_error_message_key(&error_msg);
                    let ui_weak_for_completion = ui_weak.clone();
                    let ui_weak_for_logic = ui_weak.clone();
                    let settings_page_for_completion = settings_page.clone();

                    slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak_for_completion.upgrade() else {
                            settings_page_for_completion.set_update_checking(false);
                            return;
                        };
                        mirror_update_checking(&ui, &settings_page_for_completion, false);
                        NotificationManager::get(&ui).invoke_error(NotificationOptions {
                            message: crate::i18n::tr(friendly_message_key).as_ref().into(),
                            title: crate::i18n::tr("update.check_failed_title").as_ref().into(),
                            duration: 8000,
                            icon: "".into(),
                            component: CustomComponentType::None,
                            actions: Default::default(),
                            tag: "software-update".into(),
                            source: "Updater".into(),
                            category: "software_update".into(),
                            native_delivery: NotificationDelivery::WhenUnfocused,
                        });
                    })
                    .ok();

                    // Use Timer to delay logic reset, ensure execution in next event loop iteration
                    Timer::single_shot(std::time::Duration::from_millis(100), move || {
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_logic.upgrade() {
                                {
                                    let logic = ui.global::<UpdateLogic>();
                                    logic.set_error("".into());
                                    logic.set_update_available(false);
                                } // logic borrow released here
                            }
                        })
                        .ok();
                    });
                }
            }
        });
    });

    // ========= Install update callback ========= //
    let state_clone = state.clone();
    let app_weak_clone = app_weak.clone();
    let rt_clone = rt.clone();
    let cancel_flag_clone = cancel_flag.clone();
    app.on_install(move || {
        log::debug!(" on_install callback called");
        let state = state_clone.clone();
        let ui_weak = app_weak_clone.clone();
        let rt = rt_clone.clone();
        let cancel_flag = cancel_flag_clone.clone();

        // Reset cancel flag
        cancel_flag.store(false, Ordering::Relaxed);

        // Use Timer to delay all operations, avoid RefCell borrow conflict
        Timer::single_shot(std::time::Duration::from_millis(100), move || {
            log::debug!(" Timer callback executed, preparing to show download progress modal");

            // Show download progress modal
            if let Some(ui) = ui_weak.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_is_downloading(true);
                logic.set_download_progress(0.0);
                logic.set_download_status(crate::i18n::tr("update.preparing").into_owned().into());
                log::debug!(" Download progress modal shown");
            }

            // Create progress callback
            let ui_weak_for_progress = ui_weak.clone();
            let progress_callback = Arc::new(
                move |progress: f32, downloaded: u64, total: u64, status: &str| {
                    let ui_weak = ui_weak_for_progress.clone();
                    let status_str = status.to_string();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_download_progress(progress);
                            logic.set_download_status(status_str.into());
                            logic.set_downloaded_bytes(downloaded as i32);
                            logic.set_total_bytes(total as i32);
                        }
                    })
                    .ok();
                },
            );

            let ui_weak_for_error = ui_weak.clone();
            rt.spawn(async move {
                log::debug!(" Starting update with progress...");
                let update_result =
                    perform_update_with_progress(&state, progress_callback, cancel_flag.clone())
                        .await;

                match &update_result {
                    Ok(_) => {
                        log::debug!(" Update successful, program will restart");
                        // After successful update, program will automatically restart, no additional handling needed here
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        log::debug!(" Update failed: {}", error_msg);

                        // Reset state
                        reset_update_state(&state);

                        let is_cancelled = error_msg.contains("cancelled");
                        let ui_weak_for_error = ui_weak_for_error.clone();

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_error.upgrade() {
                                // Hide download progress modal
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_is_downloading(false);
                                logic.set_download_progress(0.0);
                                logic.set_download_status("".into());

                                if !is_cancelled {
                                    // Only show error notification for non-cancelled cases
                                    let manager = NotificationManager::get(&ui);
                                    manager.invoke_error(NotificationOptions {
                                        message: crate::i18n::tr("update.install_failed")
                                            .as_ref()
                                            .into(),
                                        title: "".into(),
                                        duration: 5000,
                                        icon: "".into(),
                                        component: CustomComponentType::None,
                                        actions: Default::default(),
                                        tag: "software-update".into(),
                                        source: "Updater".into(),
                                        category: "software_update".into(),
                                        native_delivery: NotificationDelivery::WhenUnfocused,
                                    });
                                }
                            }
                        })
                        .ok();
                    }
                }
            });
        });
    });

    // ========= Cancel download callback ========= //
    let cancel_flag_clone = cancel_flag.clone();
    let state_clone = state.clone();
    let app_weak_clone = app_weak.clone();
    app.on_cancel_download(move || {
        log::debug!(" on_cancel_download callback called");

        // Set cancel flag
        cancel_flag_clone.store(true, Ordering::Relaxed);

        // Reset state
        reset_update_state(&state_clone);

        // Use Timer to delay UI update, avoid RefCell borrow conflict
        let ui_weak = app_weak_clone.clone();
        Timer::single_shot(std::time::Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_is_downloading(false);
                logic.set_download_progress(0.0);
                logic.set_download_status("".into());
                logic.set_update_available(false);

                // Show cancel notification
                let manager = NotificationManager::get(&ui);
                manager.invoke_info(NotificationOptions {
                    message: crate::i18n::tr("update.cancelled").as_ref().into(),
                    title: "".into(),
                    duration: 3000,
                    icon: "".into(),
                    component: CustomComponentType::None,
                    actions: Default::default(),
                    tag: "software-update".into(),
                    source: "Updater".into(),
                    category: "software_update".into(),
                    native_delivery: NotificationDelivery::WhenUnfocused,
                });
            }
        });
    });

    // Return the context for auto-update control
    Ok(UpdaterContext { rt, state })
}

#[cfg(test)]
mod tests {
    use super::{UpdateChannel, select_release};
    use self_update::update::Release;

    fn release(version: &str) -> Release {
        Release {
            version: version.to_string(),
            ..Release::default()
        }
    }

    #[test]
    fn stable_channel_ignores_prereleases_and_invalid_versions() {
        let selected = select_release(
            vec![
                release("1.2.0-alpha.3"),
                release("not-semver"),
                release("1.1.9"),
                release("1.2.0-rc.1"),
            ],
            UpdateChannel::Stable,
        )
        .unwrap();

        assert_eq!(selected.version, "1.1.9");
    }

    #[test]
    fn alpha_channel_selects_the_highest_semver_release() {
        let selected = select_release(
            vec![
                release("1.2.0-alpha.3"),
                release("1.1.9"),
                release("1.2.0-beta.1"),
            ],
            UpdateChannel::Alpha,
        )
        .unwrap();

        assert_eq!(selected.version, "1.2.0-beta.1");
    }

    #[test]
    fn stable_channel_returns_none_when_only_prereleases_exist() {
        assert!(
            select_release(
                vec![release("2.0.0-alpha.1"), release("2.0.0-rc.1")],
                UpdateChannel::Stable,
            )
            .is_none()
        );
    }
}
