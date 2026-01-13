use slint::{ComponentHandle, Global, Model, ModelRc, SharedVector, Timer, VecModel, Weak};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};
use std::time::Duration;

// 使用从 crate 根重新导出的类型
use crate::{
    MainWindow, Notification, NotificationIcons, NotificationManager, NotificationOptions,
    NotificationType,
};

// 全局计数器，用于生成唯一ID
static NOTIFICATION_ID_COUNTER: AtomicI32 = AtomicI32::new(0);

// 全局 hover 状态跟踪（避免修改 Notification struct 触发重新渲染）
lazy_static::lazy_static! {
    static ref HOVER_STATE: RwLock<HashMap<i32, bool>> = RwLock::new(HashMap::new());
}

// 生成下一个唯一ID
fn next_notification_id() -> i32 {
    NOTIFICATION_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// 默认值常量
const DEFAULT_DURATION_INFO: i32 = 3000;
const DEFAULT_DURATION_SUCCESS: i32 = 3000;
const DEFAULT_DURATION_WARNING: i32 = 3000;
const DEFAULT_DURATION_ERROR: i32 = 5000;
// const DEFAULT_DURATION_GENERAL: i32 = 3000;

// 默认消息常量
const DEFAULT_MESSAGE_INFO: &str = "Info";
const DEFAULT_MESSAGE_SUCCESS: &str = "Success";
const DEFAULT_MESSAGE_WARNING: &str = "Warning";
const DEFAULT_MESSAGE_ERROR: &str = "Error";

// 全局 UI 引用（用于获取嵌入的图标）
lazy_static::lazy_static! {
    static ref UI_WEAK: RwLock<Option<Weak<MainWindow>>> = RwLock::new(None);
}

// 辅助函数：获取嵌入的默认图标
fn get_default_icon(notification_type: NotificationType) -> slint::Image {
    // 尝试从 Slint global 获取嵌入的图标
    if let Some(ui_weak) = UI_WEAK.read().ok().and_then(|guard| guard.clone()) {
        if let Some(ui) = ui_weak.upgrade() {
            let icons = NotificationIcons::get(&ui);
            return match notification_type {
                NotificationType::Info => icons.get_info(),
                NotificationType::Success => icons.get_success(),
                NotificationType::Warning => icons.get_warning(),
                NotificationType::Error => icons.get_error(),
            };
        }
    }

    // 如果无法获取嵌入图标，尝试从文件系统加载（开发模式备用）
    let icon_path_str = match notification_type {
        NotificationType::Info => "ui/assets/icons/info.svg",
        NotificationType::Success => "ui/assets/icons/success.svg",
        NotificationType::Warning => "ui/assets/icons/warning.svg",
        NotificationType::Error => "ui/assets/icons/error.svg",
    };

    let icon_path = Path::new(icon_path_str);
    slint::Image::load_from_path(icon_path).unwrap_or_else(|_| slint::Image::default())
}

// 辅助函数：创建带默认值的通知
// 从 NotificationOptions 中提取值并应用默认值
fn create_notification_with_defaults(
    notification_type: NotificationType,
    options: NotificationOptions,
) -> Notification {
    let default_duration = match notification_type {
        NotificationType::Info => DEFAULT_DURATION_INFO,
        NotificationType::Success => DEFAULT_DURATION_SUCCESS,
        NotificationType::Warning => DEFAULT_DURATION_WARNING,
        NotificationType::Error => DEFAULT_DURATION_ERROR,
    };

    let default_message = match notification_type {
        NotificationType::Info => DEFAULT_MESSAGE_INFO,
        NotificationType::Success => DEFAULT_MESSAGE_SUCCESS,
        NotificationType::Warning => DEFAULT_MESSAGE_WARNING,
        NotificationType::Error => DEFAULT_MESSAGE_ERROR,
    };

    // 如果 message 为空字符串，使用默认消息
    let final_message = if options.message.is_empty() {
        slint::SharedString::from(default_message)
    } else {
        options.message
    };

    // duration 处理逻辑：
    // - duration < 0：使用默认值
    // - duration == 0：永久存在（不自动关闭）
    // - duration > 0：使用指定值
    let final_duration = if options.duration < 0 {
        default_duration
    } else {
        options.duration // 包括 0（永久）和正数（指定时间）
    };

    // icon 的处理：优先使用用户传入的图标，如果为空则使用默认图标
    // 检查用户传入的图标是否为空图像
    // 如果用户传入了有效图标，使用用户的图标；否则使用默认图标
    //
    // 注意：在 Slint 中，如果用户没有传入 icon 字段，结构体可能使用一个特殊的默认值
    // 这个默认值可能不等于 slint::Image::default()，所以我们需要更宽松的检测
    //
    // 策略：由于无法可靠检测空图像，我们采用保守策略：
    // 1. 如果传入的图标是空图像（通过 is_empty_image 检测），使用默认图标
    // 2. 否则，使用用户传入的图标
    //
    // 但是，如果用户在 Slint 中没有传入 icon，结构体可能使用一个特殊的默认值
    // 这个值可能不是空图像，但也不是用户想要的图标
    //
    // 解决方案：在 Slint 端，当用户没有传入 icon 时，应该显式传入一个空图像
    // 或者在 Rust 端，我们可以尝试一个更可靠的方法来检测空图像
    //
    // 当前实现：如果传入的图标是空图像，使用默认图标；否则使用用户传入的图标
    // 如果用户在 Slint 中没有传入 icon，结构体可能使用一个特殊的默认值
    // 这个值可能不是空图像，所以我们需要在 Slint 端处理，确保未传入时传入空图像
    // icon 的处理：使用 icon 字符串
    // 逻辑：
    // - 如果 icon 为空字符串，使用默认图标
    // - 如果 icon 不为空，从路径加载图标
    //
    // 使用方式：
    // 1. 不传入 icon（使用默认图标）：
    //    NotificationManager.show(NotificationType.Success, { message: "Hello" })
    //
    // 2. 显式传入 icon（使用自定义图标）：
    //    NotificationManager.show(NotificationType.Success, {
    //        message: "Hello",
    //        icon: "ui/assets/icons/error.svg"
    //    })
    let final_icon = if options.icon.is_empty() {
        // 使用默认图标
        get_default_icon(notification_type)
    } else {
        // 从路径加载自定义图标
        let icon_path = Path::new(options.icon.as_str());
        slint::Image::load_from_path(icon_path).unwrap_or_else(|_| {
            // 如果加载失败，使用默认图标
            get_default_icon(notification_type)
        })
    };

    Notification {
        id: next_notification_id(),
        title: options.title.clone(), // title 直接使用用户传入的值，空字符串表示无标题
        message: final_message,
        r#type: notification_type,
        duration: final_duration,
        icon: final_icon,
        custom_component: options.component,
        dismissed: false, // 初始为未关闭
        hovered: false,   // 初始为未悬停
    }
}

#[derive(Default, Clone)]
pub struct SharedQueue(pub SharedVector<Notification>);

// 辅助函数：将通知添加到队列
fn push_notification(
    manager: &NotificationManager,
    notification: Notification,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    // 先收集所有数据到一个 Vec，避免持有 queue_model 的借用
    let mut items = Vec::new();
    {
        let queue_model: ModelRc<Notification> = manager.get_queue();
        let row_count = queue_model.row_count();
        for i in 0..row_count {
            if let Some(item) = queue_model.row_data(i) {
                items.push(item);
            }
        }
    } // queue_model 在这里被 drop，释放所有借用

    // 创建新的 VecModel，复制现有元素并添加新元素
    let mut new_vec = VecModel::default();
    for item in items {
        new_vec.push(item);
    }
    new_vec.push(notification.clone());
    manager.set_queue(ModelRc::new(new_vec));

    // 可选：更新共享队列
    if let Some(queue) = shared_queue {
        if let Ok(mut q) = queue.write() {
            q.0.push(notification.clone());
        }
    }
}

// 辅助函数：通过通知ID删除通知（精确匹配，避免索引错位问题）
fn remove_notification_by_id(
    manager: &NotificationManager,
    target_id: i32,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    // 清理 hover 状态
    if let Ok(mut state) = HOVER_STATE.write() {
        state.remove(&target_id);
    }

    // 先收集所有数据到一个 Vec，避免持有 queue_model 的借用
    let mut items = Vec::new();
    {
        let queue_model: ModelRc<Notification> = manager.get_queue();
        let row_count = queue_model.row_count();
        for i in 0..row_count {
            if let Some(item) = queue_model.row_data(i) {
                // 通过ID精确匹配，确保每个通知的定时器只删除自己
                if item.id != target_id {
                    items.push(item);
                }
            }
        }
    } // queue_model 在这里被 drop，释放所有借用

    // 创建新的 VecModel，排除匹配ID的通知
    let new_vec = VecModel::default();
    for item in items {
        new_vec.push(item);
    }
    manager.set_queue(ModelRc::new(new_vec));

    // 可选：更新共享队列
    if let Some(queue) = shared_queue {
        if let Ok(mut q) = queue.write() {
            let mut new_vec = SharedVector::default();
            for i in 0..q.0.len() {
                let item = &q.0[i];
                if item.id != target_id {
                    new_vec.push(item.clone());
                }
            }
            q.0 = new_vec;
        }
    }
}

// 辅助函数：标记通知为已关闭（不从队列删除）
fn dismiss_notification_by_id(
    manager: &NotificationManager,
    target_id: i32,
    _shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    // 收集所有数据，修改匹配ID的通知的 dismissed 字段
    let mut items = Vec::new();
    {
        let queue_model: ModelRc<Notification> = manager.get_queue();
        let row_count = queue_model.row_count();
        for i in 0..row_count {
            if let Some(mut item) = queue_model.row_data(i) {
                if item.id == target_id {
                    item.dismissed = true;
                }
                items.push(item);
            }
        }
    }

    // 创建新的 VecModel
    let new_vec = VecModel::default();
    for item in items {
        new_vec.push(item);
    }
    manager.set_queue(ModelRc::new(new_vec));
}

// 辅助函数：设置通知的 hover 状态（使用全局 HashMap，避免触发 UI 重新渲染）
fn set_notification_hovered_by_id(
    _manager: &NotificationManager,
    target_id: i32,
    hovered: bool,
    _shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    if let Ok(mut state) = HOVER_STATE.write() {
        if hovered {
            state.insert(target_id, true);
        } else {
            state.remove(&target_id);
        }
    }
}

// 辅助函数：检查通知是否处于 hover 状态（从全局 HashMap 读取）
fn is_notification_hovered(_manager: &NotificationManager, target_id: i32) -> bool {
    if let Ok(state) = HOVER_STATE.read() {
        state.get(&target_id).copied().unwrap_or(false)
    } else {
        false
    }
}

// 辅助函数：检查通知是否存在
fn notification_exists(manager: &NotificationManager, target_id: i32) -> bool {
    let queue_model: ModelRc<Notification> = manager.get_queue();
    let row_count = queue_model.row_count();
    for i in 0..row_count {
        if let Some(item) = queue_model.row_data(i) {
            if item.id == target_id {
                return true;
            }
        }
    }
    false
}

// 辅助函数：获取通知的 duration
fn get_notification_duration(manager: &NotificationManager, target_id: i32) -> Option<i32> {
    let queue_model: ModelRc<Notification> = manager.get_queue();
    let row_count = queue_model.row_count();
    for i in 0..row_count {
        if let Some(item) = queue_model.row_data(i) {
            if item.id == target_id {
                return Some(item.duration);
            }
        }
    }
    None
}

// 启动自动关闭定时器（支持 hover 暂停）
// 如果通知处于 hover 状态，定时器到期后会继续等待
fn start_auto_close_timer(
    ui_weak: slint::Weak<MainWindow>,
    notification_id: i32,
    duration: Duration,
    shared_queue: Option<Arc<RwLock<SharedQueue>>>,
) {
    let ui_weak_clone = ui_weak.clone();
    let shared_queue_clone = shared_queue.clone();

    Timer::single_shot(duration, move || {
        if let Some(ui) = ui_weak_clone.upgrade() {
            let manager = NotificationManager::get(&ui);

            // 检查通知是否还存在
            if !notification_exists(&manager, notification_id) {
                return;
            }

            // 检查通知是否处于 hover 状态
            if is_notification_hovered(&manager, notification_id) {
                // 如果 hover，继续等待（每 200ms 检查一次）
                start_auto_close_timer(
                    ui_weak,
                    notification_id,
                    Duration::from_millis(200),
                    shared_queue_clone,
                );
            } else {
                // 不是 hover，删除通知
                remove_notification_by_id(&manager, notification_id, &shared_queue_clone);
            }
        }
    });
}

// 通知管理器的初始化和回调绑定函数
pub fn setup_notifications(
    ui: &MainWindow,
    shared_queue: Option<Arc<RwLock<SharedQueue>>>, // 可选：用于多窗口共享
) {
    let ui_weak = ui.as_weak();

    // 存储 UI 引用，用于获取嵌入的图标
    if let Ok(mut guard) = UI_WEAK.write() {
        *guard = Some(ui_weak.clone());
    }

    let manager = NotificationManager::get(ui);

    // 便捷方法回调
    let ui_weak_info = ui_weak.clone();
    let shared_queue_clone_info = shared_queue.clone();
    manager.on_info(move |options: NotificationOptions| {
        let ui_weak = ui_weak_info.clone();
        let shared_queue = shared_queue_clone_info.clone();
        // 提取可以安全发送的数据（component 不能跨线程发送，使用 None）
        let message = options.message.to_string();
        let title = options.title.to_string();
        let duration = options.duration;
        let icon = options.icon.to_string();
        // 使用 Timer 延迟执行，避免在 invoke_info 的回调中立即访问 RefCell
        Timer::single_shot(Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let manager = NotificationManager::get(&ui);
                let notif = create_notification_with_defaults(
                    NotificationType::Info,
                    NotificationOptions {
                        message: message.into(),
                        title: title.into(),
                        duration,
                        icon: icon.into(),
                        component: crate::CustomComponentType::None,
                    },
                );
                let notification_id = notif.id;
                push_notification(&manager, notif.clone(), &shared_queue);

                // 设置自动关闭定时器（支持 hover 暂停）
                if notif.duration > 0 {
                    let duration = Duration::from_millis(notif.duration as u64);
                    start_auto_close_timer(
                        ui_weak.clone(),
                        notification_id,
                        duration,
                        shared_queue.clone(),
                    );
                }
            }
        });
    });

    let ui_weak_success = ui_weak.clone();
    let shared_queue_clone_success = shared_queue.clone();
    manager.on_success(move |options: NotificationOptions| {
        let ui_weak = ui_weak_success.clone();
        let shared_queue = shared_queue_clone_success.clone();
        // 提取可以安全发送的数据（component 不能跨线程发送，使用 None）
        let message = options.message.to_string();
        let title = options.title.to_string();
        let duration = options.duration;
        let icon = options.icon.to_string();
        // 使用 Timer 延迟执行，避免在 invoke_success 的回调中立即访问 RefCell
        Timer::single_shot(Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let manager = NotificationManager::get(&ui);
                let notif = create_notification_with_defaults(
                    NotificationType::Success,
                    NotificationOptions {
                        message: message.into(),
                        title: title.into(),
                        duration,
                        icon: icon.into(),
                        component: crate::CustomComponentType::None,
                    },
                );
                let notification_id = notif.id;
                push_notification(&manager, notif.clone(), &shared_queue);

                if notif.duration > 0 {
                    let duration = Duration::from_millis(notif.duration as u64);
                    start_auto_close_timer(
                        ui_weak.clone(),
                        notification_id,
                        duration,
                        shared_queue.clone(),
                    );
                }
            }
        });
    });

    let ui_weak_warning = ui_weak.clone();
    let shared_queue_clone_warning = shared_queue.clone();
    manager.on_warn(move |options: NotificationOptions| {
        if let Some(ui) = ui_weak_warning.upgrade() {
            let manager = NotificationManager::get(&ui);
            let notif = create_notification_with_defaults(NotificationType::Warning, options);
            let notification_id = notif.id;
            push_notification(&manager, notif.clone(), &shared_queue_clone_warning);

            if notif.duration > 0 {
                let duration = Duration::from_millis(notif.duration as u64);
                start_auto_close_timer(
                    ui_weak_warning.clone(),
                    notification_id,
                    duration,
                    shared_queue_clone_warning.clone(),
                );
            }
        }
    });

    let ui_weak_error = ui_weak.clone();
    let shared_queue_clone_error = shared_queue.clone();
    manager.on_error(move |options: NotificationOptions| {
        let ui_weak = ui_weak_error.clone();
        let shared_queue = shared_queue_clone_error.clone();
        // 提取可以安全发送的数据（component 不能跨线程发送，使用 None）
        let message = options.message.to_string();
        let title = options.title.to_string();
        let duration = options.duration;
        let icon = options.icon.to_string();
        // 使用 Timer 延迟执行，避免在 invoke_error 的回调中立即访问 RefCell
        Timer::single_shot(Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let manager = NotificationManager::get(&ui);
                let notif = create_notification_with_defaults(
                    NotificationType::Error,
                    NotificationOptions {
                        message: message.into(),
                        title: title.into(),
                        duration,
                        icon: icon.into(),
                        component: crate::CustomComponentType::None,
                    },
                );
                let notification_id = notif.id;
                push_notification(&manager, notif.clone(), &shared_queue);

                if notif.duration > 0 {
                    let duration = Duration::from_millis(notif.duration as u64);
                    start_auto_close_timer(
                        ui_weak.clone(),
                        notification_id,
                        duration,
                        shared_queue.clone(),
                    );
                }
            }
        });
    });

    let ui_weak_show = ui_weak.clone();
    let shared_queue_clone_show = shared_queue.clone();
    manager.on_show(move |type_, options: NotificationOptions| {
        if let Some(ui) = ui_weak_show.upgrade() {
            let manager = NotificationManager::get(&ui);
            let notif = create_notification_with_defaults(type_, options);
            let notification_id = notif.id;
            push_notification(&manager, notif.clone(), &shared_queue_clone_show);

            if notif.duration > 0 {
                let duration = Duration::from_millis(notif.duration as u64);
                start_auto_close_timer(
                    ui_weak_show.clone(),
                    notification_id,
                    duration,
                    shared_queue_clone_show.clone(),
                );
            }
        }
    });

    // push 回调：添加消息 + 启动定时器
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_push = ui_weak.clone();
    manager.on_push(move |mut notification: Notification| {
        let ui_weak = ui_weak_push.clone();
        let shared_queue = shared_queue_clone.clone();

        // 如果通知没有ID（ID为0），生成一个新的唯一ID
        if notification.id == 0 {
            notification.id = next_notification_id();
        }
        let notification_id = notification.id;

        if let Some(ui) = ui_weak.upgrade() {
            let manager = NotificationManager::get(&ui);
            // 先收集所有数据到一个 Vec，避免持有 queue_model 的借用
            let mut items = Vec::new();
            {
                let queue_model: ModelRc<Notification> = manager.get_queue();
                let row_count = queue_model.row_count();
                for i in 0..row_count {
                    if let Some(item) = queue_model.row_data(i) {
                        items.push(item);
                    }
                }
            } // queue_model 在这里被 drop，释放所有借用
            // 创建新的 VecModel，复制现有元素并添加新元素
            let mut new_vec = VecModel::default();
            for item in items {
                new_vec.push(item);
            }
            new_vec.push(notification.clone());
            manager.set_queue(ModelRc::new(new_vec));

            // 可选：更新共享队列
            if let Some(queue) = &shared_queue {
                if let Ok(mut q) = queue.write() {
                    q.0.push(notification.clone());
                }
            }

            // 自动关闭定时器（支持 hover 暂停）
            if notification.duration > 0 {
                let duration = Duration::from_millis(notification.duration as u64);
                start_auto_close_timer(
                    ui_weak.clone(),
                    notification_id,
                    duration,
                    shared_queue.clone(),
                );
            }
        }
    });

    // remove 回调
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove = ui_weak.clone();
    manager.on_remove(move |index| {
        if let Some(ui) = ui_weak_remove.upgrade() {
            let manager = NotificationManager::get(&ui);
            // 先收集所有数据到一个 Vec，避免持有 queue_model 的借用
            let mut items = Vec::new();
            {
                let queue_model: ModelRc<Notification> = manager.get_queue();
                let row_count = queue_model.row_count();
                for i in 0..row_count {
                    if i != index as usize {
                        if let Some(item) = queue_model.row_data(i) {
                            items.push(item);
                        }
                    }
                }
            } // queue_model 在这里被 drop，释放所有借用
            // 创建新的 VecModel
            let mut new_vec = VecModel::default();
            for item in items {
                new_vec.push(item);
            }
            manager.set_queue(ModelRc::new(new_vec));

            // 可选：同步共享队列
            if let Some(queue) = &shared_queue_clone {
                if let Ok(mut q) = queue.write() {
                    if index >= 0 && (index as usize) < q.0.len() {
                        // SharedVector 没有 remove 方法，需要创建新的向量
                        let mut new_vec = SharedVector::default();
                        for i in 0..q.0.len() {
                            if i != index as usize {
                                // 使用索引访问 SharedVector 元素
                                new_vec.push(q.0[i].clone());
                            }
                        }
                        q.0 = new_vec;
                    }
                }
            }
        }
    });

    // remove-by-id 回调（更可靠的删除方式）
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove_by_id = ui_weak.clone();
    manager.on_remove_by_id(move |id| {
        if let Some(ui) = ui_weak_remove_by_id.upgrade() {
            let manager = NotificationManager::get(&ui);
            remove_notification_by_id(&manager, id, &shared_queue_clone);
        }
    });

    // dismiss-by-id 回调（标记为已关闭，不删除）
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_dismiss = ui_weak.clone();
    manager.on_dismiss_by_id(move |id| {
        if let Some(ui) = ui_weak_dismiss.upgrade() {
            let manager = NotificationManager::get(&ui);
            dismiss_notification_by_id(&manager, id, &shared_queue_clone);
        }
    });

    // set-hovered 回调（设置通知的 hover 状态）
    // 当 hover 结束时，重新启动完整 duration 的计时器
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_set_hovered = ui_weak.clone();
    manager.on_set_hovered(move |id, hovered| {
        if let Some(ui) = ui_weak_set_hovered.upgrade() {
            let manager = NotificationManager::get(&ui);

            // 获取通知的 duration（在更新状态前获取）
            let duration = get_notification_duration(&manager, id);

            // 更新 hover 状态
            set_notification_hovered_by_id(&manager, id, hovered, &shared_queue_clone);

            // 如果 hover 结束（从 true 变为 false），重启完整 duration 的计时器
            if !hovered {
                if let Some(dur) = duration {
                    if dur > 0 {
                        start_auto_close_timer(
                            ui_weak_set_hovered.clone(),
                            id,
                            Duration::from_millis(dur as u64),
                            shared_queue_clone.clone(),
                        );
                    }
                }
            }
        }
    });

    // remove-all 回调
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove_all = ui_weak.clone();
    manager.on_remove_all(move || {
        if let Some(ui) = ui_weak_remove_all.upgrade() {
            let manager = NotificationManager::get(&ui);
            // 直接清空队列
            manager.set_queue(ModelRc::new(VecModel::default()));

            // 可选：清空共享队列
            if let Some(queue) = &shared_queue_clone {
                if let Ok(mut q) = queue.write() {
                    q.0.clear();
                }
            }
        }
    });

    // clear 回调
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_clear = ui_weak.clone();
    manager.on_clear(move || {
        if let Some(ui) = ui_weak_clear.upgrade() {
            let manager = NotificationManager::get(&ui);
            manager.set_queue(ModelRc::new(VecModel::default()));

            // 可选：清空共享队列
            if let Some(queue) = &shared_queue_clone {
                if let Ok(mut q) = queue.write() {
                    q.0.clear();
                }
            }
        }
    });
}
