//! Slint bridge for the independently compiled message-center page.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::Local;
use slint::{ComponentFactory, ComponentHandle, Global, Model, ModelRc, VecModel};
use usher_messages::{
    AppState as MessagesAppState, GitHubMessagesPage, LanguageOption as MessagesLanguageOption,
    MESSAGE_PAGE_SIZE, MessageCenter, MessageLevel, MessageListItem, MessageReadFilter,
    MessageSeverity, MessageSnapshot, NotifyRustDesktopNotifier, Subscription,
};

use crate::{AppState, MainWindow, app_paths, app_runtime, startup};

struct LocalizedDesktopNotifier;
impl usher_messages::DesktopNotifier for LocalizedDesktopNotifier {
    fn show(&self, message: &usher_messages::Message) -> std::result::Result<(), String> {
        let mut display = message.clone();
        if let Some((title, body)) =
            crate::hooks::notification_copy(message, usher_i18n::active_locale().id)
        {
            display.title = title;
            display.body = body;
        }
        usher_messages::DesktopNotifier::show(&NotifyRustDesktopNotifier, &display)
    }
}

#[derive(Clone)]
pub(crate) struct MessagePageBridge {
    center: MessageCenter,
    page: Arc<Mutex<Option<slint::Weak<GitHubMessagesPage>>>>,
    subscription: Arc<Mutex<Option<Subscription>>>,
}

