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
static SEMANTIC_TOAST: Mutex<Option<(u64, &'static str)>> = Mutex::new(None);

pub(crate) fn localized_error(ui: &MainWindow, key: &'static str) {
    ui.global::<ToastManager>().invoke_error(ToastOptions {
        message: i18n::tr(key).into_owned().into(),
        duration: 3000,
        position: "bottom".into(),
        width: "auto".into(),
        ..Default::default()
    });
    *SEMANTIC_TOAST.lock().unwrap() = Some((TOAST_GENERATION.load(Ordering::Relaxed), key));
}

pub(crate) fn sync_language(ui: &MainWindow) {
    if let Some((generation, key)) = *SEMANTIC_TOAST.lock().unwrap()
        && generation == TOAST_GENERATION.load(Ordering::Relaxed)
        && ui.global::<ToastManager>().get_visible()
    {
        ui.global::<ToastManager>()
            .set_message(i18n::tr(key).into_owned().into());
    }
}

const DEFAULT_DURATION_TEXT: i32 = 2500;
const DEFAULT_DURATION_SUCCESS: i32 = 2500;
const DEFAULT_DURATION_ERROR: i32 = 3000;
const DEFAULT_DURATION_LOADING: i32 = 0;

// Default messages stay semantic until the Toast is presented.
const DEFAULT_MESSAGE_TEXT_KEY: &str = "toast.default.text";
const DEFAULT_MESSAGE_SUCCESS_KEY: &str = "toast.default.success";
const DEFAULT_MESSAGE_ERROR_KEY: &str = "toast.default.error";
const DEFAULT_MESSAGE_LOADING_KEY: &str = "toast.default.loading";

const DEFAULT_POSITION: &str = "middle";

#[derive(Debug, Clone, Copy, PartialEq)]
enum ToastWidth {
    Auto,
    Pixels(f32),
    Percent(f32),
}

impl ToastWidth {
    fn slint_values(self) -> (bool, f32, f32) {
        match self {
            Self::Auto => (true, 0.0, 0.0),
            Self::Pixels(value) => (false, value, 0.0),
            Self::Percent(value) => (false, 0.0, value),
        }
    }
}

fn parse_width(width_str: &str) -> ToastWidth {
    let width_str = width_str.trim();

    if width_str == "auto" {
        return ToastWidth::Auto;
    }

    if let Some(width) = width_str.strip_suffix("px") {
        if let Ok(value) = width.trim().parse::<f32>() {
            return ToastWidth::Pixels(value);
        }
    } else if let Some(width) = width_str.strip_suffix('%')
        && let Ok(percent) = width.trim().parse::<f32>()
    {
        return ToastWidth::Percent(percent);
    }

    ToastWidth::Auto
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedToast {
    message: String,
    toast_type: ToastType,
    position: String,
    icon: String,
    overlay: bool,
    close_on_click: bool,
    loading_type: String,
    width_source: String,
    width: ToastWidth,
    duration: i32,
}

impl ResolvedToast {
    fn new(toast_type: ToastType, options: ToastOptions) -> Self {
        let message = if options.message.is_empty() {
            i18n::tr(default_message_key(toast_type)).into_owned()
        } else {
            options.message.to_string()
        };
        let position = if options.position.is_empty() {
            DEFAULT_POSITION.to_string()
        } else {
            options.position.to_string()
        };
        let loading_type = if options.loading_type.is_empty() {
            "circular".to_string()
        } else {
            options.loading_type.to_string()
        };
        let width_source = if options.width.is_empty() {
            "auto".to_string()
        } else {
            options.width.to_string()
        };
        let duration = if options.duration < 0 {
            default_duration(toast_type)
        } else {
            options.duration
        };

        Self {
            message,
            toast_type,
            position,
            icon: options.icon.to_string(),
            overlay: options.overlay,
            close_on_click: options.close_on_click,
            loading_type,
            width: parse_width(&width_source),
            width_source,
            duration,
        }
    }
}

fn default_message_key(toast_type: ToastType) -> &'static str {
    match toast_type {
        ToastType::Text => DEFAULT_MESSAGE_TEXT_KEY,
        ToastType::Success => DEFAULT_MESSAGE_SUCCESS_KEY,
        ToastType::Error => DEFAULT_MESSAGE_ERROR_KEY,
        ToastType::Loading => DEFAULT_MESSAGE_LOADING_KEY,
    }
}

fn default_duration(toast_type: ToastType) -> i32 {
    match toast_type {
        ToastType::Text => DEFAULT_DURATION_TEXT,
        ToastType::Success => DEFAULT_DURATION_SUCCESS,
        ToastType::Error => DEFAULT_DURATION_ERROR,
        ToastType::Loading => DEFAULT_DURATION_LOADING,
    }
}

fn apply_toast(manager: &ToastManager, toast: ResolvedToast) {
    let generation = TOAST_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let (width_is_auto, width_value_px, width_percent) = toast.width.slint_values();

    manager.set_message(toast.message.into());
    manager.set_type(toast.toast_type);
    manager.set_position(toast.position.into());
    manager.set_icon(toast.icon.into());
    manager.set_overlay(toast.overlay);
    manager.set_close_on_click(toast.close_on_click);
    manager.set_loading_type(toast.loading_type.into());
    manager.set_width(toast.width_source.into());
    manager.set_width_is_auto(width_is_auto);
    manager.set_width_value(width_value_px);
    manager.set_width_percent(width_percent);
    manager.set_visible(true);

    if toast.duration > 0 {
        let ui_weak = {
            let guard = UI_WEAK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.clone()
        };

        if let Some(weak) = ui_weak {
            Timer::single_shot(Duration::from_millis(toast.duration as u64), move || {
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

fn show_toast(ui: &MainWindow, toast_type: ToastType, options: ToastOptions) {
    apply_toast(
        &ToastManager::get(ui),
        ResolvedToast::new(toast_type, options),
    );
}

// Toast manager initialization and callback binding function
pub fn setup_toast(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    *UI_WEAK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ui_weak.clone());

    let manager = ToastManager::get(ui);

    let ui_weak_success = ui_weak.clone();
    manager.on_success(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_success.upgrade() {
            show_toast(&ui, ToastType::Success, options);
        }
    });

    let ui_weak_error = ui_weak.clone();
    manager.on_error(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_error.upgrade() {
            show_toast(&ui, ToastType::Error, options);
        }
    });

    let ui_weak_info = ui_weak.clone();
    manager.on_info(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_info.upgrade() {
            show_toast(&ui, ToastType::Text, options);
        }
    });

    let ui_weak_loading = ui_weak.clone();
    manager.on_loading(move |options: ToastOptions| {
        if let Some(ui) = ui_weak_loading.upgrade() {
            show_toast(&ui, ToastType::Loading, options);
        }
    });

    let ui_weak_show = ui_weak.clone();
    manager.on_show(move |toast_type: ToastType, options: ToastOptions| {
        if let Some(ui) = ui_weak_show.upgrade() {
            show_toast(&ui, toast_type, options);
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

    fn options() -> ToastOptions {
        ToastOptions {
            duration: -1,
            ..Default::default()
        }
    }

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

    #[test]
    fn width_parser_preserves_existing_auto_pixel_percent_and_fallback_semantics() {
        assert_eq!(parse_width("auto").slint_values(), (true, 0.0, 0.0));
        assert_eq!(parse_width(" 320px ").slint_values(), (false, 320.0, 0.0));
        assert_eq!(parse_width(" 45% ").slint_values(), (false, 0.0, 45.0));
        assert_eq!(parse_width("invalid").slint_values(), (true, 0.0, 0.0));
        assert_eq!(parse_width("-2px").slint_values(), (false, -2.0, 0.0));
    }

    #[test]
    fn toast_defaults_preserve_options_and_type_specific_duration_contract() {
        let mut custom = options();
        custom.message = "custom".into();
        custom.position = "top".into();
        custom.icon = "icon.svg".into();
        custom.overlay = true;
        custom.close_on_click = true;
        custom.loading_type = "linear".into();
        custom.width = "75%".into();

        let resolved = ResolvedToast::new(ToastType::Error, custom);
        assert_eq!(resolved.message, "custom");
        assert_eq!(resolved.toast_type, ToastType::Error);
        assert_eq!(resolved.position, "top");
        assert_eq!(resolved.icon, "icon.svg");
        assert!(resolved.overlay);
        assert!(resolved.close_on_click);
        assert_eq!(resolved.loading_type, "linear");
        assert_eq!(resolved.width_source, "75%");

        for (toast_type, duration) in [
            (ToastType::Text, DEFAULT_DURATION_TEXT),
            (ToastType::Success, DEFAULT_DURATION_SUCCESS),
            (ToastType::Error, DEFAULT_DURATION_ERROR),
            (ToastType::Loading, DEFAULT_DURATION_LOADING),
        ] {
            let resolved = ResolvedToast::new(toast_type, options());
            assert_eq!(resolved.position, DEFAULT_POSITION);
            assert_eq!(resolved.loading_type, "circular");
            assert_eq!(resolved.width_source, "auto");
            assert_eq!(resolved.duration, duration);
        }

        for duration in [0, 1234] {
            let mut explicit = options();
            explicit.duration = duration;
            assert_eq!(
                ResolvedToast::new(ToastType::Success, explicit).duration,
                duration
            );
        }
    }

    #[test]
    fn callbacks_apply_the_resolved_configuration_to_the_slint_manager() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        setup_toast(&ui);
        let manager = ToastManager::get(&ui);
        let mut custom = options();
        custom.message = "saved".into();
        custom.duration = 0;
        custom.position = "top".into();
        custom.icon = "icon.svg".into();
        custom.overlay = true;
        custom.close_on_click = true;
        custom.loading_type = "linear".into();
        custom.width = "75%".into();

        manager.invoke_success(custom);

        assert!(manager.get_visible());
        assert_eq!(manager.get_message(), "saved");
        assert_eq!(manager.get_type(), ToastType::Success);
        assert_eq!(manager.get_position(), "top");
        assert_eq!(manager.get_icon(), "icon.svg");
        assert!(manager.get_overlay());
        assert!(manager.get_close_on_click());
        assert_eq!(manager.get_loading_type(), "linear");
        assert_eq!(manager.get_width(), "75%");
        assert!(!manager.get_width_is_auto());
        assert_eq!(manager.get_width_percent(), 75.0);

        manager.invoke_dismiss();
        assert!(!manager.get_visible());
    }
}
