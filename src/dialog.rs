use lazy_static::lazy_static;
use slint::{ComponentHandle, Global, ModelRc, SharedString, Timer, VecModel};
use std::sync::{Arc, Mutex};

use crate::{DialogManager, DialogOptions, MainWindow};

lazy_static! {
    static ref UI_WEAK: Arc<Mutex<Option<slint::Weak<MainWindow>>>> = Arc::new(Mutex::new(None));
}

// 可发送的对话框选项（将 Rc 转换为 Vec）
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
        // 从 ModelRc 中提取 actions
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

/// 解析尺寸字符串，返回 (像素值, 百分比值, 是否为百分比)
fn parse_size(size_str: &str) -> (f32, f32, bool) {
    let size_str = size_str.trim();
    if size_str.ends_with('%') {
        // 百分比格式
        let percent_str = size_str.trim_end_matches('%').trim();
        let percent = percent_str.parse::<f32>().unwrap_or(0.0);
        (0.0, percent, true)
    } else if size_str.ends_with("px") {
        // 像素格式
        let px_str = size_str.trim_end_matches("px").trim();
        let px = px_str.parse::<f32>().unwrap_or(400.0);
        (px, 0.0, false)
    } else {
        // 尝试直接解析为数字（默认像素）
        let px = size_str.parse::<f32>().unwrap_or(400.0);
        (px, 0.0, false)
    }
}

// Dialog 管理器的初始化和回调绑定函数
pub fn setup_dialog(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    *UI_WEAK.lock().unwrap() = Some(ui_weak.clone());

    let manager = DialogManager::get(ui);

    // show 回调：延迟设置属性后显示对话框（避免 RefCell 借用冲突）
    let ui_weak_show = ui_weak.clone();
    manager.on_show(move |options: DialogOptions| {
        // 转换为可发送的选项
        let sendable_opts = SendableDialogOptions::from(&options);

        let ui_weak_timer = ui_weak_show.clone();
        Timer::single_shot(std::time::Duration::from_millis(1), move || {
            if let Some(ui) = ui_weak_timer.upgrade() {
                let manager = DialogManager::get(&ui);

                // 设置基本属性
                manager.set_id(SharedString::from(&sendable_opts.id));
                manager.set_title(SharedString::from(&sendable_opts.title));
                manager.set_content(SharedString::from(&sendable_opts.content));

                // 转换 actions 为 ModelRc
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

                // 解析并设置宽度
                let (width_px, width_percent, width_is_percent) = parse_size(&sendable_opts.width);
                manager.set_width_px(width_px);
                manager.set_width_percent(width_percent);
                manager.set_width_is_percent(width_is_percent);

                // 解析并设置高度
                let (height_px, height_percent, height_is_percent) =
                    parse_size(&sendable_opts.height);
                manager.set_height_px(height_px);
                manager.set_height_percent(height_percent);
                manager.set_height_is_percent(height_is_percent);

                // 设置 visible = true 显示对话框
                manager.set_visible(true);
            }
        });
    });

    // close 回调：关闭对话框
    let ui_weak_close = ui_weak.clone();
    manager.on_close(move || {
        if let Some(ui) = ui_weak_close.upgrade() {
            let manager = DialogManager::get(&ui);
            // 设置 visible = false 关闭对话框
            manager.set_visible(false);
        }
    });

    // hide 回调：通过 id 隐藏特定对话框
    let ui_weak_hide = ui_weak.clone();
    manager.on_hide(move |id| {
        if let Some(ui) = ui_weak_hide.upgrade() {
            let manager = DialogManager::get(&ui);
            // 只有当前对话框的 id 匹配时才关闭
            if manager.get_id() == id {
                manager.set_visible(false);
            }
        }
    });

    // 默认按钮点击回调（带 id 参数）
    let ui_weak_default = ui_weak.clone();
    manager.on_default_action_clicked(move |id| {
        println!("[Dialog] Default Action Clicked: id={}", id);
        let id_str = id.to_string();
        let ui_weak = ui_weak_default.clone();

        // 使用 Timer 延迟执行，避免 RefCell 借用冲突
        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(ui) = ui_weak.upgrade() {
                if id_str == "update-dialog" {
                    // 处理 update-dialog 的 Update 按钮点击
                    // 先关闭对话框，然后触发安装
                    DialogManager::get(&ui).set_visible(false);

                    // 再延迟一点调用 install，确保对话框完全关闭
                    let ui_weak_install = ui.as_weak();
                    Timer::single_shot(std::time::Duration::from_millis(50), move || {
                        if let Some(ui) = ui_weak_install.upgrade() {
                            ui.invoke_install();
                        }
                    });
                } else {
                    // 其他对话框：关闭对话框
                    DialogManager::get(&ui).set_visible(false);
                }
            }
        });
    });

    // 操作按钮点击回调（带 id 和 index 参数）
    let ui_weak_action = ui_weak.clone();
    manager.on_action_clicked(move |id, index| {
        println!("[Dialog] Action Clicked: id={}, index={}", id, index);
        let ui_weak = ui_weak_action.clone();

        // 使用 Timer 延迟执行，避免 RefCell 借用冲突
        Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(ui) = ui_weak.upgrade() {
                // 所有对话框点击操作按钮后关闭
                DialogManager::get(&ui).set_visible(false);
            }
        });
    });
}
