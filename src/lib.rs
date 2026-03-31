#![allow(unexpected_cfgs)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

use slint::{CloseRequestResponse, ComponentHandle, Timer};

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
use constants::shutdown_priority::PRIORITY_HIGH;

mod chat;
use chat::setup_chat;

mod app_runtime;
mod terminal;

mod logger;
use logger::error_handing;
use logger::tracing_logger;
use logger::{setup_debug_logger, setup_tracing_logger};

mod router;
use router::{Router, route_for_navigation_index};

mod shutdown;
mod single_instance;
mod workflow;
use workflow::setup_workflow;
mod project;
use project::setup_project;
use terminal::setup_terminal;

#[cfg(target_os = "macos")]
static TITLEBAR_SETTINGS_UI_WEAK: std::sync::OnceLock<slint::Weak<MainWindow>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
static mut TITLEBAR_SETTINGS_DELEGATE: *mut objc::runtime::Object = std::ptr::null_mut();

#[cfg(target_os = "macos")]
static mut TITLEBAR_SETTINGS_INSTALLED: bool = false;

#[cfg(target_os = "macos")]
static WINDOW_INTERACTION_MOVE_SEQ: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "macos")]
static WINDOW_IS_INTERACTING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub fn is_window_interacting() -> bool {
    use std::sync::atomic::Ordering;
    WINDOW_IS_INTERACTING.load(Ordering::SeqCst)
}

#[cfg(not(target_os = "macos"))]
pub fn is_window_interacting() -> bool {
    false
}

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

#[cfg(target_os = "macos")]
fn register_titlebar_settings_delegate_class() -> &'static objc::runtime::Class {
    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object, Sel};
    use std::sync::Once;

    static mut DELEGATE_CLASS: *const Class = std::ptr::null();
    static REGISTER: Once = Once::new();

    REGISTER.call_once(|| {
        let superclass = Class::get("NSObject").expect("NSObject class not found");
        let mut decl = ClassDecl::new("UsherTitlebarSettingsDelegate", superclass)
            .expect("failed to declare delegate class");

        extern "C" fn open_settings(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            if let Some(ui_weak) = TITLEBAR_SETTINGS_UI_WEAK.get() {
                let ui_weak_local = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_local.upgrade() {
                        ui.invoke_navigate(Route::Settings);
                    }
                });
            }
        }

        extern "C" fn open_about(_this: &Object, _cmd: Sel, _sender: *mut Object) {
            if let Some(ui_weak) = TITLEBAR_SETTINGS_UI_WEAK.get() {
                let ui_weak_local = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_local.upgrade() {
                        ui.invoke_navigate(Route::About);
                    }
                });
            }
        }

        extern "C" fn window_interaction_begin(
            _this: &Object,
            _cmd: Sel,
            _notification: *mut Object,
        ) {
            use std::sync::atomic::Ordering;
            WINDOW_IS_INTERACTING.store(true, Ordering::SeqCst);
        }

        extern "C" fn window_did_move(_this: &Object, _cmd: Sel, _notification: *mut Object) {
            use std::sync::atomic::Ordering;
            let seq = WINDOW_INTERACTION_MOVE_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = slint::invoke_from_event_loop(move || {
                slint::Timer::single_shot(std::time::Duration::from_millis(180), move || {
                    use std::sync::atomic::Ordering;
                    if WINDOW_INTERACTION_MOVE_SEQ.load(Ordering::SeqCst) != seq {
                        return;
                    }
                    WINDOW_IS_INTERACTING.store(false, Ordering::SeqCst);
                });
            });
        }

        extern "C" fn window_interaction_end(
            _this: &Object,
            _cmd: Sel,
            _notification: *mut Object,
        ) {
            use std::sync::atomic::Ordering;
            WINDOW_IS_INTERACTING.store(false, Ordering::SeqCst);
        }

        unsafe {
            decl.add_method(
                sel!(usherOpenSettings:),
                open_settings as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherOpenAbout:),
                open_about as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherWindowInteractionBegin:),
                window_interaction_begin as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherWindowDidMove:),
                window_did_move as extern "C" fn(&Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(usherWindowInteractionEnd:),
                window_interaction_end as extern "C" fn(&Object, Sel, *mut Object),
            );
            DELEGATE_CLASS = decl.register();
        }
    });

    unsafe { &*DELEGATE_CLASS }
}

