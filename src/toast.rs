use lazy_static::lazy_static;
use slint::{ComponentHandle, Global, Timer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{MainWindow, ToastManager, ToastOptions, ToastType, i18n};

lazy_static! {
    static ref UI_WEAK: Arc<Mutex<Option<slint::Weak<MainWindow>>>> = Arc::new(Mutex::new(None));
}

static TOAST_GENERATION: AtomicU64 = AtomicU64::new(0);

// Default value constants
const DEFAULT_DURATION_TEXT: i32 = 2500;
const DEFAULT_DURATION_SUCCESS: i32 = 2500;
const DEFAULT_DURATION_ERROR: i32 = 3000;
const DEFAULT_DURATION_LOADING: i32 = 0; // loading defaults to no auto-close
// const DEFAULT_DURATION_GENERAL: i32 = 2500;

// Default messages stay semantic until the Toast is presented.
const DEFAULT_MESSAGE_TEXT_KEY: &str = "toast.default.text";
const DEFAULT_MESSAGE_SUCCESS_KEY: &str = "toast.default.success";
const DEFAULT_MESSAGE_ERROR_KEY: &str = "toast.default.error";
const DEFAULT_MESSAGE_LOADING_KEY: &str = "toast.default.loading";

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

    if let Some(width) = width_str.strip_suffix("px") {
        if let Ok(value) = width.trim().parse::<f32>() {
            return (false, value, 0.0);
        }
    } else if let Some(width) = width_str.strip_suffix('%')
        && let Ok(percent) = width.trim().parse::<f32>()
    {
        return (false, 0.0, percent);
    }

    // Default value: GitHub Primer content-sized toast.
    (true, 0.0, 0.0)
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
    // If message is empty string, use default message
    let final_message = if options.message.is_empty() {
        i18n::tr(default_message_key(toast_type)).into_owned()
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

    // GitHub Primer Toast follows content width and caps the card at 450px.
    let final_width = if options.width.is_empty() {
        "auto".to_string()
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

fn default_message_key(toast_type: ToastType) -> &'static str {
    match toast_type {
        ToastType::Text => DEFAULT_MESSAGE_TEXT_KEY,
        ToastType::Success => DEFAULT_MESSAGE_SUCCESS_KEY,
        ToastType::Error => DEFAULT_MESSAGE_ERROR_KEY,
        ToastType::Loading => DEFAULT_MESSAGE_LOADING_KEY,
    }
}

// Helper function: apply Toast configuration to UI
// This mirrors the application-owned Slint ToastOptions fields.
#[allow(clippy::too_many_arguments)]
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
    let generation = TOAST_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
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
            let guard = UI_WEAK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.clone()
        };

        if let Some(weak) = ui_weak {
            Timer::single_shot(Duration::from_millis(duration as u64), move || {
                if TOAST_GENERATION.load(Ordering::Relaxed) != generation {
                    return;
                }
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
    *UI_WEAK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ui_weak.clone());

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
        TOAST_GENERATION.fetch_add(1, Ordering::Relaxed);
        if let Some(ui) = ui_weak_dismiss.upgrade() {
            let manager = ToastManager::get(&ui);
            manager.set_visible(false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_messages_are_resolved_from_each_locale_catalog() {
        let cases = [
            (ToastType::Text, "Toast", "提示"),
            (ToastType::Success, "Operation successful!", "操作成功！"),
            (ToastType::Error, "Operation failed!", "操作失败！"),
            (ToastType::Loading, "Loading...", "加载中..."),
        ];

        for (toast_type, english, chinese) in cases {
            let key = default_message_key(toast_type);
            assert_eq!(usher_i18n::translate_for("en", key), english);
            assert_eq!(usher_i18n::translate_for("zh-CN", key), chinese);
        }
    }
}
