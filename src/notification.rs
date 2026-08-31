use slint::{ComponentHandle, Global, Model, ModelRc, SharedVector, Timer, VecModel, Weak};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicI32, AtomicU64, Ordering},
};
use std::time::Duration;

// Use types re-exported from crate root
use crate::{
    MainWindow, Notification, NotificationDelivery, NotificationIcons, NotificationManager,
    NotificationOptions, NotificationType, UpdateLogic,
};
use usher_messages::{
    MessageCenter, MessageDraft, MessageSeverity, NativeDelivery, Subscription, categories,
};

lazy_static::lazy_static! {
    // Keeps the Inbox→Banner reconciliation subscription alive for the process.
    static ref BANNER_MESSAGE_SYNC: Mutex<Option<Subscription>> = Mutex::new(None);
    static ref BANNER_MESSAGE_IDS: RwLock<HashSet<String>> = RwLock::new(HashSet::new());
}

// Global counter for generating unique IDs
static NOTIFICATION_ID_COUNTER: AtomicI32 = AtomicI32::new(1);
static NOTIFICATION_TIMER_GENERATION: AtomicU64 = AtomicU64::new(0);

// Global hover state tracking (avoid modifying Notification struct triggering re-render)
lazy_static::lazy_static! {
    static ref HOVER_STATE: RwLock<HashMap<i32, bool>> = RwLock::new(HashMap::new());
    static ref AUTO_CLOSE_GENERATIONS: Mutex<HashMap<i32, u64>> = Mutex::new(HashMap::new());
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

/// Visible Banner queue cap (VS Code-style). Queue order is newest-first:
/// index 0 is the visible head. Older tails are dropped from the transient
/// overlay only; durable Inbox rows remain via `record_message`.
const MAX_BANNER_QUEUE: usize = 3;
const DISMISS_ANIMATION_DURATION: Duration = Duration::from_millis(180);

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
    if let Some(ui_weak) = UI_WEAK.read().ok().and_then(|guard| guard.clone())
        && let Some(ui) = ui_weak.upgrade()
    {
        let icons = NotificationIcons::get(&ui);
        return match notification_type {
            NotificationType::Info => icons.get_info(),
            NotificationType::Success => icons.get_success(),
            NotificationType::Warning => icons.get_warning(),
            NotificationType::Error => icons.get_error(),
        };
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
        message_id: Default::default(),
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

thread_local! {
    static BANNER_QUEUE_MODEL: RefCell<Option<Rc<VecModel<Notification>>>> = const { RefCell::new(None) };
}

fn banner_queue_model(manager: &NotificationManager) -> Rc<VecModel<Notification>> {
    BANNER_QUEUE_MODEL.with(|slot| {
        if let Some(model) = slot.borrow().as_ref() {
            return model.clone();
        }
        let model = Rc::new(VecModel::default());
        manager.set_queue(ModelRc::from(model.clone()));
        *slot.borrow_mut() = Some(model.clone());
        model
    })
}

fn queue_snapshot(model: &VecModel<Notification>) -> Vec<Notification> {
    (0..model.row_count())
        .filter_map(|index| model.row_data(index))
        .collect()
}

fn sync_banner_message_ids(model: &VecModel<Notification>) {
    let ids = (0..model.row_count())
        .filter_map(|index| model.row_data(index))
        .filter_map(|item| {
            let id = item.message_id.as_str();
            (!id.is_empty()).then(|| id.to_owned())
        })
        .collect();
    if let Ok(mut current) = BANNER_MESSAGE_IDS.write() {
        *current = ids;
    }
}

/// Newest-first enqueue with a hard cap. Returns retained items plus any
/// oldest tails that fell off the overlay.
#[cfg(test)]
fn prepend_capped<T>(mut items: Vec<T>, item: T, cap: usize) -> (Vec<T>, Vec<T>) {
    items.insert(0, item);
    let mut dropped = Vec::new();
    while items.len() > cap {
        if let Some(old) = items.pop() {
            dropped.push(old);
        }
    }
    (items, dropped)
}

// Helper function: add notification to queue
fn push_notification(
    manager: &NotificationManager,
    mut notification: Notification,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
    message_center: &MessageCenter,
) {
    if let Some(message_id) = record_message(message_center, &notification) {
        notification.message_id = message_id.into();
    }

    let model = banner_queue_model(manager);
    model.insert(0, notification.clone());
    let mut dropped = Vec::new();
    while model.row_count() > MAX_BANNER_QUEUE {
        let index = model.row_count() - 1;
        if let Some(item) = model.row_data(index) {
            dropped.push(item);
        }
        model.remove(index);
    }
    for dropped in &dropped {
        if let Ok(mut state) = HOVER_STATE.write() {
            state.remove(&dropped.id);
        }
        log::info!(
            "[Notification] banner queue capped at {MAX_BANNER_QUEUE}; dropped id={} message_id={}",
            dropped.id,
            dropped.message_id
        );
    }
    sync_banner_message_ids(&model);

    let items = queue_snapshot(&model);

    // Optional: update shared queue (same cap)
    if let Some(queue) = shared_queue
        && let Ok(mut q) = queue.write()
    {
        let mut next = SharedVector::default();
        for item in &items {
            next.push(item.clone());
        }
        q.0 = next;
    }
}

fn record_message(message_center: &MessageCenter, notification: &Notification) -> Option<String> {
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
    match message_center.publish(
        MessageDraft::new(title, notification.message.as_str())
            .source(notification.source.as_str())
            .category(notification.category.as_str())
            .severity(severity)
            .native_delivery(native_delivery),
    ) {
        Ok(message) => Some(message.id),
        Err(error) => {
            log::error!("[Messages] Failed to record application notification: {error}");
            None
        }
    }
}

fn clear_banner_queue(
    manager: &NotificationManager,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    if let Ok(mut state) = HOVER_STATE.write() {
        state.clear();
    }
    AUTO_CLOSE_GENERATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    let model = banner_queue_model(manager);
    while model.row_count() > 0 {
        model.remove(model.row_count() - 1);
    }
    sync_banner_message_ids(&model);
    if let Some(queue) = shared_queue
        && let Ok(mut shared) = queue.write()
    {
        shared.0.clear();
    }
    log::info!("[Notification] dismissed all banners (Inbox unchanged)");
}

/// Drops queued banners whose durable Inbox rows were deleted or cleared.
/// Must run on the Slint event loop. Only Send data (alive ids) may cross
/// `invoke_from_event_loop` — the banner queue holds non-Send `Image` handles.
fn prune_banners_for_alive_messages(manager: &NotificationManager, alive: &HashSet<String>) {
    let model = banner_queue_model(manager);
    let mut removed = 0usize;
    for index in (0..model.row_count()).rev() {
        if let Some(item) = model.row_data(index) {
            let message_id = item.message_id.as_str();
            if !message_id.is_empty() && !alive.contains(message_id) {
                removed += 1;
                if let Ok(mut state) = HOVER_STATE.write() {
                    state.remove(&item.id);
                }
                AUTO_CLOSE_GENERATIONS
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&item.id);
                model.remove(index);
            }
        }
    }
    if removed == 0 {
        return;
    }
    sync_banner_message_ids(&model);

    log::info!(
        "[Messages] pruned {removed} banner(s) after Inbox change; remaining={}",
        model.row_count()
    );
}

// Helper function: remove notification by notification ID (exact match, avoid index misalignment issues)
fn remove_notification_by_id(
    manager: &NotificationManager,
    target_id: i32,
    shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    AUTO_CLOSE_GENERATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&target_id);
    // Clean up hover state
    if let Ok(mut state) = HOVER_STATE.write() {
        state.remove(&target_id);
    }

    let model = banner_queue_model(manager);
    if let Some(index) = (0..model.row_count()).find(|index| {
        model
            .row_data(*index)
            .is_some_and(|item| item.id == target_id)
    }) {
        model.remove(index);
        sync_banner_message_ids(&model);
    }

    // Optional: update shared queue
    if let Some(queue) = shared_queue
        && let Ok(mut q) = queue.write()
    {
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

// Helper function: mark notification as dismissed (don't remove from queue)
fn dismiss_notification_by_id(
    manager: &NotificationManager,
    target_id: i32,
    _shared_queue: &Option<Arc<RwLock<SharedQueue>>>,
) {
    let model = banner_queue_model(manager);
    for index in 0..model.row_count() {
        if let Some(mut item) = model.row_data(index)
            && item.id == target_id
        {
            item.dismissed = true;
            model.set_row_data(index, item);
            break;
        }
    }
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
        if let Some(item) = queue_model.row_data(i)
            && item.id == target_id
        {
            return true;
        }
    }
    false
}

// Helper function: get notification duration
fn get_notification_duration(manager: &NotificationManager, target_id: i32) -> Option<i32> {
    let queue_model: ModelRc<Notification> = manager.get_queue();
    let row_count = queue_model.row_count();
    for i in 0..row_count {
        if let Some(item) = queue_model.row_data(i)
            && item.id == target_id
        {
            return Some(item.duration);
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
    let generation = NOTIFICATION_TIMER_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    AUTO_CLOSE_GENERATIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(notification_id, generation);
    let ui_weak_clone = ui_weak.clone();

    Timer::single_shot(duration, move || {
        let current = AUTO_CLOSE_GENERATIONS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&notification_id)
            .copied();
        if current != Some(generation) {
            return;
        }
        if let Some(ui) = ui_weak_clone.upgrade() {
            let manager = NotificationManager::get(&ui);

            // Check if notification still exists
            if !notification_exists(&manager, notification_id) {
                return;
            }

            // Check if notification is in hover state
            if is_notification_hovered(&manager, notification_id) {
                // Hover exit starts a fresh full-duration generation. Do not
                // build a polling timer chain while the pointer is stationary.
            } else {
                // Not hovered, remove notification
                remove_notification_by_id(&manager, notification_id, &shared_queue);
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
            push_notification(
                &manager,
                notification.clone(),
                &shared_queue,
                &message_center_push,
            );

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
            let model = banner_queue_model(&manager);
            if index >= 0 && (index as usize) < model.row_count() {
                if let Some(item) = model.row_data(index as usize) {
                    if let Ok(mut state) = HOVER_STATE.write() {
                        state.remove(&item.id);
                    }
                    AUTO_CLOSE_GENERATIONS
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(&item.id);
                }
                model.remove(index as usize);
                sync_banner_message_ids(&model);
            }

            // Optional: sync shared queue
            if let Some(queue) = &shared_queue_clone
                && let Ok(mut q) = queue.write()
                && index >= 0
                && (index as usize) < q.0.len()
            {
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

            // Keep the row mounted for the declared Slint height animation,
            // then remove it from the queue.
            let ui_weak_for_remove = ui_weak.clone();
            let shared_queue_for_remove = shared_queue.clone();
            Timer::single_shot(DISMISS_ANIMATION_DURATION, move || {
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
            if !hovered
                && let Some(dur) = duration
                && dur > 0
            {
                start_auto_close_timer(
                    ui_weak_set_hovered.clone(),
                    id,
                    Duration::from_millis(dur as u64),
                    shared_queue_clone.clone(),
                );
            }
        }
    });

    // remove-all / clear: dismiss the transient Banner queue only. Durable
    // Inbox rows are intentionally preserved (opposite of Inbox Clear all).
    let shared_queue_clone = shared_queue.clone();
    let ui_weak_remove_all = ui_weak.clone();
    manager.on_remove_all(move || {
        if let Some(ui) = ui_weak_remove_all.upgrade() {
            let manager = NotificationManager::get(&ui);
            clear_banner_queue(&manager, &shared_queue_clone);
        }
    });

    let shared_queue_clone = shared_queue.clone();
    let ui_weak_clear = ui_weak.clone();
    manager.on_clear(move || {
        if let Some(ui) = ui_weak_clear.upgrade() {
            let manager = NotificationManager::get(&ui);
            clear_banner_queue(&manager, &shared_queue_clone);
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
                if let Some(notification) = queue_model.row_data(i)
                    && notification.id == notification_id
                {
                    // Get action text from actions array
                    if let Some(text) = notification.actions.row_data(action_index as usize) {
                        action_text = text.to_string();
                    }
                    // Get tag for callback identification
                    tag = notification.tag.to_string();
                    break;
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
            Timer::single_shot(DISMISS_ANIMATION_DURATION, move || {
                if let Some(ui) = ui_weak_for_remove.upgrade() {
                    let manager = NotificationManager::get(&ui);
                    remove_notification_by_id(&manager, notification_id, &shared_queue_for_remove);
                }
            });
        }
    });

    // Inbox delete/clear is authoritative for mirrored banners. Closing a banner
    // still never deletes the durable message (handled only in dismiss paths).
    // Clone before later callbacks move `ui_weak`.
    let ui_weak_sync = ui.as_weak();
    let subscription = message_center.subscribe(Arc::new(move |snapshot| {
        let ui_weak = ui_weak_sync.clone();
        let mounted = BANNER_MESSAGE_IDS
            .read()
            .map(|ids| ids.clone())
            .unwrap_or_default();
        let alive = snapshot
            .messages
            .iter()
            .filter(|message| mounted.contains(message.id.as_str()))
            .map(|message| message.id.clone())
            .collect::<HashSet<_>>();
        if let Err(error) = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let manager = NotificationManager::get(&ui);
            prune_banners_for_alive_messages(&manager, &alive);
        }) {
            log::debug!("[Messages] Banner sync dropped during shutdown: {error}");
        }
    }));
    if let Ok(mut guard) = BANNER_MESSAGE_SYNC.lock() {
        *guard = Some(subscription);
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_BANNER_QUEUE, prepend_capped};

    #[test]
    fn banner_queue_keeps_newest_first_within_cap() {
        let (queue, dropped) = prepend_capped(vec![1, 2], 3, MAX_BANNER_QUEUE);
        assert_eq!(queue, vec![3, 1, 2]);
        assert!(dropped.is_empty());

        let (queue, dropped) = prepend_capped(queue, 4, MAX_BANNER_QUEUE);
        assert_eq!(queue, vec![4, 3, 1]);
        assert_eq!(dropped, vec![2]);

        let (queue, dropped) = prepend_capped(queue, 5, MAX_BANNER_QUEUE);
        assert_eq!(queue, vec![5, 4, 3]);
        assert_eq!(dropped, vec![1]);
    }
}
