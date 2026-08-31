//! Native desktop integration behind a small cross-platform application API.
//!
//! Feature modules must not depend on AppKit or raw window handles directly.
//! Platform-specific title-bar, appearance, interaction and font behavior is
//! kept here so `src/lib.rs` remains the application composition root.

use std::path::Path;
use std::process::Command;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::MainWindow;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

/// Persisted theme choice: Light / Dark / Auto.
pub(crate) const THEME_MODE_LIGHT: i32 = 0;
pub(crate) const THEME_MODE_DARK: i32 = 1;
pub(crate) const THEME_MODE_AUTO: i32 = 2;

type SystemThemeHandler = Arc<dyn Fn(bool) + Send + Sync + 'static>;

static SYSTEM_THEME_HANDLER: OnceLock<SystemThemeHandler> = OnceLock::new();
static AUTO_THEME_WATCH_ENABLED: AtomicBool = AtomicBool::new(false);
static LAST_SYSTEM_DARK: AtomicBool = AtomicBool::new(false);
static THEME_WATCH_STARTED: Once = Once::new();

#[cfg(target_os = "macos")]
pub use macos::is_window_interacting;

#[cfg(target_os = "macos")]
pub(crate) use macos::{
    activate_main_window, detect_system_dark_mode, follow_system_appearance,
    install_artifact_preview_live_resize, install_artifact_preview_titlebar_actions,
    install_memory_benchmark_window_control, install_shortcut_input_bridge,
    install_titlebar_settings_button, play_ui_click, prepare_startup_reveal, set_app_appearance,
    set_startup_presentation_hidden, set_title_bar_color, setup_pr_review_fonts,
    startup_native_chrome_ready, update_titlebar_navigation_controls,
};

#[cfg(target_os = "windows")]
pub(crate) use windows::{detect_system_dark_mode, play_ui_click, show_startup_error};

#[cfg(target_os = "linux")]
pub(crate) use linux::{detect_system_dark_mode, play_ui_click, show_startup_error};

