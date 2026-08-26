#[cfg(not(target_os = "macos"))]
use anyhow::{Context, Result};
use i_slint_core::context::set_window_event_hook;
use slint::ComponentHandle;
use slint::platform::{PointerEventButton, WindowEvent};

use crate::MainWindow;

/// Observe already-dispatched main-window pointer presses without knowing which
/// reusable surface owns the coordinate. The composition root routes the click
/// after the target has handled links and controls.
/// Production must call this after `Window::show()` has created its adapter.
#[cfg(not(target_os = "macos"))]
pub(crate) fn install(app: &MainWindow) -> Result<()> {
    install_for_window(app).context("Failed to install transient input bridge")
}

#[doc(hidden)]
pub fn install_for_window(app: &MainWindow) -> Result<(), slint::PlatformError> {
    let weak = app.as_weak();
    let previous = set_window_event_hook(None)?;

    set_window_event_hook(Some(Box::new(move |adapter, event, result| {
        if let Some(previous) = previous.as_ref() {
            previous(adapter, event, result);
        }

        let WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        } = event
        else {
            return;
        };
        let Some(app) = weak.upgrade() else {
            return;
        };

        // The hook is process-global. Restrict it to the MainWindow adapter;
        // detached Preview windows have their own transient interaction scope.
        if adapter.size() != app.window().size()
            || adapter.position().unwrap_or_default() != app.window().position()
        {
            return;
        }
        app.invoke_post_dispatch_pointer_pressed(position.x, position.y);
    })))?;

    Ok(())
}
