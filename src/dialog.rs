use lazy_static::lazy_static;
use slint::{ComponentHandle, Global, ModelRc, SharedString, Timer, VecModel};
use std::sync::{Arc, Mutex};

use crate::{DialogManager, DialogOptions, MainWindow};

lazy_static! {
    static ref UI_WEAK: Arc<Mutex<Option<slint::Weak<MainWindow>>>> = Arc::new(Mutex::new(None));
}

// Sendable dialog options (convert Rc to Vec)
struct SendableDialogOptions {
    id: String,
    title: String,
    content: String,
    actions: Vec<String>,
    default_action: String,
    overlay: bool,
    modal: bool,
    width: String,
    height: String,
}

impl From<&DialogOptions> for SendableDialogOptions {
    fn from(opts: &DialogOptions) -> Self {
        use slint::Model;
        // Extract actions from ModelRc
        let actions: Vec<String> = (0..opts.actions.row_count())
            .filter_map(|i| opts.actions.row_data(i).map(|s| s.to_string()))
            .collect();
        Self {
            id: opts.id.to_string(),
            title: opts.title.to_string(),
            content: opts.content.to_string(),
            actions,
            default_action: opts.default_action.to_string(),
            overlay: opts.overlay,
            modal: opts.modal,
            width: opts.width.to_string(),
            height: opts.height.to_string(),
        }
    }
}

/// Parse size string, return (pixel value, percentage value, is percentage)
fn parse_size(size_str: &str) -> (f32, f32, bool) {
    let size_str = size_str.trim();
    if size_str.ends_with('%') {
        // Percentage format
        let percent_str = size_str.trim_end_matches('%').trim();
        let percent = percent_str.parse::<f32>().unwrap_or(0.0);
        (0.0, percent, true)
    } else if size_str.ends_with("px") {
        // Pixel format
        let px_str = size_str.trim_end_matches("px").trim();
        let px = px_str.parse::<f32>().unwrap_or(400.0);
        (px, 0.0, false)
    } else {
        // Try to parse directly as number (default to pixels)
        let px = size_str.parse::<f32>().unwrap_or(400.0);
        (px, 0.0, false)
    }
}

// Dialog manager initialization and callback binding function
pub fn setup_dialog(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    *UI_WEAK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ui_weak.clone());

    let manager = DialogManager::get(ui);

    // show callback: delay setting properties before showing dialog (avoid RefCell borrow conflict)
    let ui_weak_show = ui_weak.clone();
    manager.on_show(move |options: DialogOptions| {
        // Convert to sendable options
        let sendable_opts = SendableDialogOptions::from(&options);

        let ui_weak_timer = ui_weak_show.clone();
        Timer::single_shot(std::time::Duration::from_millis(1), move || {
            if let Some(ui) = ui_weak_timer.upgrade() {
                let manager = DialogManager::get(&ui);

                // Set basic properties
                manager.set_id(SharedString::from(&sendable_opts.id));
                manager.set_title(SharedString::from(&sendable_opts.title));
                manager.set_content(SharedString::from(&sendable_opts.content));

                // Convert actions to ModelRc
                let actions_model: ModelRc<SharedString> = ModelRc::new(VecModel::from(
                    sendable_opts
                        .actions
                        .iter()
                        .map(|s| SharedString::from(s.as_str()))
                        .collect::<Vec<_>>(),
                ));
                manager.set_actions(actions_model);

                manager.set_default_action(SharedString::from(&sendable_opts.default_action));
                manager.set_overlay(sendable_opts.overlay);
                manager.set_modal(sendable_opts.modal);

                // Parse and set width
                let (width_px, width_percent, width_is_percent) = parse_size(&sendable_opts.width);
                manager.set_width_px(width_px);
                manager.set_width_percent(width_percent);
                manager.set_width_is_percent(width_is_percent);

                // Parse and set height
                let (height_px, height_percent, height_is_percent) =
                    parse_size(&sendable_opts.height);
                manager.set_height_px(height_px);
                manager.set_height_percent(height_percent);
                manager.set_height_is_percent(height_is_percent);

                // Set visible = true to show dialog
                manager.set_visible(true);
            }
        });
    });

    // close callback: close dialog
    let ui_weak_close = ui_weak.clone();
    manager.on_close(move || {
        if let Some(ui) = ui_weak_close.upgrade() {
            let manager = DialogManager::get(&ui);
            // Set visible = false to close dialog
            manager.set_visible(false);
        }
    });

    // hide callback: hide specific dialog by id
    let ui_weak_hide = ui_weak.clone();
    manager.on_hide(move |id| {
        if let Some(ui) = ui_weak_hide.upgrade() {
            let manager = DialogManager::get(&ui);
            // Only close if current dialog's id matches
            if manager.get_id() == id {
                manager.set_visible(false);
            }
        }
    });

    // Default button click callback (with id parameter)
    let ui_weak_default = ui_weak.clone();
    manager.on_default_action_clicked(move |id| {
        log::debug!("[Dialog] Default Action Clicked: id={}", id);
        let id_str = id.to_string();
        let ui_weak = ui_weak_default.clone();

        // Use Timer to delay execution, avoid RefCell borrow conflict
        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(ui) = ui_weak.upgrade() {
                if id_str == "update-dialog" {
                    // Handle update-dialog Update button click
                    // Close dialog first, then trigger installation
                    DialogManager::get(&ui).set_visible(false);

                    // Delay calling install a bit more to ensure dialog is fully closed
                    let ui_weak_install = ui.as_weak();
                    Timer::single_shot(std::time::Duration::from_millis(50), move || {
                        if let Some(ui) = ui_weak_install.upgrade() {
                            ui.invoke_install();
                        }
                    });
                } else {
                    // Other dialogs: close dialog
                    DialogManager::get(&ui).set_visible(false);
                }
            }
        });
    });

    // Action button click callback (with id and index parameters)
    let ui_weak_action = ui_weak.clone();
    manager.on_action_clicked(move |id, index| {
        log::debug!("[Dialog] Action Clicked: id={}, index={}", id, index);
        let ui_weak = ui_weak_action.clone();

        // Use Timer to delay execution, avoid RefCell borrow conflict
        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(ui) = ui_weak.upgrade() {
                // All dialogs close after clicking action button
                DialogManager::get(&ui).set_visible(false);
            }
        });
    });
}