#[cfg(target_os = "macos")]
fn install_titlebar_settings_button(window: &MainWindow) {
    use objc::msg_send;
    use objc::runtime::{Class, Object};
    use raw_window_handle::HasWindowHandle;
    use std::ffi::CString;

    #[repr(C)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }

    unsafe fn create_nsstring(s: &str) -> Option<*mut Object> {
        let ns_string_class = Class::get("NSString")?;
        let c_str = CString::new(s).ok()?;
        let ns: *mut Object = msg_send![ns_string_class, stringWithUTF8String: c_str.as_ptr()];
        if ns.is_null() { None } else { Some(ns) }
    }

    let _ = TITLEBAR_SETTINGS_UI_WEAK.set(window.as_weak());

    unsafe {
        if TITLEBAR_SETTINGS_INSTALLED {
            return;
        }

        let window_handle = window.window().window_handle();
        let Ok(handle) = window_handle.window_handle() else {
            return;
        };
        let raw = handle.as_ref();
        let raw_window_handle::RawWindowHandle::AppKit(appkit_handle) = raw else {
            return;
        };

        let ns_view = appkit_handle.ns_view.as_ptr() as *mut Object;
        if ns_view.is_null() {
            log::warn!("[TitleBar] ns_view is null, skip install this round");
            return;
        }
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            log::warn!("[TitleBar] ns_window is null, skip install this round");
            return;
        }

        if TITLEBAR_SETTINGS_DELEGATE.is_null() {
            let delegate_class = register_titlebar_settings_delegate_class();
            let delegate: *mut Object = msg_send![delegate_class, alloc];
            let delegate: *mut Object = msg_send![delegate, init];
            let _: () = msg_send![delegate, retain];
            TITLEBAR_SETTINGS_DELEGATE = delegate;
        }

        // Reuse the existing native delegate to observe window interaction events.
        // During drag/resize, temporarily downgrade Tag loading animation to reduce repaint pressure.
        if let Some(center_class) = Class::get("NSNotificationCenter") {
            let center: *mut Object = msg_send![center_class, defaultCenter];
            if !center.is_null() {
                if let Some(name) = create_nsstring("NSWindowWillMoveNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowInteractionBegin:)
                        name: name
                        object: ns_window
                    ];
                }
                if let Some(name) = create_nsstring("NSWindowDidMoveNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowDidMove:)
                        name: name
                        object: ns_window
                    ];
                }
                if let Some(name) = create_nsstring("NSWindowWillStartLiveResizeNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowInteractionBegin:)
                        name: name
                        object: ns_window
                    ];
                }
                if let Some(name) = create_nsstring("NSWindowDidEndLiveResizeNotification") {
                    let _: () = msg_send![
                        center,
                        addObserver: TITLEBAR_SETTINGS_DELEGATE
                        selector: sel!(usherWindowInteractionEnd:)
                        name: name
                        object: ns_window
                    ];
                }
            }
        }

        let accessory_class = match Class::get("NSTitlebarAccessoryViewController") {
            Some(c) => c,
            None => return,
        };
        let view_class = match Class::get("NSView") {
            Some(c) => c,
            None => return,
        };
        let button_class = match Class::get("NSButton") {
            Some(c) => c,
            None => return,
        };

        let accessory: *mut Object = msg_send![accessory_class, alloc];
        let accessory: *mut Object = msg_send![accessory, init];
        if accessory.is_null() {
            return;
        }

        let container_frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: 104.0,
                height: 32.0,
            },
        };
        let container: *mut Object = msg_send![view_class, alloc];
        let container: *mut Object = msg_send![container, initWithFrame: container_frame];
        if container.is_null() {
            return;
        }

        let settings_button_frame = NSRect {
            // Keep super-large icon button vertically centered in the titlebar.
            // y = (container_h - button_h) / 2 = (32 - 36) / 2 = -2
            origin: NSPoint { x: 20.0, y: -2.0 },
            size: NSSize {
                width: 48.0,
                height: 36.0,
            },
        };
        let settings_button: *mut Object = msg_send![button_class, alloc];
        let settings_button: *mut Object =
            msg_send![settings_button, initWithFrame: settings_button_frame];
        if settings_button.is_null() {
            return;
        }

        let about_button_frame = NSRect {
            origin: NSPoint { x: 52.0, y: -1.0 },
            size: NSSize {
                width: 48.0,
                height: 36.0,
            },
        };
        let about_button: *mut Object = msg_send![button_class, alloc];
        let about_button: *mut Object = msg_send![about_button, initWithFrame: about_button_frame];
        if about_button.is_null() {
            return;
        }

        // Prefer native gear-style images.
        // If unavailable on current macOS runtime, fallback to a gear glyph ("⚙︎")
        // so the control is always icon-like instead of text-like.
        let ns_image_class = Class::get("NSImage");
        if let Some(ns_image_class) = ns_image_class {
            let settings_symbol_candidates = [
                "NSImageNamePreferencesGeneral",
                "NSImageNameAdvanced",
                "NSImageNameActionTemplate",
            ];
            let mut settings_image_set = false;
            for name in settings_symbol_candidates {
                if let Some(symbol_name) = create_nsstring(name) {
                    let image: *mut Object = msg_send![ns_image_class, imageNamed: symbol_name];
                    if !image.is_null() {
                        let _: () = msg_send![settings_button, setImage: image];
                        if let Some(empty) = create_nsstring("") {
                            let _: () = msg_send![settings_button, setTitle: empty];
                        }
                        // NSImageOnly
                        let _: () = msg_send![settings_button, setImagePosition: 2u64];
                        // NSImageScaleProportionallyUpOrDown
                        let _: () = msg_send![settings_button, setImageScaling: 3u64];
                        settings_image_set = true;
                        break;
                    }
                }
            }
            if !settings_image_set {
                if let Some(gear) = create_nsstring("⚙︎") {
                    let _: () = msg_send![settings_button, setTitle: gear];
                }
            }
            let about_symbol_candidates = ["NSImageNameInfo", "NSImageNameActionTemplate"];
            let mut about_image_set = false;
            for name in about_symbol_candidates {
                if let Some(symbol_name) = create_nsstring(name) {
                    let image: *mut Object = msg_send![ns_image_class, imageNamed: symbol_name];
                    if !image.is_null() {
                        let _: () = msg_send![about_button, setImage: image];
                        if let Some(empty) = create_nsstring("") {
                            let _: () = msg_send![about_button, setTitle: empty];
                        }
                        let _: () = msg_send![about_button, setImagePosition: 2u64];
                        let _: () = msg_send![about_button, setImageScaling: 3u64];
                        about_image_set = true;
                        break;
                    }
                }
            }
            if !about_image_set {
                if let Some(info) = create_nsstring("ℹ︎") {
                    let _: () = msg_send![about_button, setTitle: info];
                }
            }
        } else {
            if let Some(gear) = create_nsstring("⚙︎") {
                let _: () = msg_send![settings_button, setTitle: gear];
            }
            if let Some(info) = create_nsstring("ℹ︎") {
                let _: () = msg_send![about_button, setTitle: info];
            }
        }

        // Increase fallback glyph readability for compact icon button.
        if let Some(ns_font_class) = Class::get("NSFont") {
            let font: *mut Object = msg_send![ns_font_class, systemFontOfSize: 24.0f64];
            if !font.is_null() {
                let _: () = msg_send![settings_button, setFont: font];
                let _: () = msg_send![about_button, setFont: font];
            }
        }

        // True icon-button style: remove default capsule background/border.
        let _: () = msg_send![settings_button, setBordered: false];
        let _: () = msg_send![about_button, setBordered: false];
        if let Some(tooltip) = create_nsstring("Settings") {
            let _: () = msg_send![settings_button, setToolTip: tooltip];
        }
        if let Some(tooltip) = create_nsstring("About") {
            let _: () = msg_send![about_button, setToolTip: tooltip];
        }
        let _: () = msg_send![settings_button, setTarget: TITLEBAR_SETTINGS_DELEGATE];
        let _: () = msg_send![settings_button, setAction: sel!(usherOpenSettings:)];
        let _: () = msg_send![about_button, setTarget: TITLEBAR_SETTINGS_DELEGATE];
        let _: () = msg_send![about_button, setAction: sel!(usherOpenAbout:)];

        let _: () = msg_send![container, addSubview: settings_button];
        let _: () = msg_send![container, addSubview: about_button];
        let _: () = msg_send![accessory, setView: container];
        // NSTitlebarAccessoryLayoutAttributeRight
        // Different SDK headers may map raw values differently; `2` works as right-side in current target.
        let _: () = msg_send![accessory, setLayoutAttribute: 2u64];
        let _: () = msg_send![ns_window, addTitlebarAccessoryViewController: accessory];

        TITLEBAR_SETTINGS_INSTALLED = true;
        log::info!("[TitleBar] Added native Settings/About buttons to macOS title bar");
    }
}

