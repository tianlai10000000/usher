use slint::{ComponentHandle, Global, Timer};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use lazy_static::lazy_static;

use crate::{MainWindow, ToastManager, ToastType, ToastOptions};

lazy_static! {
    static ref UI_WEAK: Arc<Mutex<Option<slint::Weak<MainWindow>>>> = Arc::new(Mutex::new(None));
}

// 默认值常量
const DEFAULT_DURATION_TEXT: i32 = 2500;
const DEFAULT_DURATION_SUCCESS: i32 = 2500;
const DEFAULT_DURATION_ERROR: i32 = 3000;
const DEFAULT_DURATION_LOADING: i32 = 0; // loading 默认不自动关闭
const DEFAULT_DURATION_GENERAL: i32 = 2500;

// 默认消息常量
const DEFAULT_MESSAGE_TEXT: &str = "Toast";
const DEFAULT_MESSAGE_SUCCESS: &str = "操作成功！";
const DEFAULT_MESSAGE_ERROR: &str = "操作失败！";
const DEFAULT_MESSAGE_LOADING: &str = "加载中...";

// 默认位置
const DEFAULT_POSITION: &str = "middle";

// 解析width字符串，返回(是否为auto, 宽度值px, 百分比值)
// 如果width_str是百分比，width_value为0，percent_value为百分比数值
// 如果width_str是px，width_value为px值，percent_value为0
// 如果width_str是auto，width_is_auto为true
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
    
    // 默认值：20%
    (false, 0.0, 20.0)
}

// 辅助函数：创建带默认值的 Toast 配置
fn create_toast_with_defaults(
    toast_type: ToastType,
    options: ToastOptions,
) -> (String, ToastType, String, String, bool, bool, String, String) {
    let default_duration = match toast_type {
        ToastType::Text => DEFAULT_DURATION_TEXT,
        ToastType::Success => DEFAULT_DURATION_SUCCESS,
        ToastType::Error => DEFAULT_DURATION_ERROR,
        ToastType::Loading => DEFAULT_DURATION_LOADING,
    };
    
    let default_message = match toast_type {
        ToastType::Text => DEFAULT_MESSAGE_TEXT,
        ToastType::Success => DEFAULT_MESSAGE_SUCCESS,
        ToastType::Error => DEFAULT_MESSAGE_ERROR,
        ToastType::Loading => DEFAULT_MESSAGE_LOADING,
    };
    
    // 如果 message 为空字符串，使用默认消息
    let final_message = if options.message.is_empty() {
        default_message.to_string()
    } else {
        options.message.to_string()
    };
    
    // duration 处理逻辑：
    // - duration < 0：使用默认值
    // - duration == 0：永久存在（不自动关闭）
    // - duration > 0：使用指定值
    let final_duration = if options.duration < 0 {
        default_duration
    } else {
        options.duration
    };
    
    // position 处理：如果为空，使用默认值
    let final_position = if options.position.is_empty() {
        DEFAULT_POSITION.to_string()
    } else {
        options.position.to_string()
    };
    
    // loading-type 处理：如果为空，使用默认值
    let final_loading_type = if options.loading_type.is_empty() {
        "circular".to_string()
    } else {
        options.loading_type.to_string()
    };
    
    // width 处理：如果为空，使用默认值"20%"
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

// 辅助函数：应用 Toast 配置到 UI
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
    // 解析width字符串
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
    
    // 如果 duration > 0，设置自动关闭定时器
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

// Toast 管理器的初始化和回调绑定函数
pub fn setup_toast(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    *UI_WEAK.lock().unwrap() = Some(ui_weak.clone());
    
    let manager = ToastManager::get(ui);
    
    // 便捷方法回调 - success
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
    
    // 便捷方法回调 - error
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
    
    // 便捷方法回调 - info
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
    
    // 便捷方法回调 - loading
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
    
    // 通用方法回调 - show
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
    
    // dismiss 回调
    let ui_weak_dismiss = ui_weak.clone();
    manager.on_dismiss(move || {
        if let Some(ui) = ui_weak_dismiss.upgrade() {
            let manager = ToastManager::get(&ui);
            manager.set_visible(false);
        }
    });
}
