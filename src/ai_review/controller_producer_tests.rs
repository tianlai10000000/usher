use super::*;
use std::sync::mpsc;
use usher_messages::NoopDesktopNotifier;

struct Fixture {
    window: MainWindow,
    controller: AiReviewController,
    root: tempfile::TempDir,
}

fn fixture(initial: ReviewTaskState, deferred: bool) -> Fixture {
    i_slint_backend_testing::init_no_event_loop();
    let root = tempfile::tempdir().unwrap();
    let window = MainWindow::new().unwrap();
    let config = ReviewConfigFacade::isolated(root.path().join("config.json"));
    let task_store: Arc<TaskStore> = Arc::new(LocalStateStore::new(initial, reduce_tasks));
    let catalog = task_store.clone();
    let session_dir = root.path().join("sessions");
    let sessions = SessionTransactions::new(
        session_dir.clone(),
        Arc::new(move || catalog.snapshot().state_arc()),
    );
    let task_state_path = root.path().join(TASK_STATE_FILE);
    let persistence = TaskPersistenceWorker::start_coordinated(
        task_state_path.clone(),
        session_dir,
        !deferred,
        sessions.clone(),
    );
    let sender = persistence.sender.clone();
    let commit = task_store.observe_commits(Arc::new(move |state| {
        let _ = sender.send(state.revision(), state.state_arc());
    }));
    let producers = Producers::default();
    let startup_hydration = if deferred {
        Some((task_state_path, producers.admit().unwrap()))
    } else {
        None
    };
    let task_subscription = task_store.subscribe(Arc::new(|_| {}));
    let pr_subscription = task_store.subscribe(Arc::new(|_| {}));
    let config_subscription = config.subscribe(Arc::new(|_| {}));
    let inner = Arc::new(ControllerInner {
        window: window.as_weak(),
        config,
        task_store,
        scheduler: Mutex::new(ReviewScheduler::new(1)),
        requests: Mutex::new(HashMap::new()),
        cancellations: Mutex::new(HashMap::new()),
        chat_cancellations: Mutex::new(HashMap::new()),
        chat_status_key: Mutex::new(None),
        chat_delivery_generation: Arc::new(AtomicU64::new(0)),
        sessions,
        response_projector: Mutex::new(AgentResponseProjector::default()),
        response_projection_suspended: AtomicBool::new(true),
        response_projector_clear_requested: AtomicBool::new(false),
        response_mount_generation: AtomicU64::new(0),
        response_actions: Mutex::new(HashMap::new()),
        response_variant: ResponseVariant::from_environment(),
        artifact_preview: crate::artifact_preview::setup(
            &window,
            &crate::settings::PreviewSettings::default(),
        ),
        task_persistence: persistence,
        message_center: MessageCenter::open_with(
            root.path().join("messages.json"),
            Arc::new(NoopDesktopNotifier),
        )
        .unwrap(),
        producers,
        startup_hydration: Mutex::new(startup_hydration),
        fixture: true,
    });
    bind_callbacks(&window, &inner);
    let controller = AiReviewController {
        inner,
        _task_subscription: Arc::new(task_subscription),
        _pr_list_subscription: Arc::new(pr_subscription),
        _config_subscription: Arc::new(config_subscription),
    }
    .with_persistence_subscription(commit);
    Fixture {
        window,
        controller,
        root,
    }
}

