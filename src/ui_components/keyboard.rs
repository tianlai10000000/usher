//! Host keyboard ownership across the independent playground tree.
use crate::MainWindow;
use slint::{ComponentHandle, platform::Key};
use usher_ui_components::GitHubUiComponentsWindow;

fn tab(
    window: &MainWindow,
    text: &slint::SharedString,
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> bool {
    let backward = text == &slint::SharedString::from(Key::Backtab);
    if (text == &slint::SharedString::from(Key::Tab) || backward) && !control && !alt && !meta {
        return crate::focus_navigation::move_tab(window, shift || backward);
    }
    false
}

pub(super) fn bind(window: &MainWindow, page: &GitHubUiComponentsWindow) {
    let host = window.as_weak();
    window.global::<crate::UiComponentsBridge>().on_key_pressed(
        move |text, control, alt, shift, meta| {
            let Some(window) = host.upgrade() else {
                return false;
            };
            if !window.global::<crate::UiComponentsBridge>().get_active()
                || window.get_modal_overlay_visible()
            {
                return false;
            }
            tab(&window, &text, control, alt, shift, meta)
        },
    );
    let host = window.as_weak();
    let product = page.as_weak();
    page.on_host_key_pressed(move |text, control, alt, shift, meta| {
        let (Some(window), Some(page)) = (host.upgrade(), product.upgrade()) else {
            return false;
        };
        if !page.get_route_active() || window.get_modal_overlay_visible() {
            return true;
        }
        // Sharing a renderer does not create a parent link to the shell's
        // shortcut FocusScope or its surrounding Tab chain.
        if window.invoke_app_shortcut_key_pressed(text.clone(), control, alt, shift, meta) {
            return true;
        }
        tab(&window, &text, control, alt, shift, meta)
    });
}

#[cfg(test)]
#[path = "keyboard_tests.rs"]
mod tests;
