use slint::{ComponentHandle, Global, Model, ModelRc, SharedVector, Timer, VecModel, Weak};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicI32, Ordering},
};
use std::time::Duration;

// Use types re-exported from crate root
use crate::{
    MainWindow, Notification, NotificationDelivery, NotificationIcons, NotificationManager,
    NotificationOptions, NotificationType, UpdateLogic,
};
use usher_messages::{MessageCenter, MessageDraft, MessageSeverity, NativeDelivery, categories};

// Global counter for generating unique IDs
static NOTIFICATION_ID_COUNTER: AtomicI32 = AtomicI32::new(0);

// Global hover state tracking (avoid modifying Notification struct triggering re-render)
lazy_static::lazy_static! {
    static ref HOVER_STATE: RwLock<HashMap<i32, bool>> = RwLock::new(HashMap::new());
}

// Generate next unique ID
fn next_notification_id() -> i32 {
    NOTIFICATION_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// Default value constants
const DEFAULT_DURATION_INFO: i32 = 3000;
const DEFAULT_DURATION_SUCCESS: i32 = 3000;
const DEFAULT_DURATION_WARNING: i32 = 3000;
const DEFAULT_DURATION_ERROR: i32 = 5000;
// const DEFAULT_DURATION_GENERAL: i32 = 3000;

// Default message constants
const DEFAULT_MESSAGE_INFO: &str = "Info";
const DEFAULT_MESSAGE_SUCCESS: &str = "Success";
const DEFAULT_MESSAGE_WARNING: &str = "Warning";
const DEFAULT_MESSAGE_ERROR: &str = "Error";

// Global UI reference (for getting embedded icons)
lazy_static::lazy_static! {
    static ref UI_WEAK: RwLock<Option<Weak<MainWindow>>> = RwLock::new(None);
}

// Helper function: get embedded default icon
fn get_default_icon(notification_type: NotificationType) -> slint::Image {
    // Try to get embedded icon from Slint global
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

    // If unable to get embedded icon, try loading from file system (fallback for development mode)
    let icon_path_str = match notification_type {
        NotificationType::Info => "ui/assets/icons/octicon-info-16.svg",
        NotificationType::Success => "ui/assets/icons/octicon-check-circle-16.svg",
        NotificationType::Warning => "ui/assets/icons/octicon-alert-16.svg",
        NotificationType::Error => "ui/assets/icons/octicon-stop-16.svg",
    };

    let icon_path = Path::new(icon_path_str);
    slint::Image::load_from_path(icon_path).unwrap_or_else(|_| slint::Image::default())
}

// Helper function: create notification with default values
// Extract values from NotificationOptions and apply defaults
fn create_notification_with_defaults(
    notification_type: NotificationType,
    options: NotificationOptions,
) -> Notification {
    let source = if options.source.is_empty() {
        "Usher".into()
    } else {
        options.source.clone()
    };
    let category = if options.category.is_empty() {
        match options.tag.as_str() {
            "auto-update" | "manual-update" | "software-update" => {
                categories::SOFTWARE_UPDATE.into()
            }
            _ => categories::GENERAL.into(),
        }
    } else {
        options.category.clone()
    };
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

    // If message is empty string, use default message
    let final_message = if options.message.is_empty() {
        slint::SharedString::from(default_message)
    } else {
        options.message
    };

    // duration handling logic:
    // - duration < 0: use default value
    // - duration == 0: permanent (no auto-close)
    // - duration > 0: use specified value
    let final_duration = if options.duration < 0 {
        default_duration
    } else {
        options.duration // Including 0 (permanent) and positive numbers (specified time)
    };

    // icon handling: prioritize user-provided icon, use default icon if empty
    // Check if user-provided icon is empty image
    // If user provided a valid icon, use user's icon; otherwise use default icon
    //
    // Note: In Slint, if user doesn't provide icon field, struct may use a special default value
    // This default value may not equal slint::Image::default(), so we need more lenient detection
    //
    // Strategy: Since we can't reliably detect empty images, we adopt a conservative strategy:
    // 1. If provided icon is empty image (detected via is_empty_image), use default icon
    // 2. Otherwise, use user-provided icon
    //
    // However, if user doesn't provide icon in Slint, struct may use a special default value
    // This value may not be empty image, but also not the icon user wants
    //
    // Solution: In Slint side, when user doesn't provide icon, should explicitly pass an empty image
    // Or in Rust side, we can try a more reliable method to detect empty images
    //
    // Current implementation: If provided icon is empty image, use default icon; otherwise use user-provided icon
    // If user doesn't provide icon in Slint, struct may use a special default value
    // This value may not be empty image, so we need to handle in Slint side, ensure empty image is passed when not provided
    // icon handling: use icon string
    // Logic:
    // - If icon is empty string, use default icon
    // - If icon is not empty, load icon from path
    //
    // Usage:
    // 1. Don't provide icon (use default icon):
    //    NotificationManager.show(NotificationType.Success, { message: "Hello" })
    //
    // 2. Explicitly provide icon (use custom icon):
    //    NotificationManager.show(NotificationType.Success, {
    //        message: "Hello",
    //        icon: "ui/assets/icons/error.svg"
    //    })
    let final_icon = if options.icon.is_empty() {
        // Use default icon
        get_default_icon(notification_type)
    } else {
        // Load custom icon from path
        let icon_path = Path::new(options.icon.as_str());
        slint::Image::load_from_path(icon_path).unwrap_or_else(|_| {
            // If loading fails, use default icon
            get_default_icon(notification_type)
        })
    };

    Notification {
        id: next_notification_id(),
        title: options.title.clone(), // title directly uses user-provided value, empty string means no title
        message: final_message,
        r#type: notification_type,
        duration: final_duration,
        icon: final_icon,
        custom_component: options.component,
        actions: options.actions.clone(), // Copy actions array
        tag: options.tag.clone(),         // Copy tag for callback identification
        source,
        category,
        native_delivery: options.native_delivery,
        dismissed: false, // Initially not dismissed
        hovered: false,   // Initially not hovered
    }
}

#[derive(Default, Clone)]
pub struct SharedQueue(pub SharedVector<Notification>);

// Helper function: add notification to queue
fn push_notification(
    manager: &NotificationManager,
    notification: Notification,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
    message_center: &MessageCenter,
) {
    // First collect all data into a Vec, avoid holding queue_model borrow
    let mut items = Vec::new();
    {
        let queue_model: ModelRc<Notification> = manager.get_queue();
        let row_count = queue_model.row_count();
        for i in 0..row_count {
            if let Some(item) = queue_model.row_data(i) {
                items.push(item);
            }
        }
    } // queue_model is dropped here, releasing all borrows

    // Create new VecModel, copy existing elements and add new element
    let new_vec = VecModel::default();
    for item in items {
        new_vec.push(item);
    }
    new_vec.push(notification.clone());
    manager.set_queue(ModelRc::new(new_vec));

    // Optional: update shared queue
    if let Some(queue) = shared_queue {
        if let Ok(mut q) = queue.write() {
            q.0.push(notification.clone());
        }
    }

    record_message(message_center, &notification);
}

fn record_message(message_center: &MessageCenter, notification: &Notification) {
    let severity = match notification.r#type {
        NotificationType::Info => MessageSeverity::Info,
        NotificationType::Success => MessageSeverity::Success,
        NotificationType::Warning => MessageSeverity::Warning,
        NotificationType::Error => MessageSeverity::Error,
    };
    let fallback_title = match notification.r#type {
        NotificationType::Info => "Information",
        NotificationType::Success => "Completed",
        NotificationType::Warning => "Attention",
        NotificationType::Error => "Failed",
    };
    let title = if notification.title.is_empty() {
        fallback_title
    } else {
        notification.title.as_str()
    };
    let native_delivery = match notification.native_delivery {
        NotificationDelivery::WhenUnfocused => NativeDelivery::WhenUnfocused,
        NotificationDelivery::Never => NativeDelivery::Never,
        NotificationDelivery::Always => NativeDelivery::Always,
    };
    if let Err(error) = message_center.publish(
        MessageDraft::new(title, notification.message.as_str())
            .source(notification.source.as_str())
            .category(notification.category.as_str())
            .severity(severity)
            .native_delivery(native_delivery),
    ) {
        log::error!("[Messages] Failed to record application notification: {error}");
    }
}

