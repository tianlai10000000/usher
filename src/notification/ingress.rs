//! Bounded UI notification ingress, worker publication, and completion ownership.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use super::{
    NotificationType, SharedQueue, default_title_key, get_default_icon, push_notification,
    start_auto_close_timer,
};
use crate::message_center::MessageCommandWorker;
use crate::{MainWindow, Notification, NotificationDelivery, NotificationManager, i18n};
use slint::{ComponentHandle, Global, Model, ModelRc, Timer, VecModel};
use usher_messages::{MessageCenter, MessageDraft, MessageSeverity, NativeDelivery};

const MAX_PENDING_NOTIFICATIONS: usize = 256;
const MAX_PENDING_NOTIFICATION_BYTES: usize = 1024 * 1024;
static PENDING_NOTIFICATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);
thread_local! {
    // Images and models remain on the event loop while only the draft enters
    // the command worker. Completion takes the pending row exactly once.
    static PENDING_NOTIFICATIONS: RefCell<HashMap<u64, PendingNotification>> = RefCell::new(HashMap::new());
    static PENDING_NOTIFICATION_IDS: RefCell<HashMap<i32, u64>> = RefCell::new(HashMap::new());
    static PENDING_NOTIFICATION_BYTES: Cell<usize> = const { Cell::new(0) };
    // Overload keeps one bounded latest fallback and one UI timer, rather than
    // retaining a new Notification in one timer closure per rejected attempt.
    static FALLBACK_NOTIFICATION: RefCell<Option<PendingNotification>> = const { RefCell::new(None) };
}

struct PendingNotification {
    ticket: u64,
    notification: Notification,
    shared_queue: Option<Arc<RwLock<SharedQueue>>>,
    bytes: usize,
}

fn pending_notification_bytes(notification: &Notification) -> usize {
    let image = notification.icon.size();
    let mut bytes = std::mem::size_of::<PendingNotification>()
        .saturating_add(
            [
                &notification.title,
                &notification.message,
                &notification.source,
                &notification.category,
                &notification.tag,
            ]
            .into_iter()
            .map(|value| value.len())
            .sum::<usize>(),
        )
        .saturating_add(
            (image.width as usize)
                .saturating_mul(image.height as usize)
                .saturating_mul(4),
        )
        .saturating_add(
            notification
                .actions
                .row_count()
                .saturating_mul(std::mem::size_of::<slint::SharedString>()),
        );
    for action in notification.actions.iter() {
        if bytes > MAX_PENDING_NOTIFICATION_BYTES {
            break;
        }
        bytes = bytes.saturating_add(action.len());
    }
    bytes
}

fn take_pending_notification(ticket: u64) -> Option<PendingNotification> {
    let pending = PENDING_NOTIFICATIONS.with(|pending| pending.borrow_mut().remove(&ticket))?;
    clear_latest_pending(&pending);
    PENDING_NOTIFICATION_BYTES.with(|bytes| bytes.set(bytes.get().saturating_sub(pending.bytes)));
    Some(pending)
}

fn clear_latest_pending(pending: &PendingNotification) {
    PENDING_NOTIFICATION_IDS.with(|ids| {
        let mut ids = ids.borrow_mut();
        if ids.get(&pending.notification.id) == Some(&pending.ticket) {
            ids.remove(&pending.notification.id);
        }
    });
}

fn is_latest_pending(pending: &PendingNotification) -> bool {
    PENDING_NOTIFICATION_IDS
        .with(|ids| ids.borrow().get(&pending.notification.id) == Some(&pending.ticket))
}

pub(super) fn clear_pending_notifications() {
    PENDING_NOTIFICATIONS.with(|pending| pending.borrow_mut().clear());
    PENDING_NOTIFICATION_IDS.with(|ids| ids.borrow_mut().clear());
    PENDING_NOTIFICATION_BYTES.with(|bytes| bytes.set(0));
    FALLBACK_NOTIFICATION.with(|fallback| fallback.borrow_mut().take());
}

fn defer_overload_banner(ui: &MainWindow, mut pending: PendingNotification) {
    let notification = &mut pending.notification;
    notification.title = notification
        .title
        .chars()
        .take(200)
        .collect::<String>()
        .into();
    notification.message = notification
        .message
        .chars()
        .take(4096)
        .collect::<String>()
        .into();
    notification.source = notification
        .source
        .chars()
        .take(100)
        .collect::<String>()
        .into();
    notification.category = notification
        .category
        .chars()
        .take(64)
        .collect::<String>()
        .into();
    notification.tag = notification
        .tag
        .chars()
        .take(512)
        .collect::<String>()
        .into();
    notification.actions = ModelRc::new(VecModel::from(
        notification
            .actions
            .iter()
            .take(8)
            .map(|action| action.chars().take(512).collect::<String>().into())
            .collect::<Vec<slint::SharedString>>(),
    ));
    notification.icon = get_default_icon(notification.r#type);
    let id = notification.id;
    let ticket = pending.ticket;
    let previous = FALLBACK_NOTIFICATION.with(|fallback| fallback.borrow_mut().replace(pending));
    if let Some(previous) = previous.as_ref() {
        clear_latest_pending(previous);
    }
    PENDING_NOTIFICATION_IDS.with(|ids| ids.borrow_mut().insert(id, ticket));
    let already_scheduled = previous.is_some();
    if already_scheduled {
        return;
    }
    let weak = ui.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        let pending = FALLBACK_NOTIFICATION.with(|fallback| fallback.borrow_mut().take());
        if let Some(ui) = weak.upgrade()
            && let Some(pending) = pending
        {
            let latest = is_latest_pending(&pending);
            clear_latest_pending(&pending);
            if latest {
                present_pending_notification(&ui, pending);
            }
        }
    });
}