#[cfg(target_os = "macos")]
pub(crate) use macos::show_startup_error;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) fn detect_system_dark_mode() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn is_window_interacting() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_app_appearance(_is_dark: bool) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn follow_system_appearance() {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_title_bar_color(_window: &MainWindow, _is_dark: bool) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_titlebar_settings_button(_window: &MainWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_memory_benchmark_window_control(_window: &MainWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_artifact_preview_live_resize(_window: &crate::ArtifactPreviewWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_artifact_preview_titlebar_actions(_window: &crate::ArtifactPreviewWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_startup_presentation_hidden(_window: &MainWindow, _hidden: bool) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn startup_native_chrome_ready() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn prepare_startup_reveal(_window: &MainWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_shortcut_input_bridge(_window: &MainWindow) -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn update_titlebar_navigation_controls(_window: &MainWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn activate_main_window(_window: &MainWindow) {}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) fn show_startup_error(message: &str) {
    eprintln!("Usher could not start: {message}");
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) fn play_ui_click() {
    log::debug!("[Sound] UI click is unavailable on this platform");
}

#[cfg(target_os = "macos")]
pub(crate) use macos::shutdown_native_integrations;

#[cfg(not(target_os = "macos"))]
pub(crate) fn shutdown_native_integrations() {}

pub(crate) fn setup_application_font(app: &MainWindow) {
    #[cfg(target_os = "macos")]
    let family = ".AppleSystemUIFont";
    #[cfg(target_os = "windows")]
    let family = "Microsoft YaHei UI";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let family = "Noto Sans";

    app.set_application_font_family(family.into());
    log::info!("Using application font family: {family}");
}

#[cfg(target_os = "windows")]
pub(crate) fn setup_pr_review_fonts(app: &MainWindow) {
    app.set_workflow_pr_review_code_font_family("Consolas".into());
    app.set_workflow_pr_review_tree_font_family("Segoe UI".into());
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn setup_pr_review_fonts(app: &MainWindow) {
    app.set_workflow_pr_review_code_font_family("Liberation Mono".into());
    app.set_workflow_pr_review_tree_font_family("Noto Sans".into());
}

/// Resolve the persisted theme choice to the effective light/dark appearance.
pub(crate) fn effective_dark_mode(theme_mode: i32) -> bool {
    match theme_mode {
        THEME_MODE_LIGHT => false,
        THEME_MODE_DARK => true,
        THEME_MODE_AUTO => detect_system_dark_mode(),
        _ => false,
    }
}

/// Apply native window chrome for Light/Dark/Auto without resolving Auto's palette.
///
/// Auto clears any forced AppKit appearance so the process can track the OS;
/// explicit Light/Dark lock Aqua/DarkAqua on macOS. No-op chrome helpers on
/// Windows/Linux still leave Slint palette updates to the caller.
pub(crate) fn apply_native_chrome_for_mode(theme_mode: i32) {
    if theme_mode == THEME_MODE_AUTO {
        follow_system_appearance();
    } else {
        set_app_appearance(theme_mode == THEME_MODE_DARK);
    }
}

/// Apply native chrome for Light/Dark/Auto and return the effective dark flag.
pub(crate) fn apply_native_appearance_for_mode(theme_mode: i32) -> bool {
    apply_native_chrome_for_mode(theme_mode);
    effective_dark_mode(theme_mode)
}

/// Enable or disable live system-theme watching used by Auto mode.
pub(crate) fn set_auto_theme_watch_enabled(enabled: bool) {
    if enabled {
        LAST_SYSTEM_DARK.store(detect_system_dark_mode(), Ordering::SeqCst);
    }
    AUTO_THEME_WATCH_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Install the cross-platform Auto theme watcher once.
///
/// - macOS: `AppleInterfaceThemeChangedNotification` plus a 1s poll fallback
/// - Windows / Linux: 1s poll of the native theme preference / XDG portal
///
/// `on_change` runs on the Slint event loop with the newly detected dark flag.
pub(crate) fn install_system_theme_watch(on_change: impl Fn(bool) + Send + Sync + 'static) {
    let _ = SYSTEM_THEME_HANDLER.set(Arc::new(on_change));
    LAST_SYSTEM_DARK.store(detect_system_dark_mode(), Ordering::SeqCst);

    #[cfg(target_os = "macos")]
    macos::install_system_theme_change_observer();

    THEME_WATCH_STARTED.call_once(|| {
        if let Err(error) = std::thread::Builder::new()
            .name("system-theme-watch".to_string())
            .spawn(|| {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    if !AUTO_THEME_WATCH_ENABLED.load(Ordering::SeqCst) {
                        continue;
                    }
                    publish_system_theme_if_changed();
                }
            })
        {
            log::warn!("[Theme] Failed to start system theme watcher: {error}");
        }
    });
}

/// Re-read the OS theme and notify the UI when Auto mode is active and it changed.
pub(crate) fn publish_system_theme_if_changed() {
    if !AUTO_THEME_WATCH_ENABLED.load(Ordering::SeqCst) {
        return;
    }
    let is_dark = detect_system_dark_mode();
    let previous = LAST_SYSTEM_DARK.swap(is_dark, Ordering::SeqCst);
    if previous == is_dark {
        return;
    }
    let Some(handler) = SYSTEM_THEME_HANDLER.get().cloned() else {
        return;
    };
    let _ = slint::invoke_from_event_loop(move || {
        handler(is_dark);
    });
}

/// GitHub Desktop wording for revealing a folder in the OS file manager.
pub(crate) fn open_directory_tooltip() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Show in Finder"
    }
    #[cfg(target_os = "windows")]
    {
        "Show in Explorer"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "Show in file manager"
    }
}

/// Open `path` in the platform file manager without blocking for completion.
pub(crate) fn open_directory_in_file_manager(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("Local path is empty.".to_string());
    }
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("Path is not a directory: {}", path.display()));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to open Finder: {error}"))
    }

    #[cfg(target_os = "windows")]
    {
        // `explorer` returns a non-zero status even on success; only spawn matters.
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to open Explorer: {error}"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Failed to open file manager: {error}"))
    }
}

/// Open a file with the operating system's associated application.
///
/// This is only called from an explicit user action. The child is spawned and
/// not waited on, so the Slint event loop is never blocked.
pub(crate) fn open_file_with_system_application(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(path).spawn();

    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn();

    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(path).spawn();

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let result: std::io::Result<std::process::Child> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "opening files is unsupported on this platform",
    ));

    result
        .map(|_| ())
        .map_err(|error| format!("Failed to open file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{THEME_MODE_DARK, THEME_MODE_LIGHT, effective_dark_mode, open_directory_tooltip};

    #[test]
    fn resolves_explicit_theme_modes_without_platform_state() {
        assert!(!effective_dark_mode(THEME_MODE_LIGHT));
        assert!(effective_dark_mode(THEME_MODE_DARK));
        assert!(!effective_dark_mode(-1));
        assert!(!effective_dark_mode(99));
    }

    #[test]
    fn open_directory_tooltip_is_non_empty() {
        assert!(!open_directory_tooltip().is_empty());
    }
}
