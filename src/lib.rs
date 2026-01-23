#![allow(unexpected_cfgs)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::{Timer, Weak};

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
                    println!(
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

#[cfg(target_os = "macos")]
fn set_title_bar_color(_window: &MainWindow, is_dark: bool) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use std::ffi::CString;
    use std::ptr;

    // Use delayed execution to ensure window is created and execute on main thread
    slint::invoke_from_event_loop(move || {
        unsafe {
            // Get application's main window
            let ns_app_class = match Class::get("NSApplication") {
                Some(cls) => cls,
                None => return,
            };

            let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
            if ns_app.is_null() {
                return;
            }

            let ns_window: *mut Object = msg_send![ns_app, mainWindow];
            if ns_window.is_null() {
                return;
            }

            if is_dark {
                // Set title bar to dark mode
                // Make title bar transparent so background color shows through
                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: objc::runtime::YES];

                // Set window background to Material Design dark theme background color rgb(17, 19, 24)
                if let Some(ns_color_class) = Class::get("NSColor") {
                    // Use colorWithRed:green:blue:alpha: to create color
                    // Convert RGB(17, 19, 24) to 0-1 range: (17/255.0, 19/255.0, 24/255.0)
                    let r = 17.0 / 255.0;
                    let g = 19.0 / 255.0;
                    let b = 24.0 / 255.0;
                    let dark_bg_color: *mut Object = msg_send![ns_color_class, colorWithRed: r green: g blue: b alpha: 1.0];
                    let _: () = msg_send![ns_window, setBackgroundColor: dark_bg_color];
                }

                // Set window appearance to dark mode so title text becomes white
                if let Some(ns_appearance_class) = Class::get("NSAppearance") {
                    if let Some(ns_string_class) = Class::get("NSString") {
                        // Create NSString "NSAppearanceNameDarkAqua"
                        if let Ok(c_str) = CString::new("NSAppearanceNameDarkAqua") {
                            let appearance_name: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                            if !appearance_name.is_null() {
                                let dark_appearance: *mut Object = msg_send![ns_appearance_class, appearanceNamed: appearance_name];
                                if !dark_appearance.is_null() {
                                    let _: () = msg_send![ns_window, setAppearance: dark_appearance];
                                }
                            }
                        }
                    }
                }
            } else {
                // Restore light mode
                let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: objc::runtime::NO];

                // Restore default background color (nil means use system default)
                let _: () = msg_send![ns_window, setBackgroundColor: ptr::null_mut::<Object>()];

                // Set window appearance to light mode
                if let Some(ns_appearance_class) = Class::get("NSAppearance") {
                    if let Some(ns_string_class) = Class::get("NSString") {
                        // Create NSString "NSAppearanceNameAqua"
                        if let Ok(c_str) = CString::new("NSAppearanceNameAqua") {
                            let appearance_name: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
                            if !appearance_name.is_null() {
                                let light_appearance: *mut Object = msg_send![ns_appearance_class, appearanceNamed: appearance_name];
                                if !light_appearance.is_null() {
                                    let _: () = msg_send![ns_window, setAppearance: light_appearance];
                                }
                            }
                        }
                    }
                }
            }
        }
    }).ok();
}

#[cfg(not(target_os = "macos"))]
fn set_title_bar_color(_window: &MainWindow, _is_dark: bool) {
    // Other platforms not supported yet
}

#[derive(Clone)]
struct Router {
    window: Weak<MainWindow>,
}

impl Router {
    fn new(window: Weak<MainWindow>) -> Self {
        Self { window }
    }

    fn navigate(&self, route: Route) {
        if let Some(win) = self.window.upgrade() {
            win.set_current_route(route);
            // Can add other global side effects here, such as:
            // - Record route history
            // - Send analytics events
            // - Lazy load certain resources, etc.
            println!("→ Navigate to: {:?}", route);
        }
    }
}

