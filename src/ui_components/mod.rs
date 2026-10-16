#![cfg(all(feature = "ui-components", not(feature = "live-preview")))]

//! Event-loop-only ownership for the optional static component playground.
mod feedback;
mod fixture;
mod keyboard;
mod popover;

use crate::{AppState, MainWindow};
use slint::{ComponentFactory, ComponentHandle};
use std::{cell::RefCell, rc::Rc};
use usher_ui_components::{AppState as GalleryAppState, GitHubUiComponentsWindow};

type PageSlot = Rc<RefCell<Option<slint::Weak<GitHubUiComponentsWindow>>>>;

fn with_page(slot: &PageSlot, apply: impl FnOnce(&GitHubUiComponentsWindow)) {
    let weak = slot.borrow().clone();
    if let Some(page) = weak.and_then(|weak| weak.upgrade()) {
        apply(&page);
    }
}

fn sync(window: &MainWindow, page: &GitHubUiComponentsWindow) {
    let host = window.global::<crate::UiComponentsBridge>();
    let bounds = host.get_window_bounds();
    let viewport = host.get_viewport();
    page.set_window_width(bounds.width);
    page.set_window_height(bounds.height);
    page.set_content_x(viewport.x);
    page.set_content_y(viewport.y);
    page.set_content_width(viewport.width);
    page.set_content_height(viewport.height);
    page.set_content_top_padding(host.get_top_padding());
    page.set_application_font_family(window.get_application_font_family());
    page.set_route_active(host.get_active());
    let source = window.global::<AppState>();
    let target = page.global::<GalleryAppState>();
    target.set_theme_mode(source.get_theme_mode());
    target.set_effective_dark_mode(source.get_effective_dark_mode());
}

fn route(value: usher_ui_components::Route) -> crate::Route {
    use usher_ui_components::Route as R;
    match value {
        R::Home => crate::Route::Home,
        R::Search => crate::Route::Search,
        R::Messages => crate::Route::Messages,
        R::Settings => crate::Route::Settings,
        R::Project => crate::Route::Project,
        R::Chat => crate::Route::Chat,
        R::UiComponents => crate::Route::UiComponents,
        R::About => crate::Route::About,
        R::Workflow => crate::Route::Workflow,
        R::JobRun => crate::Route::JobRun,
        R::PrReview => crate::Route::PrReview,
    }
}

fn create_page(window: &MainWindow) -> Result<GitHubUiComponentsWindow, slint::PlatformError> {
    // Embedded trees must unregister from the renderer that actually paints
    // them. A standalone hidden window leaves stale host caches after a
    // conditional/model drops its children. Restore the shell immediately:
    // Slint's shared-window constructor temporarily installs its own root.
    let host = i_slint_core::window::WindowInner::from_pub(window.window());
    let shell = host.component();
    let page = GitHubUiComponentsWindow::new_with_existing_window(window.window());
    host.set_component(&shell);
    page
}

#[doc(hidden)]
pub fn install(window: &MainWindow) {
    let trace = fixture::arm(window);
    let slot: PageSlot = Rc::default();
    let page_slot = slot.clone();
    let weak = window.as_weak();
    window
        .global::<crate::UiComponentsBridge>()
        .set_factory(ComponentFactory::new(move |_| {
            let window = weak.upgrade()?;
            let page = match create_page(&window) {
                Ok(page) => page,
                Err(error) => {
                    log::error!("[UI Components] Failed to create page: {error}");
                    return None;
                }
            };
            sync(&window, &page);
            if trace {
                fixture::trace(&page);
            }
            feedback::bind(&window, &page);
            popover::bind(&window, &page);
            keyboard::bind(&window, &page);
            let weak = window.as_weak();
            page.on_navigate(move |value| {
                if let Some(window) = weak.upgrade() {
                    window.invoke_navigate(route(value));
                }
            });
            let weak = window.as_weak();
            page.on_open_artifact_preview_fixture(move |id| {
                if let Some(window) = weak.upgrade() {
                    window.invoke_open_artifact_preview_fixture(id);
                }
            });
            let weak = window.as_weak();
            let bridge = page.global::<usher_ui_components::FloatingOverlayBridge>();
            bridge.on_position(move |a, b, s, gap, padding, side, align| {
                let Some(window) = weak.upgrade() else {
                    return Default::default();
                };
                let rect =
                    |v: usher_ui_components::FloatingOverlayRect| crate::FloatingOverlayRect {
                        x: v.x,
                        y: v.y,
                        width: v.width,
                        height: v.height,
                    };
                let p = window
                    .global::<crate::FloatingOverlayBridge>()
                    .invoke_position(rect(a), rect(b), rect(s), gap, padding, side, align);
                usher_ui_components::FloatingOverlayRect {
                    x: p.x,
                    y: p.y,
                    width: p.width,
                    height: p.height,
                }
            });
            bridge.set_ready(true);
            *page_slot.borrow_mut() = Some(page.as_weak());
            Some(page)
        }));
    let page_slot = slot.clone();
    let weak = window.as_weak();
    window
        .global::<crate::UiComponentsBridge>()
        .on_sync(move || {
            if let Some(window) = weak.upgrade() {
                with_page(&page_slot, |page| {
                    sync(&window, page);
                    if trace {
                        fixture::trace(page);
                    }
                });
            }
        });
    let page_slot = slot.clone();
    window
        .global::<crate::UiComponentsBridge>()
        .on_dismiss(move |all| {
            with_page(&page_slot, |page| page.invoke_dismiss_transients(all));
        });
    let weak = window.as_weak();
    window
        .global::<crate::UiComponentsBridge>()
        .on_pointer_released(move |x, y| {
            with_page(&slot, |page| page.invoke_select_pointer_released(x, y));
            if trace {
                if let Some(window) = weak.upgrade() {
                    fixture::trace_pointer(&window, x, y);
                }
            }
        });
}

#[cfg(test)]
mod tests;