impl MessagePageBridge {
    pub(crate) fn open_deferred(
        timeline: &startup::StartupTimeline,
    ) -> Result<(MessageCenter, Self)> {
        let center = if let Some(path) = app_paths::message_store_path() {
            MessageCenter::open_deferred_with(path, Arc::new(LocalizedDesktopNotifier))
        } else {
            MessageCenter::open_default_deferred()
        }
        .context("Failed to initialize message center")?;
        timeline.checkpoint("message_center_ready");

        let center_for_hydration = center.clone();
        let timeline_for_messages = timeline.clone();
        startup::run_after_first_frame(move || {
            app_runtime::shared_runtime().spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    center_for_hydration.hydrate_from_storage()
                })
                .await;
                match result {
                    Ok(Ok(count)) => {
                        log::info!("[Messages] Deferred hydration loaded {count} messages");
                        timeline_for_messages.checkpoint("messages_hydrated");
                    }
                    Ok(Err(error)) => log::error!("[Messages] Deferred hydration failed: {error}"),
                    Err(error) => log::error!("[Messages] Hydration worker failed: {error}"),
                }
            });
        });

        Ok((center.clone(), Self::new(center)))
    }

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
            sync_language(&window, &page);
            page.set_viewport_width(window.get_messages_page_viewport_width());
            page.set_viewport_height(window.get_messages_page_viewport_height());
            bind_page_callbacks(&center, &page, &window);
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
        let deliver = app_runtime::latest_ui(
            window.as_weak(),
            move |window, snapshot: MessageSnapshot| {
                if std::env::var("USHER_P5_BENCHMARK").as_deref() == Ok("59") {
                    log::info!("[P5][59] inbox_ui unread={}", snapshot.unread_count);
                }
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
            },
        );
        let subscription = self.center.subscribe(Arc::new(deliver));
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

    pub(crate) fn sync_language(&self, preference: &str) {
        let snapshot = self.center.snapshot();
        self.with_page(|page| {
            sync_page_language(page, preference);
            if page.get_active() {
                apply_page_snapshot(page, snapshot);
            }
        });
    }

    fn set_active(&self, active: bool) {
        if active {
            let snapshot = self.center.snapshot();
            self.with_page(|page| {
                // Rehydrate from the authoritative current snapshot before the
                // stable page tree becomes visible. Messages published
                // while another route was visible are therefore included.
                apply_page_snapshot(page, snapshot);
                page.set_active(true);
                log::debug!(
                    "[Messages][Lifecycle] active=true projected_rows={}",
                    page.get_page_row_count()
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
    update_page_rows(page, Vec::new());
}

fn bind_page_callbacks(center: &MessageCenter, page: &GitHubMessagesPage, window: &MainWindow) {
    let center_for_filter = center.clone();
    let weak_page = page.as_weak();
    page.on_change_filter(move |filter| {
        if let Some(page) = weak_page.upgrade() {
            page.set_selected_filter(filter.clamp(0, 2));
            page.set_current_page(1);
            apply_page_snapshot(&page, center_for_filter.snapshot());
        }
    });

    let center_for_page = center.clone();
    let weak_page = page.as_weak();
    page.on_change_page(move |number| {
        if let Some(page) = weak_page.upgrade() {
            let number = number.clamp(1, page.get_page_count().max(1));
            if number != page.get_current_page() {
                page.set_current_page(number);
                apply_page_snapshot(&page, center_for_page.snapshot());
                page.set_scroll_y(0.0);
            }
        }
    });

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

    let window = window.as_weak();
    page.on_activate_target(move |target| {
        if let Some(window) = window.upgrade() {
            if crate::hooks::open_target(&window, target.as_str()) {
                return;
            }
        }
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

fn sync_language(window: &MainWindow, page: &GitHubMessagesPage) {
    let source = AppState::get(window);
    sync_page_language(page, source.get_language_selection().id.as_str());
}

fn sync_page_language(page: &GitHubMessagesPage, preference: &str) {
    let canonical = usher_i18n::canonical_preference(preference);
    let locale = usher_i18n::resolve_preference(preference);
    let label = crate::i18n::language_choices()
        .into_iter()
        .find(|choice| choice.id == canonical)
        .map(|choice| choice.label)
        .unwrap_or_else(|| canonical.to_string());
    MessagesAppState::get(page).set_language_selection(MessagesLanguageOption {
        id: canonical.into(),
        label: label.into(),
    });
    page.set_filter_count_suffix_offset(locale.segmented_count_suffix_offset);
}

fn apply_page_snapshot(page: &GitHubMessagesPage, snapshot: MessageSnapshot) {
    let locale = usher_i18n::active_locale().id;
    let filter = match page.get_selected_filter() {
        1 => MessageReadFilter::Unread,
        2 => MessageReadFilter::Read,
        _ => MessageReadFilter::All,
    };
    let result = snapshot.page(filter, page.get_current_page().max(1) as usize);
    let page_changed = page.get_current_page() != saturating_i32(result.number);
    page.set_current_page(saturating_i32(result.number));
    page.set_page_count(saturating_i32(result.page_count));
    let links = pagination_links(result.number, result.page_count);
    let current_links = page.get_page_links();
    if current_links.iter().ne(links.iter().copied()) {
        page.set_page_links(ModelRc::new(VecModel::from(links)));
    }
    let rows = result
        .messages
        .into_iter()
        .cloned()
        .map(|mut message| {
            if let Some((title, body)) = crate::hooks::notification_copy(&message, locale) {
                message.title = title;
                message.body = body;
            }
            MessageListItem {
                id: message.id.into(),
                title: localized_message_value(
                    locale,
                    message.source.as_str(),
                    message.channel.as_str(),
                    message.category.as_str(),
                    message.title.as_str(),
                    DIAGNOSTIC_TITLE_KEYS,
                )
                .into(),
                body: localized_message_value(
                    locale,
                    message.source.as_str(),
                    message.channel.as_str(),
                    message.category.as_str(),
                    message.body.as_str(),
                    DIAGNOSTIC_BODY_KEYS,
                )
                .into(),
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
            }
        })
        .collect::<Vec<_>>();
    let total = snapshot.messages.len();
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
    update_page_rows(page, rows);
    if page_changed {
        page.set_scroll_y(0.0);
    } else {
        page.invoke_clamp_scroll();
    }
}

fn update_page_rows(page: &GitHubMessagesPage, mut rows: Vec<MessageListItem>) {
    // Keep exactly 20 delegate slots, including on route exit and empty pages.
    // Recreating factory delegates can reuse stale renderer text bindings.
    // Unused slots contain no message payload and remain invisible at zero height.
    let visible_count = rows.len().min(MESSAGE_PAGE_SIZE);
    rows.resize_with(MESSAGE_PAGE_SIZE, MessageListItem::default);
    let current = page.get_messages();
    if let Some(model) = current.as_any().downcast_ref::<VecModel<MessageListItem>>() {
        for (index, row) in rows.into_iter().enumerate() {
            if index == model.row_count() {
                model.push(row);
            } else if model
                .row_data(index)
                .is_none_or(|existing| !message_row_equivalent(&existing, &row))
            {
                model.set_row_data(index, row);
            }
        }
    } else {
        page.set_messages(ModelRc::new(VecModel::from(rows)));
    }
    page.set_page_row_count(saturating_i32(visible_count));
}

/// Primer's first/last links and two neighbors, with edge windows expanded.
fn pagination_links(current: usize, total: usize) -> Vec<i32> {
    if total <= 9 {
        return (1..=total).map(saturating_i32).collect();
    }
    let start = current.saturating_sub(2).clamp(1, total - 6);
    let end = (current + 2).max(7).min(total);
    let mut links = vec![1];
    if start > 3 {
        links.push(0);
    } else if start == 3 {
        links.push(2);
    }
    links.extend((start.max(2)..=end.min(total - 1)).map(saturating_i32));
    if end < total - 2 {
        links.push(0);
    } else if end == total - 2 {
        links.push(saturating_i32(total - 1));
    }
    links.push(saturating_i32(total));
    links
}

const DIAGNOSTIC_TITLE_KEYS: &[&str] = &["diagnostics.previous_run_title"];
const DIAGNOSTIC_BODY_KEYS: &[&str] = &["diagnostics.previous_run_message"];

fn localized_message_value(
    locale: &str,
    source: &str,
    channel: &str,
    category: &str,
    value: &str,
    keys: &[&'static str],
) -> String {
    if source != "Diagnostics" || channel != "application" || category != "diagnostics" {
        return value.to_string();
    }

    keys.iter()
        .copied()
        .find(|key| {
            usher_i18n::available_locales()
                .iter()
                .any(|candidate| usher_i18n::translate_for(candidate.id, key).as_ref() == value)
        })
        .map(|key| usher_i18n::translate_for(locale, key).into_owned())
        .unwrap_or_else(|| value.to_string())
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
    status_description_for(usher_i18n::active_locale().id, unread)
}

fn status_description_for(locale: &str, unread: usize) -> String {
    match unread {
        0 => usher_i18n::translate_for(locale, "messages.caught_up").into_owned(),
        1 => format!(
            "1 {}",
            usher_i18n::translate_for(locale, "messages.unread_notification")
        ),
        n => format!(
            "{n} {}",
            usher_i18n::translate_for(locale, "status.unread_notifications")
        ),
    }
}

fn category_label(category: &str) -> String {
    category_label_for(usher_i18n::active_locale().id, category)
}

fn category_label_for(locale: &str, category: &str) -> String {
    match category {
        usher_messages::categories::GENERAL => {
            usher_i18n::translate_for(locale, "messages.category_general").into_owned()
        }
        usher_messages::categories::WEBHOOK => {
            usher_i18n::translate_for(locale, "messages.category_webhook").into_owned()
        }
        usher_messages::categories::SOFTWARE_UPDATE => {
            usher_i18n::translate_for(locale, "messages.category_software_update").into_owned()
        }
        usher_messages::categories::CD_WORKFLOW => {
            usher_i18n::translate_for(locale, "messages.category_cd_workflow").into_owned()
        }
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
#[path = "message_center/paging_tests.rs"]
mod paging_tests;

#[cfg(test)]
mod tests {
    use super::{
        DIAGNOSTIC_BODY_KEYS, DIAGNOSTIC_TITLE_KEYS, category_label_for, localized_message_value,
        message_row_equivalent, status_description_for,
    };
    use usher_messages::{MessageLevel, MessageListItem};

    #[test]
    fn category_labels_cover_built_in_and_extension_keys() {
        assert_eq!(
            category_label_for("en", "software_update"),
            "Software update"
        );
        assert_eq!(category_label_for("zh-CN", "software_update"), "软件更新");
        assert_eq!(
            category_label_for("zh-CN", "security-alert"),
            "Security Alert"
        );
    }

    #[test]
    fn status_description_matches_unread_count() {
        assert_eq!(status_description_for("en", 0), "You're all caught up.");
        assert_eq!(status_description_for("en", 1), "1 unread notification");
        assert_eq!(status_description_for("en", 17), "17 unread notifications");
        assert_eq!(status_description_for("zh-CN", 0), "所有通知都已处理。");
        assert_eq!(status_description_for("zh-CN", 17), "17 条未读通知");
    }

    #[test]
    fn persisted_diagnostic_copy_follows_the_active_locale() {
        assert_eq!(
            localized_message_value(
                "en",
                "Diagnostics",
                "application",
                "diagnostics",
                "上次运行意外结束",
                DIAGNOSTIC_TITLE_KEYS,
            ),
            "Previous run ended unexpectedly"
        );
        assert_eq!(
            localized_message_value(
                "zh-CN",
                "Diagnostics",
                "application",
                "diagnostics",
                "Open Settings > Diagnostics to export the relevant logs.",
                DIAGNOSTIC_BODY_KEYS,
            ),
            "请打开设置 > 诊断，导出相关日志。"
        );
    }

    #[test]
    fn remote_or_unrecognized_message_copy_is_never_reclassified() {
        assert_eq!(
            localized_message_value(
                "en",
                "Webhook",
                "webhook",
                "diagnostics",
                "上次运行意外结束",
                DIAGNOSTIC_TITLE_KEYS,
            ),
            "上次运行意外结束"
        );
        assert_eq!(
            localized_message_value(
                "en",
                "Diagnostics",
                "application",
                "diagnostics",
                "自定义诊断消息",
                DIAGNOSTIC_TITLE_KEYS,
            ),
            "自定义诊断消息"
        );
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
