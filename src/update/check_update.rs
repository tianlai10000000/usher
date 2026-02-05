use super::updater::{UpdateState, check_for_update};
use self_update::cargo_crate_version;
use slint::{ComponentHandle, Global, ModelRc, SharedString, Timer, TimerMode, VecModel, Weak};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

use crate::{CustomComponentType, NotificationManager, NotificationOptions, UpdateLogic};

// Thread-local timer for auto-update (Timer is not thread-safe)
thread_local! {
    static AUTO_UPDATE_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

/// Perform an immediate background update check (non-blocking)
/// This is called on app startup when auto-update is enabled
fn perform_startup_check(
    ui_weak: Weak<crate::MainWindow>,
    state: Arc<Mutex<UpdateState>>,
    rt: Arc<Runtime>,
) {
    let state = state.clone();
    let ui_weak = ui_weak.clone();

    // Set checking status before starting (show in status bar)
    let ui_weak_for_status = ui_weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak_for_status.upgrade() {
            let logic = ui.global::<UpdateLogic>();
            logic.set_checking_update(true);
        }
    })
    .ok();

    rt.spawn(async move {
        log::info!("[AUTO-UPDATE] Performing startup update check...");

        let result = check_for_update(&state).await;

        // Reset checking status after check completes
        let ui_weak_for_reset = ui_weak.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak_for_reset.upgrade() {
                let logic = ui.global::<UpdateLogic>();
                logic.set_checking_update(false);
            }
        })
        .ok();

        match result {
            Ok(is_up_to_date) => {
                if !is_up_to_date {
                    // New version available - show update dialog
                    let s = state.lock().unwrap();
                    let new_version = s.version.clone();
                    drop(s);

                    let current_version = cargo_crate_version!().to_string();
                    log::info!(
                        "[AUTO-UPDATE] Update available: {} -> {}",
                        current_version, new_version
                    );

                    let ui_weak_for_notification = ui_weak.clone();
                    let new_version_clone = new_version.clone();
                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_for_notification.upgrade() {
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_update_available(true);
                            logic.set_new_version(new_version_clone.clone().into());

                            // Show update notification with Cancel and Update Now buttons
                            // Using ActionsNotify component template
                            let actions: ModelRc<SharedString> =
                                ModelRc::new(Rc::new(VecModel::from(vec![
                                    SharedString::from("Cancel"),
                                    SharedString::from("Update Now"),
                                ])));

                            let manager = NotificationManager::get(&ui);
                            manager.invoke_info(NotificationOptions {
                                message: format!(
                                    "New version {} is available (current: {})",
                                    new_version_clone, current_version
                                )
                                .into(),
                                title: "Update Available".into(),
                                duration: 0, // Permanent until user clicks
                                icon: "".into(),
                                component: CustomComponentType::ActionsNotify,
                                actions,
                                tag: "auto-update".into(),
                            });
                        }
                    })
                    .ok();
                } else {
                    log::info!("[AUTO-UPDATE] Already up to date");
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                log::info!("[AUTO-UPDATE] Startup check failed: {}", error_msg);

                // Show error notification to user
                let friendly_msg = if error_msg.contains("404") || error_msg.contains("Not Found") {
                    "Repository not found or no releases available. Please check if the repository exists.".to_string()
                } else if error_msg.contains("NetworkError") || error_msg.contains("network") 
                    || error_msg.contains("ReqwestError") || error_msg.contains("error sending request") {
                    "Network connection failed. Please check your network and try again.".to_string()
                } else {
                    format!("Update check failed: {}", error_msg)
                };

                let ui_weak_for_error = ui_weak.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak_for_error.upgrade() {
                        let manager = NotificationManager::get(&ui);
                        manager.invoke_error(NotificationOptions {
                            message: friendly_msg.into(),
                            title: "Update Check Failed".into(),
                            duration: 5000,
                            icon: "".into(),
                            component: CustomComponentType::None,
                            actions: Default::default(),
                            tag: Default::default(),
                        });
                    }
                }).ok();
            }
        }
    });
}

/// Start auto-update timer (check for updates every hour)
/// Also performs an immediate check on startup
///
/// Call this when user enables auto-update in Settings
pub fn start_auto_update(
    ui_weak: Weak<crate::MainWindow>,
    state: Arc<Mutex<UpdateState>>,
    rt: Arc<Runtime>,
) {
    // Stop existing timer first
    stop_auto_update();

    // Perform immediate startup check (non-blocking)
    perform_startup_check(ui_weak.clone(), state.clone(), rt.clone());

    // Create timer for periodic checks (every hour)
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, std::time::Duration::from_secs(3600), {
        let state = state.clone();
        let ui_weak = ui_weak.clone();
        let rt = rt.clone();
        move || {
            let state = state.clone();
            let ui_weak = ui_weak.clone();
            let current_version = cargo_crate_version!().to_string();

            rt.spawn(async move {
                match check_for_update(&state).await {
                    Ok(is_up_to_date) => {
                        // Notify UI when update is available
                        if !is_up_to_date {
                            let s = state.lock().unwrap();
                            let new_version = s.version.clone();
                            drop(s);

                            slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak.upgrade() {
                                    let logic = ui.global::<UpdateLogic>();
                                    logic.set_update_available(true);
                                    logic.set_new_version(new_version.clone().into());

                                    // Show update available dialog
                                    ui.invoke_show_update_dialog(
                                        new_version.into(),
                                        current_version.into(),
                                    );
                                }
                            })
                            .ok();
                        }
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        log::info!("[AUTO-UPDATE] Periodic check failed: {}", error_msg);

                        // Store error in state
                        {
                            let mut s = state.lock().unwrap();
                            s.error = error_msg.clone();
                        }

                        // Show error notification to user
                        let friendly_msg =
                            if error_msg.contains("404") || error_msg.contains("Not Found") {
                                "Repository not found or no releases available.".to_string()
                            } else if error_msg.contains("NetworkError")
                                || error_msg.contains("network")
                                || error_msg.contains("ReqwestError")
                                || error_msg.contains("error sending request")
                            {
                                "Network connection failed. Please check your network.".to_string()
                            } else {
                                format!("Update check failed: {}", error_msg)
                            };

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let manager = NotificationManager::get(&ui);
                                manager.invoke_error(NotificationOptions {
                                    message: friendly_msg.into(),
                                    title: "Update Check Failed".into(),
                                    duration: 5000,
                                    icon: "".into(),
                                    component: CustomComponentType::None,
                                    actions: Default::default(),
                                    tag: Default::default(),
                                });
                            }
                        })
                        .ok();
                    }
                }
            });
        }
    });

    // Store timer to keep it alive
    AUTO_UPDATE_TIMER.with(|t| {
        *t.borrow_mut() = Some(timer);
    });

    log::info!("[AUTO-UPDATE] Started auto-update (immediate check + hourly timer)");
}

/// Stop auto-update timer
///
/// Call this when user disables auto-update in Settings
pub fn stop_auto_update() {
    AUTO_UPDATE_TIMER.with(|t| {
        let mut timer_opt = t.borrow_mut();
        if timer_opt.is_some() {
            *timer_opt = None;
            log::info!("[AUTO-UPDATE] Stopped auto-update timer");
        }
    });
}

/// Check if auto-update is currently active
#[allow(dead_code)]
pub fn is_auto_update_active() -> bool {
    AUTO_UPDATE_TIMER.with(|t| t.borrow().is_some())
}
