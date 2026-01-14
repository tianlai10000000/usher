use lazy_static::lazy_static;
use slint::{ComponentHandle, Global, Timer};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{MainWindow, ToastManager, ToastOptions, ToastType};

lazy_static! {
    static ref UI_WEAK: Arc<Mutex<Option<slint::Weak<MainWindow>>>> = Arc::new(Mutex::new(None));
}

// Default value constants
const DEFAULT_DURATION_TEXT: i32 = 2500;
const DEFAULT_DURATION_SUCCESS: i32 = 2500;
const DEFAULT_DURATION_ERROR: i32 = 3000;
const DEFAULT_DURATION_LOADING: i32 = 0; // loading defaults to no auto-close
// const DEFAULT_DURATION_GENERAL: i32 = 2500;

// Default message constants
const DEFAULT_MESSAGE_TEXT: &str = "Toast";
const DEFAULT_MESSAGE_SUCCESS: &str = "Operation successful!";
const DEFAULT_MESSAGE_ERROR: &str = "Operation failed!";
const DEFAULT_MESSAGE_LOADING: &str = "Loading...";

// Default position
const DEFAULT_POSITION: &str = "middle";

// Parse width string, return (is_auto, width_value_px, percent_value)
// If width_str is percentage, width_value is 0, percent_value is percentage value
// If width_str is px, width_value is px value, percent_value is 0
// If width_str is auto, width_is_auto is true
fn parse_width(width_str: &str) -> (bool, f32, f32) {
    let width_str = width_str.trim();

    if width_str == "auto" {
        return (true, 0.0, 0.0);
    }

    if width_str.ends_with("px") {
        if let Ok(value) = width_str[..width_str.len() - 2].trim().parse::<f32>() {
            return (false, value, 0.0);
        }
    } else if width_str.ends_with("%") {
        if let Ok(percent) = width_str[..width_str.len() - 1].trim().parse::<f32>() {
            return (false, 0.0, percent);
        }
    }

    // Default value: 20%
    (false, 0.0, 20.0)
}

// Helper function: create Toast configuration with default values
fn create_toast_with_defaults(
    toast_type: ToastType,
    options: ToastOptions,
) -> (
    String,
    ToastType,
    String,
    String,
    bool,
    bool,
    String,
    String,
) {
    let default_message = match toast_type {
        ToastType::Text => DEFAULT_MESSAGE_TEXT,
        ToastType::Success => DEFAULT_MESSAGE_SUCCESS,
        ToastType::Error => DEFAULT_MESSAGE_ERROR,
        ToastType::Loading => DEFAULT_MESSAGE_LOADING,
    };

    // If message is empty string, use default message
    let final_message = if options.message.is_empty() {
        default_message.to_string()
    } else {
        options.message.to_string()
    };

    // duration handling logic:
    // - duration < 0: use default value
    // - duration == 0: permanent (no auto-close)
    // - duration > 0: use specified value
    // let final_duration = if options.duration < 0 {
    //     default_duration
    // } else {
    //     options.duration
    // };

    // position handling: if empty, use default value
    let final_position = if options.position.is_empty() {
        DEFAULT_POSITION.to_string()
    } else {
        options.position.to_string()
    };

    // loading-type handling: if empty, use default value
    let final_loading_type = if options.loading_type.is_empty() {
        "circular".to_string()
    } else {
        options.loading_type.to_string()
    };

    // width handling: if empty, use default value "20%"
    let final_width = if options.width.is_empty() {
        "20%".to_string()
    } else {
        options.width.to_string()
    };

    (
        final_message,
        toast_type,
        final_position,
        options.icon.to_string(),
        options.overlay,
        options.close_on_click,
        final_loading_type,
        final_width,
    )
}

// Helper function: apply Toast configuration to UI
fn apply_toast(
    manager: &ToastManager,
    message: String,
    toast_type: ToastType,
    position: String,
    icon: String,
    overlay: bool,
    close_on_click: bool,
    loading_type: String,
    width: String,
    duration: i32,
) {
    // Parse width string
    let (width_is_auto, width_value_px, width_percent) = parse_width(&width);

    manager.set_message(message.clone().into());
    manager.set_type(toast_type);
    manager.set_position(position.clone().into());
    manager.set_icon(icon.clone().into());
    manager.set_overlay(overlay);
    manager.set_close_on_click(close_on_click);
    manager.set_loading_type(loading_type.clone().into());
    manager.set_width(width.clone().into());
    manager.set_width_is_auto(width_is_auto);
    manager.set_width_value(width_value_px);
    manager.set_width_percent(width_percent);
    manager.set_visible(true);

    // If duration > 0, set auto-close timer
    if duration > 0 {
        let ui_weak = {
            let guard = UI_WEAK.lock().unwrap();
            guard.clone()
        };

        if let Some(weak) = ui_weak {
            Timer::single_shot(Duration::from_millis(duration as u64), move || {
                if let Some(ui) = weak.upgrade() {
                    let manager = ToastManager::get(&ui);
                    manager.set_visible(false);
                }
            });
        }
    }
}

