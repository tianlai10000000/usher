//! Slint bridge for the independently compiled message-center page.

use std::sync::{Arc, Mutex};

use chrono::Local;
use slint::{ComponentFactory, ComponentHandle, Global, Model, ModelRc, VecModel};
use usher_messages::{
    AppState as MessagesAppState, GitHubMessagesPage, MessageCenter, MessageLevel, MessageListItem,
    MessageSeverity, MessageSnapshot, Subscription,
};

use crate::{AppState, MainWindow};

#[derive(Clone)]
pub(crate) struct MessagePageBridge {
    center: MessageCenter,
    page: Arc<Mutex<Option<slint::Weak<GitHubMessagesPage>>>>,
    subscription: Arc<Mutex<Option<Subscription>>>,
}

impl MessagePageBridge {
    pub(crate) fn new(center: MessageCenter) -> Self {
        Self {
            center,
            page: Arc::new(Mutex::new(None)),
            subscription: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn install(&self, window: &MainWindow) {
        let page_slot = self.page.clone();
        let center = self.center.clone();
        let window_weak = window.as_weak();
        window.set_messages_page_factory(ComponentFactory::new(move |_| {
            let window = window_weak.upgrade()?;
            let page = match GitHubMessagesPage::new() {
                Ok(page) => page,
                Err(error) => {
                    log::error!("[Messages] Failed to create isolated page: {error}");
                    return None;
                }
            };
            sync_theme(&window, &page);
            page.set_viewport_width(window.get_messages_page_viewport_width());
            page.set_viewport_height(window.get_messages_page_viewport_height());
            bind_page_callbacks(&center, &page);
            apply_page_snapshot(&page, center.snapshot());
            page.set_active(window.get_messages_page_active());
            *page_slot.lock().expect("message page bridge poisoned") = Some(page.as_weak());
            Some(page)
        }));

        let bridge = self.clone();
        window.on_messages_page_viewport_changed(move |width, height| {
            bridge.with_page(|page| {
                page.set_viewport_width(width);
                page.set_viewport_height(height);
            });
        });

        let bridge = self.clone();
        window.on_messages_page_active_changed(move |active| {
            bridge.set_active(active);
        });

        let page_slot = self.page.clone();
        let window_weak = window.as_weak();
        let subscription = self.center.subscribe(Arc::new(move |snapshot| {
            let page_slot = page_slot.clone();
            let window_weak = window_weak.clone();
            if let Err(error) = slint::invoke_from_event_loop(move || {
                let Some(window) = window_weak.upgrade() else {
                    return;
                };
                window.set_message_unread_count(saturating_i32(snapshot.unread_count));
                let page = page_slot
                    .lock()
                    .ok()
                    .and_then(|page| page.clone())
                    .and_then(|page| page.upgrade());
                if let Some(page) = page
                    && page.get_active()
                {
                    apply_page_snapshot(&page, snapshot);
                }
            }) {
                log::debug!("[Messages] UI snapshot dropped during shutdown: {error}");
            }
        }));
        *self
            .subscription
            .lock()
            .expect("message subscription poisoned") = Some(subscription);

        let snapshot = self.center.snapshot();
        window.set_message_unread_count(saturating_i32(snapshot.unread_count));
    }

    pub(crate) fn sync_theme(&self, mode: i32, effective_dark_mode: bool) {
        self.with_page(|page| {
            let state = MessagesAppState::get(page);
            state.set_theme_mode(mode);
            state.set_effective_dark_mode(effective_dark_mode);
        });
    }

    fn set_active(&self, active: bool) {
        if active {
            let snapshot = self.center.snapshot();
            self.with_page(|page| {
                // Rehydrate from the authoritative current snapshot before the
                // conditional page tree is instantiated. Messages published
                // while another route was visible are therefore included.
                apply_page_snapshot(page, snapshot);
                page.set_active(true);
                log::debug!(
                    "[Messages][Lifecycle] active=true projected_rows={}",
                    page.get_messages().row_count()
                );
            });
        } else {
            self.with_page(|page| {
                page.set_active(false);
                release_page_rows(page);
                log::debug!("[Messages][Lifecycle] active=false projected_rows=0");
            });
        }
    }

    fn with_page(&self, update: impl FnOnce(&GitHubMessagesPage)) {
        let page = self
            .page
            .lock()
            .ok()
            .and_then(|page| page.clone())
            .and_then(|page| page.upgrade());
        if let Some(page) = page {
            update(&page);
        }
    }
}

fn release_page_rows(page: &GitHubMessagesPage) {
    let current = page.get_messages();
    if let Some(model) = current.as_any().downcast_ref::<VecModel<MessageListItem>>() {
        model.set_vec(Vec::new());
    } else {
        page.set_messages(ModelRc::new(VecModel::default()));
    }
}

fn bind_page_callbacks(center: &MessageCenter, page: &GitHubMessagesPage) {
    let center_for_read = center.clone();
    page.on_set_read(move |id, read| {
        if let Err(error) = center_for_read.set_read(id.as_str(), read) {
            log::error!("[Messages] Failed to update read state: {error}");
        }
    });

    let center_for_all = center.clone();
    page.on_mark_all_read(move || {
        if let Err(error) = center_for_all.mark_all_read() {
            log::error!("[Messages] Failed to mark all as read: {error}");
        }
    });

    let center_for_delete = center.clone();
    page.on_delete_message(move |id| {
        if let Err(error) = center_for_delete.remove(id.as_str()) {
            log::error!("[Messages] Failed to delete notification: {error}");
        }
    });

    let center_for_clear = center.clone();
    page.on_clear_messages(move || {
        if let Err(error) = center_for_clear.clear() {
            log::error!("[Messages] Failed to clear notifications: {error}");
        }
    });

    page.on_activate_target(|target| {
        // Target dispatch is intentionally open for feature-owned resolvers.
        // The first version records generic notifications without deep links.
        log::debug!("[Messages] No route resolver registered for target '{target}'");
    });
}

fn sync_theme(window: &MainWindow, page: &GitHubMessagesPage) {
    let source = AppState::get(window);
    let target = MessagesAppState::get(page);
    target.set_theme_mode(source.get_theme_mode());
    target.set_effective_dark_mode(source.get_effective_dark_mode());
}

fn apply_page_snapshot(page: &GitHubMessagesPage, snapshot: MessageSnapshot) {
    let rows = snapshot
        .messages
        .iter()
        .cloned()
        .map(|message| MessageListItem {
            id: message.id.into(),
            title: message.title.into(),
            body: message.body.into(),
            source: message.source.into(),
            category_label: category_label(&message.category).into(),
            category: message.category.into(),
            created_at: message
                .created_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
                .into(),
            level: match message.severity {
                MessageSeverity::Info => MessageLevel::Info,
                MessageSeverity::Success => MessageLevel::Success,
                MessageSeverity::Warning => MessageLevel::Warning,
                MessageSeverity::Error => MessageLevel::Error,
            },
            read: message.read,
            target: message.target.unwrap_or_default().into(),
        })
        .collect::<Vec<_>>();
    let total = rows.len();
    let unread = snapshot.unread_count.min(total);
    let read = total.saturating_sub(unread);
    // Push filter badges as ints before the model mutates. Tab labels bind to
    // these counts through separate Text nodes; string-concat captions and
    // finished "All (N)" strings both freeze after first paint on this page.
    page.set_total_count(saturating_i32(total));
    page.set_unread_count(saturating_i32(unread));
    page.set_read_count(saturating_i32(read));
    // Header sentence is still a complete Rust string — int concat in Slint
    // freezes the same way on this ComponentFactory page.
    page.set_status_description(status_description(unread).into());
    // Keep the VecModel identity across read-state updates. Replacing ModelRc
    // remounts every All-tab row and makes Slint reshuffle scroll/order so a
    // body click looks like a delete or reorder even though storage is stable.
    let current = page.get_messages();
    if let Some(model) = current.as_any().downcast_ref::<VecModel<MessageListItem>>() {
        let same_identity = model.row_count() == rows.len()
            && (0..rows.len()).all(|index| {
                model
                    .row_data(index)
                    .is_some_and(|existing| existing.id == rows[index].id)
            });
        if same_identity {
            for (index, row) in rows.into_iter().enumerate() {
                let unchanged = model
                    .row_data(index)
                    .is_some_and(|existing| message_row_equivalent(&existing, &row));
                if !unchanged {
                    model.set_row_data(index, row);
                }
            }
            return;
        }
        model.set_vec(rows);
        return;
    }
    page.set_messages(ModelRc::new(VecModel::from(rows)));
}

fn message_row_equivalent(left: &MessageListItem, right: &MessageListItem) -> bool {
    left.id == right.id
        && left.title == right.title
        && left.body == right.body
        && left.source == right.source
        && left.category == right.category
        && left.category_label == right.category_label
        && left.created_at == right.created_at
        && left.level == right.level
        && left.read == right.read
        && left.target == right.target
}

fn saturating_i32(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn status_description(unread: usize) -> String {
    match unread {
        0 => "You're all caught up.".to_string(),
        1 => "1 unread notification".to_string(),
        n => format!("{n} unread notifications"),
    }
}

fn category_label(category: &str) -> String {
    match category {
        usher_messages::categories::GENERAL => "General".to_string(),
        usher_messages::categories::WEBHOOK => "Webhook".to_string(),
        usher_messages::categories::SOFTWARE_UPDATE => "Software update".to_string(),
        usher_messages::categories::CD_WORKFLOW => "CD workflow".to_string(),
        other => other
            .split(['_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::{category_label, message_row_equivalent, status_description};
    use usher_messages::{MessageLevel, MessageListItem};

    #[test]
    fn category_labels_cover_built_in_and_extension_keys() {
        assert_eq!(category_label("software_update"), "Software update");
        assert_eq!(category_label("security-alert"), "Security Alert");
    }

    #[test]
    fn status_description_matches_unread_count() {
        assert_eq!(status_description(0), "You're all caught up.");
        assert_eq!(status_description(1), "1 unread notification");
        assert_eq!(status_description(17), "17 unread notifications");
    }

    #[test]
    fn message_row_equivalence_tracks_read_state_without_identity_drift() {
        let unread = MessageListItem {
            id: "1".into(),
            title: "Webhook received".into(),
            body: "A webhook event is ready for review.".into(),
            source: "Webhook".into(),
            category: "webhook".into(),
            category_label: "Webhook".into(),
            created_at: "2026-08-04 15:07".into(),
            level: MessageLevel::Info,
            read: false,
            target: "".into(),
        };
        let mut read = unread.clone();
        read.read = true;
        assert!(message_row_equivalent(&unread, &unread));
        assert!(!message_row_equivalent(&unread, &read));
    }
}
