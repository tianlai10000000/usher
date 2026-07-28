//! Native desktop integration behind a small cross-platform application API.
//!
//! Feature modules must not depend on AppKit or raw window handles directly.
//! Platform-specific title-bar, appearance, interaction and font behavior is
//! kept here so `src/lib.rs` remains the application composition root.

use crate::MainWindow;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::is_window_interacting;

#[cfg(target_os = "macos")]
pub(crate) use macos::{
    detect_system_dark_mode, install_titlebar_settings_button, set_app_appearance,
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

#[cfg(test)]
mod tests {
    use super::effective_dark_mode;

    #[test]
    fn resolves_explicit_theme_modes_without_platform_state() {
        assert!(!effective_dark_mode(0));
        assert!(effective_dark_mode(1));
        assert!(!effective_dark_mode(-1));
        assert!(!effective_dark_mode(99));
    }
}