struct Gate {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}
fn gate() -> (Arc<dyn Fn() + Send + Sync>, Gate) {
    let (started, entered) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let wait = Mutex::new(wait);
    let first = AtomicBool::new(true);
    (
        Arc::new(move || {
            if first.swap(false, Ordering::AcqRel) {
                started.send(()).unwrap();
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
        }),
        Gate { entered, release },
    )
}

fn request(number: &str) -> PendingReviewRequest {
    PendingReviewRequest {
        project_alias: "fixture".into(),
        project_key: ProjectReviewKey("project".into()),
        project_name: "Fixture".into(),
        local_repo: String::new(),
        remote_repo: String::new(),
        repo_type: "github".into(),
        repo_project_id: "fixture/project".into(),
        number: number.into(),
        title: format!("PR {number}"),
        author: "Fixture".into(),
        source_branch: "feature".into(),
        target_branch: "main".into(),
        updated_at: "2026-10-15T00:00:00Z".into(),
        model_name: "Fixture".into(),
        extra_requirements: String::new(),
    }
}

#[test]
fn shutdown_waits_for_review_terminal_continuation_and_cancels_queued_start() {
    let _locale_guard = crate::i18n::test_locale_guard();
    let fixture = fixture(ReviewTaskState::default(), false);
    let inner = fixture.controller.inner.clone();
    let (block, gate) = gate();
    let _subscription = inner.task_store.subscribe(Arc::new(move |state| {
        if state
            .state()
            .tasks
            .values()
            .any(|task| task.status == ReviewTaskStatus::Completed)
        {
            block();
        }
    }));
    inner.clone().enqueue(request("1"), false);
    gate.entered.recv_timeout(Duration::from_secs(5)).unwrap();
    // Session I/O has completed, but the real controller is still between its
    // terminal Task Action and Message Center/Hook continuations.
    let first = inner
        .task_store
        .snapshot()
        .state()
        .tasks
        .values()
        .next()
        .unwrap()
        .id
        .clone();
    assert!(
        session_paths(&fixture.root.path().join("sessions"), &first)
            .0
            .exists()
    );
    assert!(inner.message_center.snapshot().messages.is_empty());
    inner.clone().enqueue(request("2"), false);
    assert_eq!(inner.scheduler.lock().unwrap().queued(), 1);
    let controller = fixture.controller.clone();
    let mut closing = crate::app_runtime::shared_runtime()
        .spawn(async move { controller.shutdown_async().await });
    crate::app_runtime::shared_runtime().block_on(async {
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut closing)
                .await
                .is_err()
        );
    });
    inner.clone().enqueue(request("3"), false);
    assert_eq!(inner.task_store.snapshot().state().tasks.len(), 2);
    assert_eq!(inner.scheduler.lock().unwrap().queued(), 0);
    gate.release.send(()).unwrap();
    crate::app_runtime::shared_runtime()
        .block_on(closing)
        .unwrap()
        .unwrap();
    assert_eq!(inner.message_center.snapshot().messages.len(), 1);
    assert_eq!(inner.scheduler.lock().unwrap().active(), 0);
    assert!(inner.requests.lock().unwrap().is_empty());
    let stored: ReviewTaskState =
        serde_json::from_slice(&std::fs::read(fixture.root.path().join(TASK_STATE_FILE)).unwrap())
            .unwrap();
    assert_eq!(
        stored
            .tasks
            .values()
            .filter(|task| task.status == ReviewTaskStatus::Completed)
            .count(),
        1
    );
    assert_eq!(
        stored
            .tasks
            .values()
            .filter(|task| task.status == ReviewTaskStatus::Cancelled)
            .count(),
        1
    );
    fixture.controller.shutdown();
    inner.message_center.shutdown_checked().unwrap();
}

fn ask_state() -> ReviewTaskState {
    let task = response_fixture_session(0, 512).task;
    ReviewTaskState {
        order: vec![task.id.clone()],
        tasks: std::collections::BTreeMap::from([(task.id.clone(), task)]),
    }
}

fn prepare_ask(fixture: &Fixture) {
    let task = fixture
        .controller
        .inner
        .task_store
        .snapshot()
        .state()
        .tasks
        .values()
        .next()
        .unwrap()
        .clone();
    fixture.window.set_current_route(Route::PrReview);
    fixture
        .window
        .set_ai_review_current_task_id(task.id.0.into());
    fixture
        .window
        .set_workflow_project_id(task.target.project_key.0.into());
    fixture
        .window
        .set_workflow_pr_review_number(task.target.pull_request_number.into());
    fixture.window.set_ai_review_chat_sending(false);
    fixture.controller.inner.begin_response_mount();
}