// Toast manager initialization and callback binding function
pub fn setup_toast(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    *UI_WEAK.lock().unwrap() = Some(ui_weak.clone());

    let manager = ToastManager::get(ui);

    // Convenience method callback - success
    let ui_weak_success = ui_weak.clone();
    manager.on_success(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_success.upgrade() {
            let manager = ToastManager::get(&ui);
            let options_clone = options.clone();
            let (message, toast_type, position, icon, overlay, close_on_click, loading_type, width) =
                create_toast_with_defaults(ToastType::Success, options_clone.clone());
            let duration = if options_clone.duration < 0 {
                DEFAULT_DURATION_SUCCESS
            } else {
                options_clone.duration
            };
            apply_toast(
                &manager,
                message,
                toast_type,
                position,
                icon,
                overlay,
                close_on_click,
                loading_type,
                width,
                duration,
            );
        }
    });

    // Convenience method callback - error
    let ui_weak_error = ui_weak.clone();
    manager.on_error(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_error.upgrade() {
            let manager = ToastManager::get(&ui);
            let options_clone = options.clone();
            let (message, toast_type, position, icon, overlay, close_on_click, loading_type, width) =
                create_toast_with_defaults(ToastType::Error, options_clone.clone());
            let duration = if options_clone.duration < 0 {
                DEFAULT_DURATION_ERROR
            } else {
                options_clone.duration
            };
            apply_toast(
                &manager,
                message,
                toast_type,
                position,
                icon,
                overlay,
                close_on_click,
                loading_type,
                width,
                duration,
            );
        }
    });

    // Convenience method callback - info
    let ui_weak_info = ui_weak.clone();
    manager.on_info(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_info.upgrade() {
            let manager = ToastManager::get(&ui);
            let options_clone = options.clone();
            let (message, toast_type, position, icon, overlay, close_on_click, loading_type, width) =
                create_toast_with_defaults(ToastType::Text, options_clone.clone());
            let duration = if options_clone.duration < 0 {
                DEFAULT_DURATION_TEXT
            } else {
                options_clone.duration
            };
            apply_toast(
                &manager,
                message,
                toast_type,
                position,
                icon,
                overlay,
                close_on_click,
                loading_type,
                width,
                duration,
            );
        }
    });

    // Convenience method callback - loading
    let ui_weak_loading = ui_weak.clone();
    manager.on_loading(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_loading.upgrade() {
            let manager = ToastManager::get(&ui);
            let options_clone = options.clone();
            let (message, toast_type, position, icon, overlay, close_on_click, loading_type, width) =
                create_toast_with_defaults(ToastType::Loading, options_clone.clone());
            let duration = if options_clone.duration < 0 {
                DEFAULT_DURATION_LOADING
            } else {
                options_clone.duration
            };
            apply_toast(
                &manager,
                message,
                toast_type,
                position,
                icon,
                overlay,
                close_on_click,
                loading_type,
                width,
                duration,
            );
        }
    });

    // Generic method callback - show
    let ui_weak_show = ui_weak.clone();
    manager.on_show(move |toast_type: ToastType, options: ToastOptions| {
        if let Some(ui) = ui_weak_show.upgrade() {
            let manager = ToastManager::get(&ui);
            let options_clone = options.clone();
            let (message, toast_type, position, icon, overlay, close_on_click, loading_type, width) =
                create_toast_with_defaults(toast_type, options_clone.clone());
            let default_duration = match toast_type {
                ToastType::Text => DEFAULT_DURATION_TEXT,
                ToastType::Success => DEFAULT_DURATION_SUCCESS,
                ToastType::Error => DEFAULT_DURATION_ERROR,
                ToastType::Loading => DEFAULT_DURATION_LOADING,
            };
            let duration = if options_clone.duration < 0 {
                default_duration
            } else {
                options_clone.duration
            };
            apply_toast(
                &manager,
                message,
                toast_type,
                position,
                icon,
                overlay,
                close_on_click,
                loading_type,
                width,
                duration,
            );
        }
    });

    // dismiss callback
    let ui_weak_dismiss = ui_weak.clone();
    manager.on_dismiss(move || {
        if let Some(ui) = ui_weak_dismiss.upgrade() {
            let manager = ToastManager::get(&ui);
            manager.set_visible(false);
        }
    });
}