// Helper function: remove notification by notification ID (exact match, avoid index misalignment issues)
fn remove_notification_by_id(
    manager: &NotificationManager,
    target_id: i32,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    // Clean up hover state
    if let Ok(mut state) = HOVER_STATE.write() {
        state.remove(&target_id);
    }

    // First collect all data into a Vec, avoid holding queue_model borrow
    let mut items = Vec::new();
    {
        let queue_model: ModelRc<Notification> = manager.get_queue();
        let row_count = queue_model.row_count();
        for i in 0..row_count {
            if let Some(item) = queue_model.row_data(i) {
                // Exact match by ID, ensure each notification's timer only deletes itself
                if item.id != target_id {
                    items.push(item);
                }
            }
        }
    } // queue_model is dropped here, releasing all borrows

    // Create new VecModel, excluding notification with matching ID
    let new_vec = VecModel::default();
    for item in items {
        new_vec.push(item);
    }
    manager.set_queue(ModelRc::new(new_vec));

    // Optional: update shared queue
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

// Helper function: mark notification as dismissed (don't remove from queue)
fn dismiss_notification_by_id(
    manager: &NotificationManager,
    target_id: i32,
    _shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    // Collect all data, modify dismissed field of notification with matching ID
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

    // Create new VecModel
    let new_vec = VecModel::default();
    for item in items {
        new_vec.push(item);
    }
    manager.set_queue(ModelRc::new(new_vec));
}

// Helper function: set notification hover state (use global HashMap, avoid triggering UI re-render)
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

// Helper function: check if notification is in hover state (read from global HashMap)
fn is_notification_hovered(_manager: &NotificationManager, target_id: i32) -> bool {
    if let Ok(state) = HOVER_STATE.read() {
        state.get(&target_id).copied().unwrap_or(false)
    } else {
        false
    }
}

// Helper function: check if notification exists
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

// Helper function: get notification duration
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

// Start auto-close timer (supports hover pause)
// If notification is in hover state, timer will continue waiting after expiry
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

            // Check if notification still exists
            if !notification_exists(&manager, notification_id) {
                return;
            }

            // Check if notification is in hover state
            if is_notification_hovered(&manager, notification_id) {
                // If hovered, continue waiting (check every 200ms)
                start_auto_close_timer(
                    ui_weak,
                    notification_id,
                    Duration::from_millis(200),
                    shared_queue_clone,
                );
            } else {
                // Not hovered, remove notification
                remove_notification_by_id(&manager, notification_id, &shared_queue_clone);
            }
        }
    });
}

