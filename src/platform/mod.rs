//! Native desktop integration behind a small cross-platform application API.
//!
//! Feature modules must not depend on AppKit or raw window handles directly.
//! Platform-specific title-bar, appearance, interaction and font behavior is
//! kept here so `src/lib.rs` remains the application composition root.

use std::path::Path;
use std::process::Command;

use crate::MainWindow;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::is_window_interacting;

#[cfg(target_os = "macos")]
pub(crate) use macos::{
    detect_system_dark_mode, install_titlebar_settings_button, play_ui_click, set_app_appearance,
    set_title_bar_color, setup_pr_review_fonts,
};

#[cfg(not(target_os = "macos"))]
pub fn is_window_interacting() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn detect_system_dark_mode() -> bool {
    // TODO: Implement native system-theme detection for Windows and Linux.
    false
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_app_appearance(_is_dark: bool) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_title_bar_color(_window: &MainWindow, _is_dark: bool) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_titlebar_settings_button(_window: &MainWindow) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn play_ui_click() {
    // System click sounds are only wired on macOS for the first pass.
}

pub(crate) fn setup_application_font(app: &MainWindow) {
    #[cfg(target_os = "macos")]
    let family = "Hiragino Sans GB";
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
        0 => false,
        1 => true,
        2 => detect_system_dark_mode(),
        _ => false,
    }
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

#[cfg(test)]
mod tests {
    use super::{effective_dark_mode, open_directory_tooltip};

    #[test]
    fn resolves_explicit_theme_modes_without_platform_state() {
        assert!(!effective_dark_mode(0));
        assert!(effective_dark_mode(1));
        assert!(!effective_dark_mode(-1));
        assert!(!effective_dark_mode(99));
    }

    #[test]
    fn open_directory_tooltip_is_non_empty() {
        assert!(!open_directory_tooltip().is_empty());
    }
}
