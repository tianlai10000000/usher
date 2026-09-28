//! Opt-in native presentation controls for source-built memory fixtures.
//!
//! Production builds do not compile this module. The benchmark window remains
//! rendered, but it does not activate the application or receive system mouse
//! input. Fixture code drives Slint through event-loop callbacks instead.

use crate::MainWindow;

const NONACTIVATING_MODE: &str = "winit-background-v1";

pub(crate) fn nonactivating_window_requested() -> bool {
    std::env::var("USHER_MEMORY_PRESENTATION").as_deref() == Ok(NONACTIVATING_MODE)
}

pub(crate) fn select_nonactivating_backend() -> Result<(), slint::PlatformError> {
    if !nonactivating_window_requested() {
        return Ok(());
    }
    if !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return Err(
            "USHER_MEMORY_PRESENTATION requires offline mode and an isolated state root".into(),
        );
    }

    #[cfg(target_os = "macos")]
    {
        use slint::winit_030::winit::platform::macos::{
            ActivationPolicy, EventLoopBuilderExtMacOS,
        };

        let mut event_loop: slint::winit_030::EventLoopBuilder =
            slint::winit_030::winit::event_loop::EventLoop::<
                slint::winit_030::SlintEvent,
            >::with_user_event();
        event_loop
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_activate_ignoring_other_apps(false);
        return slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("femtovg".into())
            .with_winit_event_loop_builder(event_loop)
            .with_winit_window_attributes_hook(|attributes| {
                attributes
                    .with_active(false)
                    .with_window_level(slint::winit_030::winit::window::WindowLevel::AlwaysOnBottom)
            })
            .select();
    }

    #[cfg(not(target_os = "macos"))]
    Err("The non-activating memory presentation is currently macOS-only".into())
}

pub(crate) fn configure_noninteractive_window(window: &MainWindow) {
    if !nonactivating_window_requested() {
        return;
    }

    use slint::ComponentHandle;
    use slint::winit_030::WinitWindowAccessor;

    let configured = window
        .window()
        .with_winit_window(|native| {
            native.set_window_level(slint::winit_030::winit::window::WindowLevel::AlwaysOnBottom);
            native.set_cursor_hittest(false)
        })
        .transpose();
    match configured {
        Ok(Some(())) => log::info!(
            "[Memory][Presentation] mode={NONACTIVATING_MODE} active=false cursor_hittest=false"
        ),
        Err(error) => {
            log::error!("[Memory][Presentation] failed to disable cursor hit testing: {error}")
        }
        Ok(None) => log::error!("[Memory][Presentation] native Winit window is unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::NONACTIVATING_MODE;

    #[test]
    fn presentation_mode_is_explicit_and_versioned() {
        assert_eq!(NONACTIVATING_MODE, "winit-background-v1");
    }
}
