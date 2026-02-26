#![allow(unexpected_cfgs)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::Timer;

// Must be called before module declarations so generated types are available in all modules
slint::include_modules!();

mod notification;

use notification::setup_notifications;

mod toast;
use toast::setup_toast;

mod dialog;
use dialog::setup_dialog;

mod tray;
use tray::setup_tray;

mod update;
use update::{setup_updater, start_auto_update, stop_auto_update};

mod network;
use network::setup_network_monitor;

mod settings;
use settings::setup_settings;

mod constants;

mod chat;
use chat::setup_chat;

mod logger;
use logger::error_handing;
use logger::tracing_logger;
use logger::{setup_debug_logger, setup_tracing_logger};

mod router;
use router::{Router, route_for_navigation_index};

/// Set application-level appearance on macOS (called BEFORE window is shown)
/// This prevents the flash of wrong theme on startup
#[cfg(target_os = "macos")]
fn set_app_appearance(is_dark: bool) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use std::ffi::CString;

    unsafe {
        let ns_app_class = match Class::get("NSApplication") {
            Some(cls) => cls,
            None => return,
        };

        let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
        if ns_app.is_null() {
            return;
        }

        let ns_appearance_class = match Class::get("NSAppearance") {
            Some(cls) => cls,
            None => return,
        };

        let ns_string_class = match Class::get("NSString") {
            Some(cls) => cls,
            None => return,
        };

        let appearance_name = if is_dark {
            "NSAppearanceNameDarkAqua"
        } else {
            "NSAppearanceNameAqua"
        };

        if let Ok(c_str) = CString::new(appearance_name) {
            let name_obj: *mut Object =
                msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
            if !name_obj.is_null() {
                let appearance: *mut Object =
                    msg_send![ns_appearance_class, appearanceNamed: name_obj];
                if !appearance.is_null() {
                    let _: () = msg_send![ns_app, setAppearance: appearance];
                    log::info!(
                        "[Theme] Set app appearance to: {}",
                        if is_dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_app_appearance(_is_dark: bool) {
    // Other platforms not supported yet
}

/// Detect system theme preference on macOS
/// Returns true if system is in dark mode, false for light mode
#[cfg(target_os = "macos")]
fn detect_system_dark_mode() -> bool {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use std::ffi::CString;

    unsafe {
        // Get NSUserDefaults.standardUserDefaults
        let ns_user_defaults_class = match Class::get("NSUserDefaults") {
            Some(cls) => cls,
            None => return false,
        };

        let user_defaults: *mut Object = msg_send![ns_user_defaults_class, standardUserDefaults];
        if user_defaults.is_null() {
            return false;
        }

        // Get "AppleInterfaceStyle" key
        let ns_string_class = match Class::get("NSString") {
            Some(cls) => cls,
            None => return false,
        };

        let key_cstr = match CString::new("AppleInterfaceStyle") {
            Ok(s) => s,
            Err(_) => return false,
        };

        let key: *mut Object = msg_send![ns_string_class, stringWithUTF8String: key_cstr.as_ptr()];
        if key.is_null() {
            return false;
        }

        // Get the value
        let value: *mut Object = msg_send![user_defaults, stringForKey: key];
        if value.is_null() {
            // If nil, system is in light mode
            return false;
        }

        // Check if value is "Dark"
        let dark_cstr = match CString::new("Dark") {
            Ok(s) => s,
            Err(_) => return false,
        };

        let dark_str: *mut Object =
            msg_send![ns_string_class, stringWithUTF8String: dark_cstr.as_ptr()];
        if dark_str.is_null() {
            return false;
        }

        let is_equal: bool = msg_send![value, isEqualToString: dark_str];
        is_equal
    }
}

#[cfg(not(target_os = "macos"))]
fn detect_system_dark_mode() -> bool {
    // Default to light mode on other platforms
    // TODO: Implement for Windows and Linux
    false
}

/// Get the effective theme (resolves Auto mode to actual light/dark)
/// Returns true if dark mode should be used
fn get_effective_dark_mode(theme_mode: i32) -> bool {
    match theme_mode {
        0 => false,                     // Light
        1 => true,                      // Dark
        2 => detect_system_dark_mode(), // Auto - use system preference
        _ => false,                     // Default to light
    }
}

/// Update window appearance when theme changes (native title bar)
#[cfg(target_os = "macos")]
fn set_title_bar_color(window: &MainWindow, is_dark: bool) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use raw_window_handle::HasWindowHandle;
    use std::ffi::CString;

    if let Ok(handle) = window.window().window_handle().window_handle() {
        if let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = handle.as_ref() {
            let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;

            unsafe {
                let ns_window: *mut Object = msg_send![ns_view, window];
                if ns_window.is_null() {
                    return;
                }

                // Set window appearance (light/dark)
                if let Some(ns_appearance_class) = Class::get("NSAppearance") {
                    if let Some(ns_string_class) = Class::get("NSString") {
                        let appearance_name = if is_dark {
                            "NSAppearanceNameDarkAqua"
                        } else {
                            "NSAppearanceNameAqua"
                        };
                        if let Ok(c_str) = CString::new(appearance_name) {
                            let name_obj: *mut Object =
                                msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                            if !name_obj.is_null() {
                                let appearance: *mut Object =
                                    msg_send![ns_appearance_class, appearanceNamed: name_obj];
                                if !appearance.is_null() {
                                    let _: () = msg_send![ns_window, setAppearance: appearance];
                                }
                            }
                        }
                    }
                }

                log::info!("[Theme] Window appearance updated (dark: {})", is_dark);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn set_title_bar_color(_window: &MainWindow, _is_dark: bool) {
    // Other platforms not supported yet
}

fn ui() -> Result<MainWindow> {
    log::info!("Usher v{} starting...", env!("CARGO_PKG_VERSION"));

    // ========= Pre-Window Settings ========= //
    // Load settings BEFORE creating window to set app appearance early (prevents theme flash)
    let loaded_settings = settings::load_settings();

    // Set app-level appearance before window is created (prevents flash on macOS)
    let is_dark = get_effective_dark_mode(loaded_settings.theme_mode);
    set_app_appearance(is_dark);

    // Now create the MainWindow (will inherit app appearance)
    // Using native macOS title bar
    let app = MainWindow::new().context("Failed to create MainWindow")?;

    // Initialize debug logger after window is created (needs app for callbacks)
    setup_debug_logger(&app, true).unwrap_or_else(|e| eprintln!("Debug Logger init failed: {}", e)); // eprintln here since logger not yet initialized

    // ========= Trace Logger Initialization ========= //
    setup_tracing_logger(&app, loaded_settings.tracing_logger_active)
        .unwrap_or_else(|e| eprintln!("Tracing Logger init failed: {}", e));

    // ========= Settings ========= //
    // Apply loaded settings to AppState
    setup_settings(&app, &loaded_settings);
    Router::apply_route_ui_state(&app, app.get_current_route());

    // ========= Debug: Simulate Crash ========= //
    app.on_simulate_crash(|| {
        if let Err(e) = error_handing::do_something_that_might_fail() {
            log::error!("[Debug] simulate crash returned error: {}", e);
        }
    });

    // ========= Update ========= //
    // setup_updater must be called AFTER setup_settings so it reads the correct auto_update value
    let updater_ctx = setup_updater(&app)?;

    // ========= Auto Update ========= //
    // Initialize auto-update based on loaded settings
    if loaded_settings.auto_update {
        start_auto_update(
            app.as_weak(),
            updater_ctx.state.clone(),
            updater_ctx.rt.clone(),
        );
    }

    // Set up auto-update toggle callback (saves settings AND controls auto-update)
    let app_weak_auto = app.as_weak();
    let rt_for_auto = updater_ctx.rt.clone();
    let state_for_auto = updater_ctx.state.clone();
    AppState::get(&app).on_toggle_auto_update(move |enabled| {
        // Save to config file
        settings::save_auto_update(enabled);

        // Control auto-update
        if enabled {
            start_auto_update(
                app_weak_auto.clone(),
                state_for_auto.clone(),
                rt_for_auto.clone(),
            );
        } else {
            stop_auto_update();
        }

        log::info!("[Settings] Auto update changed to: {}", enabled);
    });

    // ========= Tracing Logs Active ========= //
    AppState::get(&app).on_toggle_tracing_logger_active(move |enabled| {
        settings::save_tracing_logger_active(enabled);
        if let Err(e) = tracing_logger::set_tracing_active(enabled) {
            log::error!("[Logger] Failed to toggle tracing logs: {}", e);
        }
        log::info!("[Settings] Tracing logger active changed to: {}", enabled);
    });

    // ========= Navigation Drawer Expanded ========= //
    // Save navigation drawer expanded state when toggled
    app.on_toggle_navigation_drawer_expanded(move |expanded| {
        settings::save_navigation_drawer_expanded(expanded);
        log::info!(
            "[Settings] Navigation drawer expanded changed to: {}",
            expanded
        );
    });

    // ========= Router ========= //
    let router = Router::new(app.as_weak());

    let router_for_nav_index = router.clone();
    let app_for_nav_index = app.as_weak();
    app.on_navigate_from_nav_index(move |item_index| {
        if let Some(route) = route_for_navigation_index(item_index) {
            if let Some(state) = router_for_nav_index.navigate(route) {
                if let Some(ui) = app_for_nav_index.upgrade() {
                    ui.invoke_chat_route_visible_changed(state.chat_route_visible);
                }
            }
        }
    });

    // Connect navigate callback in slint
    let router_clone = router.clone();
    let app_for_chat_route = app.as_weak();
    app.on_navigate(move |route| {
        if let Some(state) = router_clone.navigate(route) {
            if let Some(ui) = app_for_chat_route.upgrade() {
                ui.invoke_chat_route_visible_changed(state.chat_route_visible);
            }
        }
    });

    // ========= Theme ========= //
    let app_weak = app.as_weak();
    AppState::get(&app).on_toggle_theme(move |mode| {
        if let Some(win) = app_weak.upgrade() {
            let app_state = AppState::get(&win);

            // Update UI state
            app_state.set_theme_mode(mode);

            // Save to config file
            settings::save_theme_mode(mode);

            // Get effective dark mode (resolves Auto to actual light/dark)
            let is_dark = get_effective_dark_mode(mode);

            // For Auto mode, set the effective-dark-mode property so Slint can update Palette
            if mode == 2 {
                app_state.set_effective_dark_mode(is_dark);
            }

            // Set title bar color
            set_title_bar_color(&win, is_dark);

            log::info!(
                "[Theme] Changed to mode: {} (effective dark: {})",
                mode,
                is_dark
            );
        }
    });

    // ========= Auto Theme Mode ========= //
    // For Auto mode, set effective-dark-mode so Slint can update Palette
    let initial_theme = loaded_settings.theme_mode;
    if initial_theme == 2 {
        let is_dark = get_effective_dark_mode(initial_theme);
        AppState::get(&app).set_effective_dark_mode(is_dark);
    }

    // ========= App Info ========= //
    // Set application version number
    AppState::get(&app).set_version(env!("CARGO_PKG_VERSION").into());

    // ========= Network ========= //
    setup_network_monitor(&app);

    // ========= Notification ========= //
    // Single window: don't pass shared queue (use None)
    setup_notifications(&app, None);

    // If multi-window application, want to truly share queue:
    // let shared_queue = Arc::new(RwLock::new(SharedQueue::default()));
    // setup_notifications(&window1, Some(shared_queue.clone()));
    // setup_notifications(&window2, Some(shared_queue));

    // ========= Toast ========= //
    setup_toast(&app);

    // ========= Dialog ========= //
    setup_dialog(&app);

    // ========= Chat ========= //
    setup_chat(&app);

    // ========= System Tray ========= //
    // Setup system tray icon with context menu
    // Using native macOS API implementation to avoid Objective-C class registration conflicts
    let app_weak_tray = app.as_weak();
    // Delay tray icon creation to ensure window is fully initialized
    // and to run on main thread (required on macOS)
    Timer::single_shot(std::time::Duration::from_millis(500), move || {
        if let Err(e) = setup_tray(app_weak_tray) {
            log::error!("Failed to setup tray icon: {}", e);
        }
    });

    // Example call - delay execution to ensure window is fully initialized
    // let app_weak = app.as_weak();
    // slint::invoke_from_event_loop(move || {
    //     if let Some(ui) = app_weak.upgrade() {
    //         toast::success("Operation successful!");
    //         toast::show(toast::ToastOptions {
    //             message: "This is a very long message...".to_string(),
    //             duration: 4000,
    //             position: "bottom".to_string(),
    //             overlay: true,
    //             ..Default::default()
    //         });
    //     }
    // }).ok();

    Ok(app)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    let ui: MainWindow = ui().unwrap();
    ui.run().unwrap();
}

// #[cfg(target_os = "android")]
// #[unsafe(no_mangle)]
// fn android_main(android_app: slint::android::AndroidApp) {
//     slint::android::init(android_app).unwrap();
//     let ui = ui();
//     MaterialWindowAdapter::get(&ui).set_disable_hover(true);
//     ui.run().unwrap();
// }
