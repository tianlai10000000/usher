//! Cross-global popover exclusivity; all callbacks stay on the event loop.
use slint::ComponentHandle;

pub(super) fn bind(
    window: &crate::MainWindow,
    page: &usher_ui_components::GitHubUiComponentsWindow,
) {
    let weak = window.as_weak();
    page.on_claim_host_exclusive(move || {
        if let Some(window) = weak.upgrade() {
            window
                .global::<crate::UiComponentsBridge>()
                .invoke_claim_host_exclusive();
        }
    });
}

#[cfg(test)]
#[path = "popover_tests.rs"]
mod tests;
