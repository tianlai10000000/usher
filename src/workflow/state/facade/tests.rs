use super::*;
use crate::workflow::tag::state::TagAction;
use std::sync::{atomic::AtomicUsize, mpsc};
use std::time::{Duration, Instant};
async fn barrier(state: &SharedWorkflowState) {
    let sender = state
        .inner
        .admission
        .lock()
        .unwrap()
        .sender
        .clone()
        .unwrap();
    let (done, wait) = tokio::sync::oneshot::channel();
    sender.send(Command::Barrier(done)).await.unwrap();
    wait.await.unwrap();
}
fn description(value: &str) -> WorkflowAction {
    WorkflowAction::Tag(TagAction::Draft {
        next: "v0.0.1".into(),
        target: "v0.0.0".into(),
        description: value.into(),
    })
}
fn project(id: &str) -> WorkflowProjectContext {
    WorkflowProjectContext {
        id: id.into(),
        name: id.into(),
        ..WorkflowProjectContext::default()
    }
}
#[test]
fn admitted_actions_reduce_off_caller_in_fifo_order_and_unchanged_inputs_do_not_commit() {
    let caller = std::thread::current().id();
    let state = SharedWorkflowState::new();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let changes = observed.clone();
    let _subscription = state.inner.store.subscribe(Arc::new(move |snapshot| {
        if snapshot.revision() > 0 {
            changes.lock().unwrap().push((
                std::thread::current().id(),
                snapshot.state().current_state_ref().tag_description.clone(),
            ));
        }
    }));
    for value in ["A", "B", "A", "A"] {
        state.submit(description(value)).unwrap();
    }
    crate::app_runtime::shared_runtime().block_on(async {
        barrier(&state).await;
    });
    let changes = observed.lock().unwrap();
    assert_eq!(
        changes
            .iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "A"]
    );
    assert!(changes.iter().all(|(thread, _)| *thread != caller));
    drop(changes);
    assert_eq!(state.snapshot().revision(), 3);
    crate::app_runtime::shared_runtime().block_on(state.shutdown_async());
}
#[test]
fn old_project_generations_cannot_publish_into_a_reselected_project() {
    let state = SharedWorkflowState::new();
    state.select_project(project("A")).unwrap();
    let old = state.identity();
    state.submit(description("old-A")).unwrap();
    state.select_project(project("B")).unwrap();
    state.submit(description("B-only")).unwrap();
    state.select_project(project("A")).unwrap();
    state.submit(description("new-A")).unwrap();
    crate::app_runtime::shared_runtime().block_on(async {
        state.publish(old, description("stale-A")).await;
        barrier(&state).await;
    });
    assert_eq!(state.snapshot().state().current_project.id, "A");
    assert_eq!(
        state.snapshot().state().current_state_ref().tag_description,
        "new-A"
    );
    assert!(!state.snapshot().state().projects.contains_key("B"));
    crate::app_runtime::shared_runtime().block_on(state.shutdown_async());
}
#[test]
fn queue_exhaustion_rejects_explicitly_and_concurrent_shutdown_drains_every_accepted_action() {
    let state = SharedWorkflowState::new();
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let count = AtomicUsize::new(0);
    let _subscription = state.inner.store.subscribe(Arc::new(move |snapshot| {
        if snapshot.revision() > 0 && count.fetch_add(1, Ordering::AcqRel) == 0 {
            entered.send(()).unwrap();
            blocked
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
    }));
    state.submit(description("first")).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    for index in 0..CAPACITY - 1 {
        state
            .submit(description(&format!("queued-{index}")))
            .unwrap();
    }
    assert!(state.submit(description("rejected-full")).is_err());
    let first = state.clone();
    let second = state.clone();
    let a = crate::app_runtime::shared_runtime().spawn(async move {
        first.shutdown_async().await;
    });
    let b = crate::app_runtime::shared_runtime().spawn(async move {
        second.shutdown_async().await;
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !state.is_closing() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(state.submit(description("rejected-closing")).is_err());
    assert!(!a.is_finished());
    release.send(()).unwrap();
    crate::app_runtime::shared_runtime().block_on(async {
        a.await.unwrap();
        b.await.unwrap();
    });
    assert_eq!(
        state.snapshot().state().current_state_ref().tag_description,
        format!("queued-{}", CAPACITY - 2)
    );
    assert_eq!(state.snapshot().revision(), CAPACITY as u64);
    crate::app_runtime::shared_runtime().block_on(state.shutdown_async());
}
#[test]
fn projection_preserves_pending_draft_and_suppresses_its_own_ui_echo() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for locale in ["en", "zh-CN"] {
        let ui = crate::MainWindow::new().unwrap();
        crate::i18n::apply_preference(locale).unwrap();
        let state = SharedWorkflowState::new();
        state.install(&ui);
        crate::workflow::tag::bind_tag(&ui, state.clone());
        state.inner.admission.lock().unwrap().draft = Some((
            "v0.0.1".into(),
            "v0.0.0".into(),
            "newer draft 中文 & $()".into(),
        ));
        let snapshot = state.snapshot();
        state.project_ui(&ui, &snapshot);
        assert_eq!(ui.get_workflow_tag_description(), "newer draft 中文 & $()");
        assert_eq!(state.snapshot().revision(), 0);
        state.inner.admission.lock().unwrap().draft = None;
        state
            .submit(description("saved in-memory draft 中文"))
            .unwrap();
        crate::app_runtime::shared_runtime().block_on(barrier(&state));
        ui.set_current_route(crate::Route::Settings);
        ui.set_workflow_tag_description("discarded visual tree".into());
        ui.set_current_route(crate::Route::Workflow);
        state.project_ui(&ui, &state.snapshot());
        assert_eq!(
            ui.get_workflow_tag_description(),
            "saved in-memory draft 中文"
        );
        crate::app_runtime::shared_runtime().block_on(state.shutdown_async());
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