fn ui() -> Result<MainWindow> {
    // ========= Pre-Window Settings ========= //
    // Load settings BEFORE creating window to set app appearance early (prevents theme flash)
    let loaded_settings = settings::load_settings();

    // Set app-level appearance before window is created (prevents flash on macOS)
    let is_dark = get_effective_dark_mode(loaded_settings.theme_mode);
    set_app_appearance(is_dark);

    // Now create the MainWindow (will inherit app appearance)
    let app = MainWindow::new().context("Failed to create MainWindow")?;

    // ========= Settings ========= //
    // Apply loaded settings to AppState
    setup_settings(&app, &loaded_settings);

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

        println!("[Settings] Auto update changed to: {}", enabled);
    });

    // ========= Router ========= //
    let router = Router::new(app.as_weak());

    // Connect navigate callback in slint
    let router_clone = router.clone();
    app.on_navigate(move |route| {
        router_clone.navigate(route);
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

            println!(
                "[Theme] Changed to mode: {} (effective dark: {})",
                mode, is_dark
            );
        }
    });

    // ========= Title Bar Customization ========= //
    // App appearance is already set before window creation (no flash)
    // Now set title bar-specific customizations (transparent title bar, custom background)
    // Use minimal delay to ensure window is ready
    let initial_theme = loaded_settings.theme_mode;
    let app_weak_init = app.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(10), move || {
        // Get effective dark mode (resolves Auto to actual light/dark)
        let is_dark = get_effective_dark_mode(initial_theme);

        // For Auto mode, set effective-dark-mode so Slint can update Palette
        if initial_theme == 2 {
            if let Some(win) = app_weak_init.upgrade() {
                AppState::get(&win).set_effective_dark_mode(is_dark);
            }
        }

        // Set title bar customizations (transparent, custom background color)
        #[cfg(target_os = "macos")]
        slint::invoke_from_event_loop(move || {
            use objc::msg_send;
            use objc::runtime::{Class, Object};
            use std::ptr;

            unsafe {
                let ns_app_class = match Class::get("NSApplication") {
                    Some(cls) => cls,
                    None => return,
                };

                let ns_app: *mut Object = msg_send![ns_app_class, sharedApplication];
                if ns_app.is_null() {
                    return;
                }

                let ns_window: *mut Object = msg_send![ns_app, mainWindow];
                if ns_window.is_null() {
                    return;
                }

                if is_dark {
                    // Make title bar transparent and set custom background color
                    let _: () =
                        msg_send![ns_window, setTitlebarAppearsTransparent: objc::runtime::YES];

                    if let Some(ns_color_class) = Class::get("NSColor") {
                        let r = 17.0 / 255.0;
                        let g = 19.0 / 255.0;
                        let b = 24.0 / 255.0;
                        let dark_bg_color: *mut Object =
                            msg_send![ns_color_class, colorWithRed: r green: g blue: b alpha: 1.0];
                        let _: () = msg_send![ns_window, setBackgroundColor: dark_bg_color];
                    }
                    println!(
                        "[Theme] Initial title bar: dark (auto: {})",
                        initial_theme == 2
                    );
                } else {
                    // Reset to default title bar
                    let _: () =
                        msg_send![ns_window, setTitlebarAppearsTransparent: objc::runtime::NO];
                    let _: () = msg_send![ns_window, setBackgroundColor: ptr::null_mut::<Object>()];
                    println!(
                        "[Theme] Initial title bar: light (auto: {})",
                        initial_theme == 2
                    );
                }
            }
        })
        .ok();
    });

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

    // ========= System Tray ========= //
    // Setup system tray icon with context menu
    // Using native macOS API implementation to avoid Objective-C class registration conflicts
    let app_weak_tray = app.as_weak();
    // Delay tray icon creation to ensure window is fully initialized
    // and to run on main thread (required on macOS)
    Timer::single_shot(std::time::Duration::from_millis(500), move || {
        if let Err(e) = setup_tray(app_weak_tray) {
            eprintln!("Failed to setup tray icon: {}", e);
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
