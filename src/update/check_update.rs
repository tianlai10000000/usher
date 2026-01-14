use super::updater::{UpdateState, check_for_update};
use slint::{ComponentHandle, Timer, TimerMode, Weak};
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

pub fn init(ui_weak: Weak<crate::MainWindow>, state: Arc<Mutex<UpdateState>>, rt: Arc<Runtime>) {
    // Check for updates every hour
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, std::time::Duration::from_secs(3600), {
        let state = state.clone();
        let ui_weak = ui_weak.clone();
        let rt = rt.clone(); // Runtime can be cloned and used
        move || {
            let state = state.clone();
            let ui_weak = ui_weak.clone();
            rt.spawn(async move {
                match check_for_update(&state).await {
                    Ok(_) => {
                        // Notify UI when update is available
                        let s = state.lock().unwrap();
                        if s.available {
                            let version = s.version.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak.upgrade() {
                                    let logic = ui.global::<crate::UpdateLogic>();
                                    logic.set_update_available(true);
                                    logic.set_new_version(version.into());
                                    logic.invoke_show_update_dialog();
                                }
                            })
                            .ok();
                        }
                    }
                    Err(e) => {
                        let mut s = state.lock().unwrap();
                        s.error = e.to_string();

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                ui.invoke_show_error();
                            }
                        })
                        .ok();
                    }
                }
            });
        }
    });
}