#[test]
fn accepted_ask_waits_its_blocking_user_receipt_even_after_shutdown_waiter_is_cancelled() {
    let _locale_guard = crate::i18n::test_locale_guard();
    let fixture = fixture(ask_state(), false);
    prepare_ask(&fixture);
    let (probe, gate) = gate();
    fixture.controller.inner.sessions.set_write_probe(probe);
    fixture
        .window
        .invoke_ai_review_send_message("Accepted before shutdown".into());
    gate.entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let controller = fixture.controller.clone();
    let first = crate::app_runtime::shared_runtime()
        .spawn(async move { controller.shutdown_async().await });
    crate::app_runtime::shared_runtime().block_on(async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        first.abort();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(30),
                fixture.controller.shutdown_async()
            )
            .await
            .is_err()
        );
    });
    fixture
        .window
        .invoke_ai_review_send_message("Rejected after shutdown".into());
    gate.release.send(()).unwrap();
    crate::app_runtime::shared_runtime()
        .block_on(fixture.controller.shutdown_async())
        .unwrap();
    let id = fixture.controller.inner.task_store.snapshot().state().order[0].clone();
    let chat: PullRequestChatSession = serde_json::from_slice(
        &std::fs::read(session_paths(&fixture.root.path().join("sessions"), &id).1).unwrap(),
    )
    .unwrap();
    assert_eq!(chat.conversation.len(), 1);
    assert_eq!(chat.conversation[0].content, "Accepted before shutdown");
    assert!(
        fixture
            .controller
            .inner
            .chat_cancellations
            .lock()
            .unwrap()
            .is_empty()
    );
    fixture
        .controller
        .inner
        .message_center
        .shutdown_checked()
        .unwrap();
}

#[test]
fn shutdown_before_first_frame_completes_reserved_task_recovery_once() {
    let _locale_guard = crate::i18n::test_locale_guard();
    let fixture = fixture(ReviewTaskState::default(), true);
    let mut recovered = ask_state();
    recovered.tasks.values_mut().next().unwrap().status = ReviewTaskStatus::Running;
    std::fs::write(
        fixture.root.path().join(TASK_STATE_FILE),
        serde_json::to_vec(&recovered).unwrap(),
    )
    .unwrap();
    assert!(
        fixture
            .controller
            .inner
            .task_store
            .snapshot()
            .state()
            .tasks
            .is_empty()
    );
    crate::app_runtime::shared_runtime()
        .block_on(fixture.controller.shutdown_async())
        .unwrap();
    let state = fixture.controller.inner.task_store.snapshot();
    assert_eq!(state.state().tasks.len(), 1);
    let task = state.state().tasks.values().next().unwrap();
    assert_eq!(task.status, ReviewTaskStatus::Failed);
    assert_eq!(task.error, "ai_review.failure.interrupted");
    let revision = state.revision();
    fixture.controller.inner.clone().start_task_hydration();
    crate::app_runtime::shared_runtime()
        .block_on(fixture.controller.shutdown_async())
        .unwrap();
    assert_eq!(
        fixture.controller.inner.task_store.snapshot().revision(),
        revision
    );
    let stored: ReviewTaskState =
        serde_json::from_slice(&std::fs::read(fixture.root.path().join(TASK_STATE_FILE)).unwrap())
            .unwrap();
    assert_eq!(&stored, state.state());
    fixture
        .controller
        .inner
        .message_center
        .shutdown_checked()
        .unwrap();
}

#[test]
fn successful_explicit_same_text_ask_after_io_failure_does_not_poison_shutdown() {
    let _locale_guard = crate::i18n::test_locale_guard();
    let fixture = fixture(ask_state(), false);
    prepare_ask(&fixture);
    let sessions = fixture.root.path().join("sessions");
    std::fs::write(&sessions, b"injected invalid parent").unwrap();
    fixture
        .window
        .invoke_ai_review_send_message("same text".into());
    crate::app_runtime::shared_runtime().block_on(async {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fixture
            .controller
            .inner
            .chat_cancellations
            .lock()
            .unwrap()
            .is_empty()
        {
            assert!(Instant::now() < deadline);
            tokio::task::yield_now().await;
        }
    });
    assert!(
        crate::app_runtime::shared_runtime()
            .block_on(fixture.controller.inner.sessions.drain())
            .is_err()
    );
    std::fs::remove_file(&sessions).unwrap();
    fixture.window.set_ai_review_chat_sending(false);
    fixture
        .window
        .invoke_ai_review_send_message("same text".into());
    crate::app_runtime::shared_runtime()
        .block_on(fixture.controller.shutdown_async())
        .unwrap();
    let id = fixture.controller.inner.task_store.snapshot().state().order[0].clone();
    let chat: PullRequestChatSession =
        serde_json::from_slice(&std::fs::read(session_paths(&sessions, &id).1).unwrap()).unwrap();
    assert_eq!(
        chat.conversation
            .iter()
            .filter(|message| message.role == ReviewConversationRole::User)
            .count(),
        1
    );
    assert_eq!(chat.conversation[0].content, "same text");
    fixture
        .controller
        .inner
        .message_center
        .shutdown_checked()
        .unwrap();
}