fn notification_draft(notification: &Notification) -> MessageDraft {
    let severity = match notification.r#type {
        NotificationType::Info => MessageSeverity::Info,
        NotificationType::Success => MessageSeverity::Success,
        NotificationType::Warning => MessageSeverity::Warning,
        NotificationType::Error => MessageSeverity::Error,
    };
    let fallback_title = i18n::tr(default_title_key(notification.r#type));
    let title = if notification.title.is_empty() {
        fallback_title.as_ref()
    } else {
        notification.title.as_str()
    };
    let native_delivery = match notification.native_delivery {
        NotificationDelivery::WhenUnfocused => NativeDelivery::WhenUnfocused,
        NotificationDelivery::Never => NativeDelivery::Never,
        NotificationDelivery::Always => NativeDelivery::Always,
    };
    MessageDraft::new(title, notification.message.as_str())
        .source(notification.source.as_str())
        .category(notification.category.as_str())
        .severity(severity)
        .native_delivery(native_delivery)
}

pub(super) fn enqueue_notification(
    ui: &MainWindow,
    mut notification: Notification,
    shared_queue: Option<Arc<RwLock<SharedQueue>>>,
    center: &MessageCenter,
    commands: &MessageCommandWorker,
) {
    let ticket = PENDING_NOTIFICATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // Freeze the bounded action list before retaining it; callers may keep and
    // mutate their original ModelRc after this callback returns.
    if pending_notification_bytes(&notification) <= MAX_PENDING_NOTIFICATION_BYTES {
        notification.actions = ModelRc::new(VecModel::from(
            notification
                .actions
                .iter()
                .take(notification.actions.row_count())
                .collect::<Vec<_>>(),
        ));
    }
    let bytes = pending_notification_bytes(&notification);
    let pending = PendingNotification {
        ticket,
        notification,
        shared_queue,
        bytes,
    };
    let fits = PENDING_NOTIFICATIONS
        .with(|pending| pending.borrow().len() < MAX_PENDING_NOTIFICATIONS)
        && PENDING_NOTIFICATION_BYTES
            .with(|current| bytes <= MAX_PENDING_NOTIFICATION_BYTES.saturating_sub(current.get()));
    if !fits {
        log::error!("[Messages] Notification UI completion entry/byte budget exhausted");
        defer_overload_banner(ui, pending);
        return;
    }
    let mut draft = notification_draft(&pending.notification);
    if draft.native_delivery == NativeDelivery::WhenUnfocused {
        draft.native_delivery = if center.application_active() {
            NativeDelivery::Never
        } else {
            NativeDelivery::Always
        };
    }
    PENDING_NOTIFICATION_IDS.with(|ids| ids.borrow_mut().insert(pending.notification.id, ticket));
    PENDING_NOTIFICATION_BYTES.with(|current| current.set(current.get() + bytes));
    PENDING_NOTIFICATIONS.with(|rows| {
        rows.borrow_mut().insert(ticket, pending);
    });
    let weak = ui.as_weak();
    let completion_center = center.clone();
    let admitted = commands.publish(draft, move |result| {
        let message_id = match result {
            Ok(message) => Some(message.id),
            Err(error) => {
                log::error!("[Messages] Failed to record application notification: {error}");
                None
            }
        };
        if let Err(error) = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                finish_pending_notification(&ui, &completion_center, ticket, message_id);
            }
        }) {
            log::debug!("[Messages] Notification completion dropped during shutdown: {error}");
        }
    });
    if let Err(error) = admitted {
        log::error!("[Messages] Notification admission failed: {error}");
        let weak = ui.as_weak();
        let center = center.clone();
        // Retain the pre-existing Banner fallback when durable admission fails.
        // Defer queue mutation so nested Slint convenience callbacks can unwind.
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                finish_pending_notification(&ui, &center, ticket, None);
            }
        });
    }
}

fn finish_pending_notification(
    ui: &MainWindow,
    center: &MessageCenter,
    ticket: u64,
    message_id: Option<String>,
) {
    let latest = PENDING_NOTIFICATIONS
        .with(|pending| pending.borrow().get(&ticket).is_some_and(is_latest_pending));
    let Some(mut pending) = take_pending_notification(ticket) else {
        return;
    };
    if !latest {
        return;
    }
    if let Some(message_id) = message_id {
        if !center
            .snapshot()
            .messages
            .iter()
            .any(|message| message.id == message_id)
        {
            return; // A later accepted delete/clear already removed this row.
        }
        pending.notification.message_id = message_id.into();
    }
    present_pending_notification(ui, pending);
}

fn present_pending_notification(ui: &MainWindow, pending: PendingNotification) {
    let duration = pending.notification.duration;
    let id = pending.notification.id;
    push_notification(
        &NotificationManager::get(ui),
        pending.notification,
        &pending.shared_queue,
    );
    if duration > 0 {
        start_auto_close_timer(
            ui.as_weak(),
            id,
            Duration::from_millis(duration as u64),
            pending.shared_queue,
        );
    }
}

#[cfg(test)]
#[path = "ingress_tests.rs"]
mod tests;