// Notification manager initialization and callback binding function
pub fn setup_notifications(
    ui: &MainWindow,
    shared_queue: Option<Arc<RwLock<SharedQueue>>>, // Optional: for multi-window sharing
    message_center: MessageCenter,
) {
    let ui_weak = ui.as_weak();

    // Store UI reference for getting embedded icons
    if let Ok(mut guard) = UI_WEAK.write() {
        *guard = Some(ui_weak.clone());
    }

    let manager = NotificationManager::get(ui);

    // Convenience method callbacks
    let ui_weak_info = ui_weak.clone();
    let shared_queue_clone_info = shared_queue.clone();
    let message_center_info = message_center.clone();
    manager.on_info(move |options: NotificationOptions| {
        let ui_weak = ui_weak_info.clone();
        let shared_queue = shared_queue_clone_info.clone();
        let message_center = message_center_info.clone();
        // Extract data that can be safely sent
        let message = options.message.to_string();
        let title = options.title.to_string();
        let duration = options.duration;
        let icon = options.icon.to_string();
        let component = options.component;
        let tag = options.tag.to_string();
        let source = options.source.to_string();
        let category = options.category.to_string();
        let native_delivery = options.native_delivery;
        // Convert actions ModelRc to Vec<String> for thread safety
        let actions_vec: Vec<String> = (0..options.actions.row_count())
            .filter_map(|i| options.actions.row_data(i).map(|s| s.to_string()))
            .collect();

        // Use Timer to delay execution, avoid immediately accessing RefCell in invoke_info callback
        Timer::single_shot(Duration::from_millis(10), move || {
            if let Some(ui) = ui_weak.upgrade() {
                let manager = NotificationManager::get(&ui);
                // Reconstruct actions as ModelRc
                let actions: ModelRc<slint::SharedString> =
                    ModelRc::new(std::rc::Rc::new(VecModel::from(
                        actions_vec
                            .iter()
                            .map(|s| slint::SharedString::from(s.as_str()))
                            .collect::<Vec<_>>(),
                    )));
                let notif = create_notification_with_defaults(
                    NotificationType::Info,
                    NotificationOptions {
                        message: message.into(),
                        title: title.into(),
                        duration,
                        icon: icon.into(),
                        component,
                        actions,
                        tag: tag.into(),
                        source: source.into(),
                        category: category.into(),
                        native_delivery,
                    },
                );
                let notification_id = notif.id;
                push_notification(&manager, notif.clone(), &shared_queue, &message_center);

                // Set auto-close timer (supports hover pause)
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
    let message_center_success = message_center.clone();
    manager.on_success(move |options: NotificationOptions| {
        let ui_weak = ui_weak_success.clone();
        let shared_queue = shared_queue_clone_success.clone();
        let message_center = message_center_success.clone();
        // Extract data that can be safely sent (component cannot be sent across threads, use None)
        let message = options.message.to_string();
        let title = options.title.to_string();
        let duration = options.duration;
        let icon = options.icon.to_string();
        let source = options.source.to_string();
        let category = options.category.to_string();
        let native_delivery = options.native_delivery;
        // Use Timer to delay execution, avoid immediately accessing RefCell in invoke_success callback
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
                        actions: Default::default(),
                        tag: Default::default(),
                        source: source.into(),
                        category: category.into(),
                        native_delivery,
                    },
                );
                let notification_id = notif.id;
                push_notification(&manager, notif.clone(), &shared_queue, &message_center);

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
    let message_center_warning = message_center.clone();
    manager.on_warn(move |options: NotificationOptions| {
        if let Some(ui) = ui_weak_warning.upgrade() {
            let manager = NotificationManager::get(&ui);
            let notif = create_notification_with_defaults(NotificationType::Warning, options);
            let notification_id = notif.id;
            push_notification(
                &manager,
                notif.clone(),
                &shared_queue_clone_warning,
                &message_center_warning,
            );

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
    let message_center_error = message_center.clone();
    manager.on_error(move |options: NotificationOptions| {
        let ui_weak = ui_weak_error.clone();
        let shared_queue = shared_queue_clone_error.clone();
        let message_center = message_center_error.clone();
        // Extract data that can be safely sent (component cannot be sent across threads, use None)
        let message = options.message.to_string();
        let title = options.title.to_string();
        let duration = options.duration;
        let icon = options.icon.to_string();
        let source = options.source.to_string();
        let category = options.category.to_string();
        let native_delivery = options.native_delivery;
        // Use Timer to delay execution, avoid immediately accessing RefCell in invoke_error callback
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
                        actions: Default::default(),
                        tag: Default::default(),
                        source: source.into(),
                        category: category.into(),
                        native_delivery,
                    },
                );
                let notification_id = notif.id;
                push_notification(&manager, notif.clone(), &shared_queue, &message_center);

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
    let message_center_show = message_center.clone();
    manager.on_show(move |type_, options: NotificationOptions| {
        if let Some(ui) = ui_weak_show.upgrade() {
            let manager = NotificationManager::get(&ui);
            let notif = create_notification_with_defaults(type_, options);
            let notification_id = notif.id;
            push_notification(
                &manager,
                notif.clone(),
                &shared_queue_clone_show,
                &message_center_show,
            );

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

    // push callback: add message + start timer
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_push = ui_weak.clone();
    let message_center_push = message_center.clone();
    manager.on_push(move |mut notification: Notification| {
        let ui_weak = ui_weak_push.clone();
        let shared_queue = shared_queue_clone.clone();

        // If notification has no ID (ID is 0), generate a new unique ID
        if notification.id == 0 {
            notification.id = next_notification_id();
        }
        let notification_id = notification.id;

        if let Some(ui) = ui_weak.upgrade() {
            let manager = NotificationManager::get(&ui);
            // First collect all data into a Vec, avoid holding queue_model borrow
            let mut items = Vec::new();
            {
                let queue_model: ModelRc<Notification> = manager.get_queue();
                let row_count = queue_model.row_count();
                for i in 0..row_count {
                    if let Some(item) = queue_model.row_data(i) {
                        items.push(item);
                    }
                }
            } // queue_model is dropped here, releasing all borrows
            // Create new VecModel, copy existing elements and add new element
            let new_vec = VecModel::default();
            for item in items {
                new_vec.push(item);
            }
            new_vec.push(notification.clone());
            manager.set_queue(ModelRc::new(new_vec));

            // Optional: update shared queue
            if let Some(queue) = &shared_queue {
                if let Ok(mut q) = queue.write() {
                    q.0.push(notification.clone());
                }
            }

            record_message(&message_center_push, &notification);

            // Auto-close timer (supports hover pause)
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

    // remove callback
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove = ui_weak.clone();
    manager.on_remove(move |index| {
        if let Some(ui) = ui_weak_remove.upgrade() {
            let manager = NotificationManager::get(&ui);
            // First collect all data into a Vec, avoid holding queue_model borrow
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
            } // queue_model is dropped here, releasing all borrows
            // Create new VecModel
            let new_vec = VecModel::default();
            for item in items {
                new_vec.push(item);
            }
            manager.set_queue(ModelRc::new(new_vec));

            // Optional: sync shared queue
            if let Some(queue) = &shared_queue_clone {
                if let Ok(mut q) = queue.write() {
                    if index >= 0 && (index as usize) < q.0.len() {
                        // SharedVector doesn't have remove method, need to create new vector
                        let mut new_vec = SharedVector::default();
                        for i in 0..q.0.len() {
                            if i != index as usize {
                                // Use index to access SharedVector elements
                                new_vec.push(q.0[i].clone());
                            }
                        }
                        q.0 = new_vec;
                    }
                }
            }
        }
    });

    // remove-by-id callback (more reliable deletion method)
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove_by_id = ui_weak.clone();
    manager.on_remove_by_id(move |id| {
        if let Some(ui) = ui_weak_remove_by_id.upgrade() {
            let manager = NotificationManager::get(&ui);
            remove_notification_by_id(&manager, id, &shared_queue_clone);
        }
    });

    // dismiss-by-id callback (mark as dismissed, then remove after animation delay)
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_dismiss = ui_weak.clone();
    manager.on_dismiss_by_id(move |id| {
        let ui_weak = ui_weak_dismiss.clone();
        let shared_queue = shared_queue_clone.clone();
        if let Some(ui) = ui_weak.upgrade() {
            let manager = NotificationManager::get(&ui);
            // First mark as dismissed (triggers UI animation)
            dismiss_notification_by_id(&manager, id, &shared_queue);

            // After animation delay, remove from queue to update count
            // Animation duration is typically around 200-300ms, use 350ms to be safe
            let ui_weak_for_remove = ui_weak.clone();
            let shared_queue_for_remove = shared_queue.clone();
            Timer::single_shot(Duration::from_millis(350), move || {
                if let Some(ui) = ui_weak_for_remove.upgrade() {
                    let manager = NotificationManager::get(&ui);
                    remove_notification_by_id(&manager, id, &shared_queue_for_remove);
                }
            });
        }
    });

    // set-hovered callback (set notification hover state)
    // When hover ends, restart full duration timer
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_set_hovered = ui_weak.clone();
    manager.on_set_hovered(move |id, hovered| {
        if let Some(ui) = ui_weak_set_hovered.upgrade() {
            let manager = NotificationManager::get(&ui);

            // Get notification duration (get before updating state)
            let duration = get_notification_duration(&manager, id);

            // Update hover state
            set_notification_hovered_by_id(&manager, id, hovered, &shared_queue_clone);

            // If hover ends (changed from true to false), restart full duration timer
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

    // remove-all callback
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove_all = ui_weak.clone();
    manager.on_remove_all(move || {
        if let Some(ui) = ui_weak_remove_all.upgrade() {
            let manager = NotificationManager::get(&ui);
            // Directly clear queue
            manager.set_queue(ModelRc::new(VecModel::default()));

            // Optional: clear shared queue
            if let Some(queue) = &shared_queue_clone {
                if let Ok(mut q) = queue.write() {
                    q.0.clear();
                }
            }
        }
    });

    // clear callback
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_clear = ui_weak.clone();
    manager.on_clear(move || {
        if let Some(ui) = ui_weak_clear.upgrade() {
            let manager = NotificationManager::get(&ui);
            manager.set_queue(ModelRc::new(VecModel::default()));

            // Optional: clear shared queue
            if let Some(queue) = &shared_queue_clone {
                if let Ok(mut q) = queue.write() {
                    q.0.clear();
                }
            }
        }
    });

    // action-clicked callback: handle ActionsNotify component button clicks
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_action = ui_weak.clone();
    manager.on_action_clicked(move |notification_id, action_index| {
        log::debug!(
            "[Notification] Action Clicked: notification_id={}, action_index={}",
            notification_id,
            action_index
        );

        // Get notification info for logging/handling
        if let Some(ui) = ui_weak_action.upgrade() {
            let manager = NotificationManager::get(&ui);

            // Find the notification to get action text and tag
            let queue_model: ModelRc<Notification> = manager.get_queue();
            let mut action_text = String::new();
            let mut tag = String::new();
            for i in 0..queue_model.row_count() {
                if let Some(notification) = queue_model.row_data(i) {
                    if notification.id == notification_id {
                        // Get action text from actions array
                        if let Some(text) = notification.actions.row_data(action_index as usize) {
                            action_text = text.to_string();
                        }
                        // Get tag for callback identification
                        tag = notification.tag.to_string();
                        break;
                    }
                }
            }

            log::debug!(
                "[Notification] Action '{}' clicked on notification {} (tag: '{}')",
                action_text,
                notification_id,
                tag
            );

            // Handle different actions based on tag
            // Users can add their own business logic here
            let ui_weak_for_action = ui_weak.clone();
            match tag.as_str() {
                "delete-file" => {
                    if action_index == 1 {
                        // User clicked "Delete" button
                        log::info!("[Notification] Delete file action triggered");
                        // Add your delete file logic here
                    }
                }
                "confirm-update" => {
                    if action_index == 1 {
                        // User clicked "Update" button
                        log::info!("[Notification] Update action triggered");
                        // Add your update logic here
                    }
                }
                "auto-update" => {
                    if action_index == 0 {
                        // User clicked "Cancel" button
                        log::info!("[Notification] Auto-update cancelled by user");
                        // Reset update state
                        if let Some(ui) = ui_weak_for_action.upgrade() {
                            let logic = ui.global::<UpdateLogic>();
                            logic.set_update_available(false);
                        }
                    } else if action_index == 1 {
                        // User clicked "Update Now" button
                        log::info!("[Notification] Auto-update: starting update...");
                        if let Some(ui) = ui_weak_for_action.upgrade() {
                            let logic = ui.global::<UpdateLogic>();
                            let new_version = logic.get_new_version().to_string();
                            let current_version = env!("CARGO_PKG_VERSION").to_string();

                            // Show update dialog to start download
                            ui.invoke_show_update_dialog(
                                new_version.into(),
                                current_version.into(),
                            );
                        }
                    }
                }
                "manual-update" => {
                    if action_index == 1 {
                        log::info!("[Notification] Manual update: starting update...");
                        if let Some(ui) = ui_weak_for_action.upgrade() {
                            ui.invoke_install();
                        }
                    }
                }
                // Add more tag handlers as needed
                _ => {
                    // Default: just log the action
                    log::debug!(
                        "[Notification] Unhandled tag '{}', action: '{}'",
                        tag,
                        action_text
                    );
                }
            }

            // Default behavior: close notification after action is clicked
            dismiss_notification_by_id(&manager, notification_id, &shared_queue_clone);

            // After animation delay, remove from queue to update count
            let ui_weak_for_remove = ui_weak_action.clone();
            let shared_queue_for_remove = shared_queue_clone.clone();
            Timer::single_shot(Duration::from_millis(350), move || {
                if let Some(ui) = ui_weak_for_remove.upgrade() {
                    let manager = NotificationManager::get(&ui);
                    remove_notification_by_id(&manager, notification_id, &shared_queue_for_remove);
                }
            });
        }
    });
}
