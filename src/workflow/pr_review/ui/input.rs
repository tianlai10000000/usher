//! Event-loop input and placement across the factory's independent globals.
use super::PrReviewUiRoot;
use crate::MainWindow;
use slint::{ComponentHandle, platform::Key};

fn tab(
    window: &MainWindow,
    text: &slint::SharedString,
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> bool {
    let back = text == &slint::SharedString::from(Key::Backtab);
    if (text == &slint::SharedString::from(Key::Tab) || back) && !control && !alt && !meta {
        return crate::focus_navigation::move_tab(window, shift || back);
    }
    false
}

pub(super) fn bind(window: &MainWindow, page: &PrReviewUiRoot) {
    let host = window.as_weak();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .on_key_pressed(move |text, c, a, s, m| {
            let Some(window) = host.upgrade() else {
                return false;
            };
            super::synchronize_modal_input(&window);
            if window.get_current_route() != crate::Route::PrReview
                || window.get_modal_overlay_visible()
            {
                return false;
            }
            tab(&window, &text, c, a, s, m)
        });
    let host = window.as_weak();
    page.on_host_key_pressed(move |text, c, a, s, m| {
        let Some(window) = host.upgrade() else {
            return false;
        };
        super::synchronize_modal_input(&window);
        window.invoke_app_shortcut_key_pressed(text.clone(), c, a, s, m)
            || (!window.get_modal_overlay_visible() && tab(&window, &text, c, a, s, m))
    });
    let host = window.as_weak();
    page.on_claim_host_exclusive(move || {
        if let Some(window) = host.upgrade() {
            window
                .global::<usher_ui::PrReviewHostBridge>()
                .invoke_claim_host_exclusive();
        }
    });
    let product = page.as_weak();
    window
        .global::<usher_ui::PrReviewHostBridge>()
        .on_dismiss(move |all| {
            if let Some(page) = product.upgrade() {
                page.global::<usher_ui_pr_review::TooltipOverlayState>()
                    .invoke_dismiss_all();
                let state = page.global::<usher_ui_pr_review::TransientOverlayState>();
                if all {
                    state.invoke_dismiss_all();
                } else {
                    state.invoke_dismiss_popovers();
                }
            }
        });
    let host = window.as_weak();
    let bridge = page.global::<usher_ui_pr_review::FloatingOverlayBridge>();
    bridge.on_position(move |a, b, s, gap, padding, side, align| {
        let Some(window) = host.upgrade() else {
            return Default::default();
        };
        let rect = |v: usher_ui_pr_review::FloatingOverlayRect| crate::FloatingOverlayRect {
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
        };
        let result = window
            .global::<crate::FloatingOverlayBridge>()
            .invoke_position(rect(a), rect(b), rect(s), gap, padding, side, align);
        usher_ui_pr_review::FloatingOverlayRect {
            x: result.x,
            y: result.y,
            width: result.width,
            height: result.height,
        }
    });
    bridge.set_ready(true);
}
