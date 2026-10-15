use super::super::{
    banner_queue_model, clear_banner_queue, prune_banners_for_deleted_messages, setup_notifications,
};
use super::*;
use crate::NotificationOptions;
use std::collections::HashSet;
use std::sync::{Condvar, Mutex};

#[test]
fn production_notification_callbacks_publish_in_order_off_ui_and_link_only_live_messages() {
    use super::*;
    use usher_messages::NoopDesktopNotifier;
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(NoopDesktopNotifier),
    )
    .unwrap();
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    let window = MainWindow::new().unwrap();
    setup_notifications(&window, None, center.clone(), commands.clone());
    let caller = std::thread::current().id();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let collected = observed.clone();
    let _subscription = center.subscribe(Arc::new(move |snapshot| {
        if !snapshot.messages.is_empty() {
            collected.lock().unwrap().push(std::thread::current().id());
        }
    }));
    let manager = NotificationManager::get(&window);
    usher_i18n::activate_locale("en").unwrap();
    manager.invoke_info(NotificationOptions {
        native_delivery: NotificationDelivery::Never,
        ..Default::default()
    });
    // Locale-sensitive defaults belong to callback admission, not the later
    // worker execution; the next publication uses the newly selected locale.
    usher_i18n::activate_locale("zh-CN").unwrap();
    manager.invoke_warn(NotificationOptions {
        native_delivery: NotificationDelivery::Never,
        ..Default::default()
    });
    commands.barrier();
    let snapshot = center.snapshot();
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].title, "注意");
    assert_eq!(snapshot.messages[0].body, "警告");
    assert_eq!(snapshot.messages[1].title, "Information");
    assert_eq!(snapshot.messages[1].body, "Info");
    assert!(
        observed
            .lock()
            .unwrap()
            .iter()
            .all(|thread| *thread != caller)
    );
    let pending_ids = PENDING_NOTIFICATIONS.with(|pending| {
        pending
            .borrow()
            .iter()
            .map(|(ticket, pending)| (*ticket, pending.notification.r#type))
            .collect::<Vec<_>>()
    });
    let first = pending_ids
        .iter()
        .find(|(_, kind)| *kind == NotificationType::Info)
        .unwrap()
        .0;
    let second = pending_ids
        .iter()
        .find(|(_, kind)| *kind == NotificationType::Warning)
        .unwrap()
        .0;
    // The no-event-loop backend cannot execute worker delivery. Exercise the
    // same production completion on its owning UI thread after the barrier.
    finish_pending_notification(
        &window,
        &center,
        first,
        Some(snapshot.messages[1].id.clone()),
    );
    assert_eq!(
        banner_queue_model(&manager)
            .row_data(0)
            .unwrap()
            .message_id
            .as_str(),
        snapshot.messages[1].id
    );
    commands.clear().unwrap();
    commands.barrier();
    finish_pending_notification(
        &window,
        &center,
        second,
        Some(snapshot.messages[0].id.clone()),
    );
    assert_eq!(
        banner_queue_model(&manager).row_count(),
        1,
        "a deleted pending message must not mount a Banner"
    );
    prune_banners_for_deleted_messages(&manager, &HashSet::from([snapshot.messages[1].id.clone()]));
    assert_eq!(banner_queue_model(&manager).row_count(), 0);
    commands.shutdown().unwrap();
    center.shutdown();
    PENDING_NOTIFICATIONS.with(|pending| assert!(pending.borrow().is_empty()));
}

#[test]
fn latest_ticket_owns_reused_banner_id_and_shared_message_links_remain_authoritative() {
    use super::*;
    use usher_messages::NoopDesktopNotifier;
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(NoopDesktopNotifier),
    )
    .unwrap();
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    let window = MainWindow::new().unwrap();
    setup_notifications(&window, None, center.clone(), commands.clone());
    let manager = NotificationManager::get(&window);
    for title in ["First", "Second"] {
        manager.invoke_push(Notification {
            id: 42,
            title: title.into(),
            message: "Fixture".into(),
            native_delivery: NotificationDelivery::Never,
            ..Default::default()
        });
    }
    commands.barrier();
    let snapshot = center.snapshot();
    let tickets = PENDING_NOTIFICATIONS.with(|pending| {
        pending
            .borrow()
            .iter()
            .map(|(ticket, pending)| (pending.notification.title.to_string(), *ticket))
            .collect::<HashMap<_, _>>()
    });
    assert_eq!(tickets.len(), 2);
    let first = &snapshot.messages[1];
    let second = &snapshot.messages[0];
    finish_pending_notification(&window, &center, tickets["First"], Some(first.id.clone()));
    finish_pending_notification(&window, &center, tickets["First"], Some(second.id.clone()));
    finish_pending_notification(&window, &center, tickets["Second"], Some(second.id.clone()));
    let rows = banner_queue_model(&manager);
    assert_eq!(rows.row_count(), 1);
    assert_eq!(rows.row_data(0).unwrap().title, "Second");
    assert_eq!(rows.row_data(0).unwrap().message_id.as_str(), second.id);
    assert_eq!(PENDING_NOTIFICATION_BYTES.with(Cell::get), 0);
    // Several Banners may intentionally refer to one already-recorded row.
    manager.invoke_push(Notification {
        id: 43,
        message_id: second.id.clone().into(),
        title: "Recorded again".into(),
        ..Default::default()
    });
    assert_eq!(
        center.snapshot().messages.len(),
        2,
        "recorded banners never duplicate publication"
    );
    prune_banners_for_deleted_messages(&manager, &HashSet::from([second.id.clone()]));
    assert_eq!(
        rows.row_count(),
        0,
        "delete removes every banner linked to the same message"
    );
    clear_banner_queue(&manager, &None);
    commands.shutdown().unwrap();
    center.shutdown();
}