#[cfg(not(target_os = "macos"))]
fn install_titlebar_settings_button(_window: &MainWindow) {}

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

    // Intercept title-bar close button: hide window instead of terminating process.
    app.window().on_close_requested(|| {
        log::info!("[Window] Close button clicked, hiding window to tray");
        CloseRequestResponse::HideWindow
    });

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

    // ========= Git Tokens ========= //
    let app_weak_git_tokens = app.as_weak();
    AppState::get(&app).on_save_git_tokens(move |github_token, gitlab_token, gitee_token| {
        settings::save_git_platform_tokens(
            github_token.as_str(),
            gitlab_token.as_str(),
            gitee_token.as_str(),
        );
        if let Some(win) = app_weak_git_tokens.upgrade() {
            win.set_settings_git_tokens_saved(true);
            let win_weak = win.as_weak();
            Timer::single_shot(std::time::Duration::from_secs(2), move || {
                if let Some(win) = win_weak.upgrade() {
                    win.set_settings_git_tokens_saved(false);
                }
            });
        }
        log::info!(
            "[Settings] Git tokens updated: github={}, gitlab={}, gitee={}",
            if github_token.is_empty() {
                "empty"
            } else {
                "set"
            },
            if gitlab_token.is_empty() {
                "empty"
            } else {
                "set"
            },
            if gitee_token.is_empty() {
                "empty"
            } else {
                "set"
            }
        );
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

    // ========= Project ========= //
    setup_project(&app);

    // ========= Chat ========= //
    setup_chat(&app);

    // ========= Terminal Drawer ========= //
    setup_terminal(&app);

    // ========= Workflow ========= //
    setup_workflow(&app);

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

    shutdown::init_signal_handlers();
    shutdown::register_pre_shutdown_task_with_priority("stop_auto_update", PRIORITY_HIGH, || {
        stop_auto_update();
    });
    shutdown::register_ui_shutdown_task_with_priority("remove_tray", PRIORITY_HIGH, || {
        tray::remove_tray();
    });

    Ok(app)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn main() {
    match single_instance::acquire_instance_lock() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("[Usher] Another instance is already running, exiting this process.");
            return;
        }
        Err(e) => {
            eprintln!(
                "[Usher] Failed to acquire instance lock (continuing anyway): {}",
                e
            );
        }
    }

    let ui: MainWindow = ui().unwrap();
    ui.show().unwrap();
    install_titlebar_settings_button(&ui);
    let ui_weak_titlebar_retry_1 = ui.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(300), move || {
        if let Some(win) = ui_weak_titlebar_retry_1.upgrade() {
            install_titlebar_settings_button(&win);
        }
    });
    let ui_weak_titlebar_retry_2 = ui.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(900), move || {
        if let Some(win) = ui_weak_titlebar_retry_2.upgrade() {
            install_titlebar_settings_button(&win);
        }
    });
    slint::run_event_loop_until_quit().unwrap();
    shutdown::run_cleanup_once("event-loop-exit");
    drop(ui);
    log::info!("[Shutdown] MainWindow dropped, native resources released");
}

// #[cfg(target_os = "android")]
// #[unsafe(no_mangle)]
// fn android_main(android_app: slint::android::AndroidApp) {
//     slint::android::init(android_app).unwrap();
//     let ui = ui();
//     MaterialWindowAdapter::get(&ui).set_disable_hover(true);
//     ui.run().unwrap();
// }
