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
use update::setup_updater;

mod network;
use network::setup_network_monitor;

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
    let app = MainWindow::new().context("Failed to create MainWindow")?;

    // ========= Update ========= //
    setup_updater(&app)?;

    // ========= Router ========= //
    let router = Router::new(app.as_weak());

    // Connect navigate callback in slint
    let router_clone = router.clone();
    app.on_navigate(move |route| {
        router_clone.navigate(route);
    });

    // Optional: default to a page on program startup (already set by .slint default)

    // Optional: example - auto navigate after 3 seconds (for demo)
    // let router_clone = router.clone();
    // slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
    //     router_clone.navigate(Route::Profile);
    // });

    // ========= Theme ========= //
    let app_weak = app.as_weak();
    AppState::get(&app).on_toggle_theme(move |_mode| {
        if let Some(win) = app_weak.upgrade() {
            let app_state = AppState::get(&win);
            let current = app_state.get_theme_mode();
            let new_mode = if current == 0 { 1 } else { 0 };
            app_state.set_theme_mode(new_mode);
            // Theme switching logic is handled in app_state.slint's changed theme-mode callback

            // Set title bar color
            set_title_bar_color(&win, new_mode == 1);
        }
    });

    // ========= Title Bar ========= //
    // Set title bar color based on current theme on initialization
    let initial_theme = AppState::get(&app).get_theme_mode();
    set_title_bar_color(&app, initial_theme == 1);

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
