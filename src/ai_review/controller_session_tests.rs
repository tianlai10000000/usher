// Session transaction delivery regressions; production callbacks remain in controller.
use super::*;

#[test]
fn queued_old_ask_terminal_callback_preserves_new_ask_ui_in_source_and_packaged_locale() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    let generation = AtomicU64::new(1);
    let actions = Mutex::new(HashMap::new());
    for locale in ["en", "zh-CN"] {
        usher_i18n::activate_preference(locale);
        generation.store(1, Ordering::Release);
        window.set_ai_review_conversation(ModelRc::new(VecModel::default()));
        // Queue the actual production terminal UI operation before the new Ask.
        let old_callback = || apply_chat_completion(&window, Err(()), 7, &actions, &generation, 1);
        generation.store(2, Ordering::Release);
        append_ui_conversation(&window, "user", "accepted next Ask", None);
        window.set_ai_review_chat_sending(true);
        let next_status = crate::i18n::tr("ai_review.preparing_context").into_owned();
        window.set_ai_review_chat_status(next_status.clone().into());
        assert!(!old_callback());
        assert!(window.get_ai_review_chat_sending());
        assert_eq!(window.get_ai_review_chat_status(), next_status.as_str());
        assert_eq!(window.get_ai_review_conversation().row_count(), 1);
        assert!(apply_chat_completion(
            &window,
            Err(()),
            7,
            &actions,
            &generation,
            2
        ));
        let rows = window.get_ai_review_conversation();
        assert_eq!(rows.row_count(), 2);
        assert_eq!(
            rows.row_data(1).unwrap().content,
            crate::i18n::tr("ai_review.response_stopped").as_ref()
        );
    }
    usher_i18n::activate_preference(usher_i18n::fallback_locale().id);
}

#[test]
fn queued_initial_session_read_cannot_replace_new_ask_completed_conversation() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    let generation = AtomicU64::new(1);
    let actions = Mutex::new(HashMap::new());
    let projection = SessionProjection {
        mount_generation: 1,
        task: response_fixture_session(1, 512).task,
        result: None,
        conversation: vec![ProjectedConversationMessage {
            id: "old-row".into(),
            role: ReviewConversationRole::User,
            content: "old read".into(),
            elapsed_ms: None,
            response: None,
        }],
        response_actions: HashMap::new(),
    };
    // This is the actual guarded projection operation used inside the event-loop
    // callback, deliberately executed after the newly accepted Ask has completed.
    let old_load_callback =
        || apply_projection_for_ask_generation(&window, projection, &actions, &generation, 1);
    generation.store(2, Ordering::Release);
    append_ui_conversation(&window, "user", "new completed Ask", None);
    window.set_ai_review_chat_sending(false);
    assert!(!old_load_callback());
    assert_eq!(window.get_ai_review_conversation().row_count(), 1);
    assert_eq!(
        window
            .get_ai_review_conversation()
            .row_data(0)
            .unwrap()
            .content,
        "new completed Ask"
    );
}

#[test]
fn review_completion_after_new_ask_stop_keeps_findings_without_replacing_conversation() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    let generation = AtomicU64::new(2);
    let actions = Mutex::new(HashMap::new());
    append_ui_conversation(&window, "user", "new Ask retained", None);
    append_ui_conversation(&window, "assistant", "stopped sentinel", Some(3));
    window.set_ai_review_chat_sending(false);
    let projection = SessionProjection {
        mount_generation: 1,
        task: response_fixture_session(1, 512).task,
        result: Some(ReviewResult {
            summary: "current legal Review".into(),
            findings: Vec::new(),
            skipped_files: Vec::new(),
        }),
        conversation: Vec::new(),
        response_actions: HashMap::new(),
    };
    apply_review_projection_for_ask_generation(&window, projection, &actions, &generation, 1);
    assert_eq!(window.get_ai_review_summary(), "current legal Review");
    assert_eq!(window.get_ai_review_conversation().row_count(), 2);
    assert_eq!(
        window
            .get_ai_review_conversation()
            .row_data(0)
            .unwrap()
            .content,
        "new Ask retained"
    );
}

#[test]
fn newer_ask_on_same_mount_rejects_queued_old_terminal_delivery() {
    let delivery = AtomicU64::new(1);
    let old = 1;
    // The old Future releases duplicate protection before its event-loop callback
    // executes; a new Ask on the same mount advances this independent generation.
    delivery.fetch_add(1, Ordering::AcqRel);
    assert!(!chat_delivery_is_current(&delivery, old));
    assert!(chat_delivery_is_current(&delivery, 2));
}

#[tokio::test]
async fn cancellation_interrupts_in_flight_review_work() {
    let cancellation = Arc::new(ReviewCancellation::default());
    let work_cancellation = cancellation.clone();
    let work = tokio::spawn(async move {
        run_review_work_until_cancelled(work_cancellation, std::future::pending::<()>()).await
    });

    tokio::task::yield_now().await;
    cancellation.cancel();

    let result = tokio::time::timeout(Duration::from_secs(1), work)
        .await
        .expect("cancelled review work should finish promptly")
        .expect("review work task should not panic");
    assert_eq!(result, Err(()));
}

#[tokio::test]
async fn invalid_chat_history_is_preserved_instead_of_overwritten() {
    let dir = std::env::temp_dir().join(format!(
        "usher-ai-review-corrupt-chat-{}-{}",
        std::process::id(),
        NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("review.chat.json");
    fs::write(&path, b"{invalid").unwrap();

    let error = load_chat_session(path.clone()).await.unwrap_err();
    assert!(error.contains("was preserved"));
    assert!(!path.exists());
    let backup = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|candidate| {
            candidate
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("review.chat.json.corrupt-"))
        })
        .expect("corrupt chat backup");
    assert_eq!(fs::read(&backup).unwrap(), b"{invalid");

    fs::remove_file(backup).unwrap();
    fs::remove_dir(dir).unwrap();
}
