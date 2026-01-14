#![allow(unexpected_cfgs)]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

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

use self_update::cargo_crate_version;
use std::sync::atomic::{AtomicBool, Ordering};
use update::check_update::init;
use update::updater::{
    CancelFlag, UpdateState, check_for_update, perform_update_with_progress, reset_update_state,
};

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

    // Update logic
    let state = Arc::new(Mutex::new(UpdateState::default()));
    let rt = Arc::new(Runtime::new().context("Failed to create Tokio runtime")?);
    let app_weak = app.as_weak();
    // Initialize update check (scheduled + notification logic)
    init(app_weak.clone(), state.clone(), rt.clone());

    // Manual update check
    let state_clone_check = state.clone();
    let app_weak_clone_check = app_weak.clone();
    let rt_clone_check = rt.clone();
    app.on_check_update(move || {
        println!("[DEBUG] check-update callback called");
        let state = state_clone_check.clone();
        let ui_weak = app_weak_clone_check.clone();
        let rt = rt_clone_check.clone();
        rt.spawn(async move {
            println!("[DEBUG] Starting update check...");
            // Reset state
            {
                let mut s = state.lock().unwrap();
                s.available = false;
                s.version.clear();
                s.error.clear();
                s.progress = 0.0;
            }

            println!("[DEBUG] Calling check_for_update...");
            let result = check_for_update(&state).await;
            println!("[DEBUG] check_for_update completed, result: {:?}", result.is_ok());
            let (is_up_to_date, version) = {
                match &result {
                    Ok(up_to_date) => {
                        let v = if !*up_to_date {
                            let s = state.lock().unwrap();
                            s.version.clone()
                        } else {
                            String::new()
                        };
                        println!("[DEBUG] is_up_to_date: {}, version: {}", *up_to_date, v);
                        (*up_to_date, v)
                    }
                    Err(_) => (false, String::new()),
                }
            };

            match result {
                Ok(_) => {
                    let version_clone = version.clone();
                    println!("[DEBUG] Processing result: is_up_to_date={}, version={}", is_up_to_date, version_clone);

                    if is_up_to_date {
                        // Current version is the latest
                        let ui_weak_for_notification = ui_weak.clone();
                        let ui_weak_for_logic = ui_weak.clone();
                        let ui_weak_for_reset = ui_weak.clone();

                        // Call notification first
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_notification.upgrade() {
                                {
                                    let manager = NotificationManager::get(&ui);
                                    manager.invoke_success(NotificationOptions {
                                        message: "Current program is already the latest version".into(),
                                        title: "".into(),
                                        duration: 3000,
                                        icon: "".into(),
                                        component: CustomComponentType::None,
                                    });
                                } // manager borrow released here
                            }
                        }).ok();

                        // Use Timer to delay logic reset, ensure execution in next event loop iteration
                        Timer::single_shot(std::time::Duration::from_millis(100), move || {
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak_for_logic.upgrade() {
                                    {
                                        let logic = ui.global::<UpdateLogic>();
                                        logic.set_error("".into());
                                        logic.set_update_available(false);
                                    } // logic borrow released here
                                }
                            }).ok();
                        });

                        // Immediately reset button state to ensure user can see button restored
                        println!("[DEBUG] Preparing to reset button state (already latest version)");
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_reset.upgrade() {
                                println!("[DEBUG] Resetting checking-update state (already latest version)");
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_checking_update(false);
                            } else {
                                println!("[DEBUG] ui_weak_for_reset.upgrade() failed (already latest version)");
                            }
                        }).ok();
                    } else {
                        println!("[DEBUG] Entering branch with new version available");
                        // New version available
                        let ui_weak_for_all = ui_weak.clone();
                        let version_for_all = version.clone();

                        // Set state
                        let ui_weak_for_state = ui_weak_for_all.clone();
                        let version_for_state = version_for_all.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_state.upgrade() {
                                println!("[DEBUG] Setting state");
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_update_available(true);
                                logic.set_new_version(version_for_state.clone().into());
                                logic.set_checking_update(false);
                                println!("[DEBUG] Set update-available=true, new-version={}", version_for_state);
                            }
                        }).ok();

                        // Delay showing dialog to avoid RefCell borrow conflict
                        // Use tokio sleep instead of Slint Timer (Slint Timer cannot trigger correctly in async tasks)
                        println!("[DEBUG] Preparing to delay showing dialog...");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        println!("[DEBUG] Delay completed, showing dialog");

                        let ui_weak_for_dialog = ui_weak_for_all.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_dialog.upgrade() {
                                println!("[DEBUG] Calling MainWindow.invoke_show_update_dialog()");
                                let current_version = cargo_crate_version!();
                                ui.invoke_show_update_dialog(
                                    version_for_all.clone().into(),
                                    current_version.into()
                                );
                                println!("[DEBUG] MainWindow.invoke_show_update_dialog() called");
                            } else {
                                println!("[DEBUG] ui_weak_for_dialog.upgrade() failed");
                            }
                        }).ok();
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    println!("[DEBUG] Update check error: {}", error_msg);
                    // Provide more user-friendly error message
                    let friendly_msg = if error_msg.contains("404") || error_msg.contains("Not Found") {
                        format!("Repository not found or no available release versions. Please check if the repository exists and has created releases, failure reason: {}", error_msg)
                    } else if error_msg.contains("NetworkError") || error_msg.contains("network") 
                        || error_msg.contains("ReqwestError") || error_msg.contains("error sending request") {
                        format!("Network connection failed, please check your network connection and retry, failure reason: {}", error_msg)
                    } else {
                        format!("Unknown error, error reason: {}", error_msg)
                    };
                    let ui_weak_for_error = ui_weak.clone();
                    let ui_weak_for_logic = ui_weak.clone();
                    let ui_weak_for_reset = ui_weak.clone();

                    // Show error notification
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_for_error.upgrade() {
                            {
                                let manager = NotificationManager::get(&ui);
                                manager.invoke_error(NotificationOptions {
                                    message: friendly_msg.clone().into(),
                                    title: "Update check failed".into(),
                                    duration: 5000,
                                    icon: "".into(),
                                    component: CustomComponentType::None,
                                });
                            } // manager borrow released here, avoid conflict with borrow in callback
                        }
                    }).ok();

                    // Use Timer to delay logic reset, ensure execution in next event loop iteration
                    Timer::single_shot(std::time::Duration::from_millis(100), move || {
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_logic.upgrade() {
                                {
                                    let logic = ui.global::<UpdateLogic>();
                                    logic.set_error("".into());
                                    logic.set_update_available(false);
                                } // logic borrow released here
                            }
                        }).ok();
                    });

                    // Immediately reset button state to ensure user can see button restored
                    println!("[DEBUG] Preparing to reset button state (check failed)");
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_for_reset.upgrade() {
                            println!("[DEBUG] Resetting checking-update state (check failed)");
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_checking_update(false);
                        } else {
                            println!("[DEBUG] ui_weak_for_reset.upgrade() failed (check failed)");
                        }
                    }).ok();
                }
            }
        });
    });

    // Global cancel flag
    let cancel_flag: CancelFlag = Arc::new(AtomicBool::new(false));

    // User clicks "Update Now" button
    let state_clone = state.clone();
    let app_weak_clone = app_weak.clone();
    let rt_clone = rt.clone();
    let cancel_flag_clone = cancel_flag.clone();
    app.on_install(move || {
        println!("[DEBUG] on_install callback called");
        let state = state_clone.clone();
        let ui_weak = app_weak_clone.clone();
        let rt = rt_clone.clone();
        let cancel_flag = cancel_flag_clone.clone();

        // Reset cancel flag
        cancel_flag.store(false, Ordering::Relaxed);

        // Use Timer to delay all operations, avoid RefCell borrow conflict
        Timer::single_shot(std::time::Duration::from_millis(100), move || {
            println!("[DEBUG] Timer callback executed, preparing to show download progress modal");

            // Show download progress modal
            if let Some(ui) = ui_weak.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_is_downloading(true);
                logic.set_download_progress(0.0);
                logic.set_download_status("Preparing...".into());
                println!("[DEBUG] Download progress modal shown");
            }

            // Create progress callback
            let ui_weak_for_progress = ui_weak.clone();
            let progress_callback = Arc::new(
                move |progress: f32, downloaded: u64, total: u64, status: &str| {
                    let ui_weak = ui_weak_for_progress.clone();
                    let status_str = status.to_string();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_download_progress(progress);
                            logic.set_download_status(status_str.into());
                            logic.set_downloaded_bytes(downloaded as i32);
                            logic.set_total_bytes(total as i32);
                        }
                    })
                    .ok();
                },
            );

            let ui_weak_for_error = ui_weak.clone();
            rt.spawn(async move {
                println!("[DEBUG] Starting update with progress...");
                let update_result =
                    perform_update_with_progress(&state, progress_callback, cancel_flag.clone())
                        .await;

                match &update_result {
                    Ok(_) => {
                        println!("[DEBUG] Update successful, program will restart");
                        // After successful update, program will automatically restart, no additional handling needed here
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        println!("[DEBUG] Update failed: {}", error_msg);

                        // Reset state
                        reset_update_state(&state);

                        let is_cancelled = error_msg.contains("cancelled");
                        let ui_weak_for_error = ui_weak_for_error.clone();

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak_for_error.upgrade() {
                                // Hide download progress modal
                                let logic = ui.global::<UpdateLogic>();
                                logic.set_is_downloading(false);
                                logic.set_download_progress(0.0);
                                logic.set_download_status("".into());

                                if !is_cancelled {
                                    // Only show error notification for non-cancelled cases
                                    let manager = NotificationManager::get(&ui);
                                    manager.invoke_error(NotificationOptions {
                                        message: format!("Update failed: {}", error_msg).into(),
                                        title: "".into(),
                                        duration: 5000,
                                        icon: "".into(),
                                        component: CustomComponentType::None,
                                    });
                                }
                            }
                        })
                        .ok();
                    }
                }
            });
        });
    });

    // User clicks cancel download button
    let cancel_flag_clone = cancel_flag.clone();
    let state_clone = state.clone();
    let app_weak_clone = app_weak.clone();
    app.on_cancel_download(move || {
        println!("[DEBUG] on_cancel_download callback called");

        // Set cancel flag
        cancel_flag_clone.store(true, Ordering::Relaxed);

        // Reset state
        reset_update_state(&state_clone);

        // Use Timer to delay UI update, avoid RefCell borrow conflict
        let ui_weak = app_weak_clone.clone();
        Timer::single_shot(std::time::Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_is_downloading(false);
                logic.set_download_progress(0.0);
                logic.set_download_status("".into());
                logic.set_update_available(false);

                // Show cancel notification
                let manager = NotificationManager::get(&ui);
                manager.invoke_info(NotificationOptions {
                    message: "Update cancelled".into(),
                    title: "".into(),
                    duration: 3000,
                    icon: "".into(),
                    component: CustomComponentType::None,
                });
            }
        });
    });

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