#[test]
fn pending_ui_payload_stays_bounded_after_worker_completes_without_event_loop_delivery() {
    use super::*;
    use usher_messages::NoopDesktopNotifier;
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(NoopDesktopNotifier),
    )
    .unwrap();
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    let window = MainWindow::new().unwrap();
    setup_notifications(&window, None, center.clone(), commands.clone());
    let manager = NotificationManager::get(&window);
    for _ in 0..MAX_PENDING_NOTIFICATIONS {
        manager.invoke_info(NotificationOptions {
            title: "Pending".into(),
            message: "Fixture".into(),
            native_delivery: NotificationDelivery::Never,
            ..Default::default()
        });
    }
    commands.barrier();
    assert_eq!(
        PENDING_NOTIFICATIONS.with(|pending| pending.borrow().len()),
        MAX_PENDING_NOTIFICATIONS
    );
    assert!(PENDING_NOTIFICATION_BYTES.with(Cell::get) <= MAX_PENDING_NOTIFICATION_BYTES);
    // Draining the worker cannot admit another UI owner until completion
    // takes an existing payload. Rejections retain one bounded fallback.
    for _ in 0..20 {
        manager.invoke_info(NotificationOptions {
            message: "x".repeat(MAX_PENDING_NOTIFICATION_BYTES).into(),
            native_delivery: NotificationDelivery::Never,
            ..Default::default()
        });
    }
    commands.barrier();
    assert_eq!(
        PENDING_NOTIFICATIONS.with(|pending| pending.borrow().len()),
        MAX_PENDING_NOTIFICATIONS
    );
    FALLBACK_NOTIFICATION.with(|fallback| {
        let fallback = fallback.borrow();
        assert!(fallback.as_ref().unwrap().notification.message.len() <= 4096);
        assert!(pending_notification_bytes(&fallback.as_ref().unwrap().notification) < 65536);
    });
    assert_eq!(center.snapshot().messages.len(), MAX_PENDING_NOTIFICATIONS);
    commands.shutdown().unwrap();
    center.shutdown();
    clear_pending_notifications();
    assert!(PENDING_NOTIFICATIONS.with(|pending| pending.borrow().is_empty()));
    assert_eq!(PENDING_NOTIFICATION_BYTES.with(Cell::get), 0);
    assert!(FALLBACK_NOTIFICATION.with(|fallback| fallback.borrow().is_none()));
}

#[test]
fn notification_admission_freezes_actions_and_focus_before_blocked_worker_executes() {
    struct CapturingNotifier(Arc<Mutex<Vec<String>>>);
    impl usher_messages::DesktopNotifier for CapturingNotifier {
        fn show(&self, message: &usher_messages::Message) -> Result<(), String> {
            self.0.lock().unwrap().push(message.title.clone());
            Ok(())
        }
    }
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let native = Arc::new(Mutex::new(Vec::new()));
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(CapturingNotifier(native.clone())),
    )
    .unwrap();
    let commands = MessageCommandWorker::start(center.clone(), false).unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let listener_gate = gate.clone();
    let (entered, blocked) = std::sync::mpsc::channel();
    let first = std::sync::atomic::AtomicBool::new(true);
    let _subscription = center.subscribe(Arc::new(move |snapshot| {
        if !snapshot.messages.is_empty() && first.swap(false, Ordering::AcqRel) {
            entered.send(()).unwrap();
            let (lock, condition) = &*listener_gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = condition.wait(open).unwrap();
            }
        }
    }));
    commands
        .publish(MessageDraft::new("Block", "Fixture").desktop(false), |_| {})
        .unwrap();
    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    let window = MainWindow::new().unwrap();
    setup_notifications(&window, None, center.clone(), commands.clone());
    let manager = NotificationManager::get(&window);
    let actions = std::rc::Rc::new(VecModel::from(vec![slint::SharedString::from("Original")]));
    center.set_application_active(true);
    manager.invoke_show(
        NotificationType::Info,
        NotificationOptions {
            title: "Focused admission".into(),
            actions: ModelRc::from(actions.clone()),
            native_delivery: NotificationDelivery::WhenUnfocused,
            ..Default::default()
        },
    );
    let retained_bytes = PENDING_NOTIFICATION_BYTES.with(Cell::get);
    actions.set_row_data(0, "Changed".into());
    actions.push("x".repeat(MAX_PENDING_NOTIFICATION_BYTES).into());
    PENDING_NOTIFICATIONS.with(|pending| {
        let pending = pending.borrow();
        let row = pending.values().next().unwrap();
        assert_eq!(row.notification.actions.row_count(), 1);
        assert_eq!(row.notification.actions.row_data(0).unwrap(), "Original");
    });
    assert_eq!(PENDING_NOTIFICATION_BYTES.with(Cell::get), retained_bytes);
    center.set_application_active(false);
    manager.invoke_show(
        NotificationType::Warning,
        NotificationOptions {
            title: "Unfocused admission".into(),
            native_delivery: NotificationDelivery::WhenUnfocused,
            ..Default::default()
        },
    );
    center.set_application_active(true);
    let (lock, condition) = &*gate;
    *lock.lock().unwrap() = true;
    condition.notify_all();
    commands.shutdown().unwrap();
    center.shutdown();
    assert_eq!(*native.lock().unwrap(), ["Unfocused admission"]);
    clear_pending_notifications();
}
