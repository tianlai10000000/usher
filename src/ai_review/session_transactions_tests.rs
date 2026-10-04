use super::super::controller::{ReviewCancellation, run_review_work_until_cancelled};
use super::*;
use std::sync::mpsc;
use std::time::Duration;
use usher_ai_review::{ReviewConversationRole, ReviewResult};

fn fixture() -> PullRequestChatSession {
    serde_json::from_value(serde_json::json!({
        "task":{"id":"task","target":{"project_key":"project","project_name":"project",
            "pull_request_number":"1","pull_request_title":"PR","base_sha":"base","head_sha":"head"},
            "rule_name":"","model_name":"fixture","status":"failed","progress_percent":0,
            "completed_chunks":0,"total_chunks":1,"finding_count":0,"error":"",
            "created_at_unix_ms":1,"updated_at_unix_ms":1},
        "input":{"target":{"project_key":"project","project_name":"project",
            "pull_request_number":"1","pull_request_title":"PR","base_sha":"base","head_sha":"head"},
            "model":{"provider":"fixture","base_url":"https://example.invalid","model":"fixture"},
            "unified_diff":"fixture","repository_context":{},"base_rule":null},"conversation":[]
    })).unwrap()
}
fn message(content: &str) -> ReviewConversationMessage {
    ReviewConversationMessage {
        message_id: None,
        role: ReviewConversationRole::User,
        content: content.into(),
        created_at_unix_ms: 42,
        elapsed_ms: None,
        prompt: None,
    }
}
fn setup(
    path: PathBuf,
) -> (
    Arc<SessionTransactions>,
    Arc<Mutex<ReviewTaskState>>,
    PullRequestChatSession,
) {
    let fallback = fixture();
    let mut state = ReviewTaskState::default();
    state
        .tasks
        .insert(fallback.task.id.clone(), fallback.task.clone());
    state.order.push(fallback.task.id.clone());
    let state = Arc::new(Mutex::new(state));
    let catalog = state.clone();
    let sessions = SessionTransactions::new(
        path,
        Arc::new(move || Arc::new(catalog.lock().unwrap().clone())),
    );
    (sessions, state, fallback)
}
struct Gate {
    ready: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}
fn probe() -> (Arc<dyn Fn() + Send + Sync>, Gate) {
    let (start, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let wait = Mutex::new(wait);
    let first = AtomicBool::new(true);
    (
        Arc::new(move || {
            if first.swap(false, Ordering::AcqRel) {
                start.send(()).unwrap();
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
        }),
        Gate { ready, release },
    )
}
async fn accepted(
    sessions: &Arc<SessionTransactions>,
    fallback: &PullRequestChatSession,
    text: &str,
) -> (AskAttempt, SessionSnapshot) {
    let (attempt, receipt) = sessions
        .accept_user(
            fallback.task.id.clone(),
            message(text),
            fallback.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    let snapshot = receipt.await.unwrap().unwrap();
    sessions.wait_attempt(&attempt).await;
    (attempt, snapshot)
}

#[tokio::test]
async fn cancellation_before_user_receipt_waits_actual_owned_commit_then_next_ask_preserves_both() {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, _, fallback) = setup(dir.path().into());
    let (hook, gate) = probe();
    *sessions.write_probe.lock().unwrap() = Some(hook);
    let cancellation = Arc::new(ReviewCancellation::default());
    let (attempt, receipt) = sessions
        .accept_user(
            fallback.task.id.clone(),
            message("old"),
            fallback.clone(),
            cancellation.flag(),
        )
        .unwrap();
    gate.ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let owner = sessions.clone();
    let token = cancellation.clone();
    let mut flow = tokio::spawn(async move {
        let snapshot = receipt.await.unwrap().unwrap();
        let outcome = run_review_work_until_cancelled(token, std::future::pending::<()>()).await;
        owner.wait_attempt(&attempt).await;
        (snapshot, outcome)
    });
    cancellation.cancel();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut flow)
            .await
            .is_err()
    );
    gate.release.send(()).unwrap();
    let (_, outcome) = flow.await.unwrap();
    assert_eq!(outcome, Err(()));
    let (_, snapshot) = accepted(&sessions, &fallback, "new").await;
    assert_eq!(
        snapshot
            .conversation()
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>(),
        ["old", "new"]
    );
    assert!(sessions.drain().await.is_ok());
}

#[tokio::test]
async fn stable_operation_retry_is_once_but_same_body_same_timestamp_new_submit_is_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, _, fallback) = setup(dir.path().into());
    let (old, _) = accepted(&sessions, &fallback, "same").await;
    let retry = sessions.retry_user(old.clone()).await.unwrap();
    assert_eq!(retry.conversation().len(), 1);
    let (new, snapshot) = accepted(&sessions, &fallback, "same").await;
    assert_eq!(snapshot.conversation().len(), 2);
    assert_ne!(
        snapshot.conversation()[0].message_id,
        snapshot.conversation()[1].message_id
    );
    let mut late = message("stale assistant");
    late.role = ReviewConversationRole::Assistant;
    assert!(
        sessions
            .append_assistant(old, late)
            .await
            .unwrap()
            .is_none()
    );
    let reload = sessions.read(new.id.clone()).await.unwrap().unwrap();
    assert_eq!(reload.conversation().len(), 2);
}

#[tokio::test]
async fn dropped_receipt_and_closed_admission_still_drain_accepted_user_and_release_weak_owner() {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, _, fallback) = setup(dir.path().into());
    let (hook, gate) = probe();
    *sessions.write_probe.lock().unwrap() = Some(hook);
    let (attempt, receipt) = sessions
        .accept_user(
            fallback.task.id.clone(),
            message("accepted"),
            fallback.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    gate.ready.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(receipt);
    sessions.close_admission();
    assert!(
        sessions
            .accept_user(
                fallback.task.id.clone(),
                message("rejected"),
                fallback.clone(),
                Arc::new(AtomicBool::new(false))
            )
            .is_err()
    );
    let owner = sessions.clone();
    let mut draining = tokio::spawn(async move { owner.drain().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut draining)
            .await
            .is_err()
    );
    gate.release.send(()).unwrap();
    assert_eq!(draining.await.unwrap().unwrap().completed_io, 1);
    sessions.wait_attempt(&attempt).await;
    drop(attempt);
    assert_eq!(sessions.retained_task_owners(), 0);
    let raw: PullRequestChatSession =
        serde_json::from_slice(&std::fs::read(dir.path().join("task.chat.json")).unwrap()).unwrap();
    assert_eq!(raw.conversation[0].content, "accepted");
}

#[tokio::test]
async fn reconciliation_waits_io_barrier_before_deleting_any_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, _, fallback) = setup(dir.path().into());
    let (hook, gate) = probe();
    *sessions.write_probe.lock().unwrap() = Some(hook);
    let (attempt, receipt) = sessions
        .accept_user(
            fallback.task.id.clone(),
            message("saved"),
            fallback.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    gate.ready.recv_timeout(Duration::from_secs(2)).unwrap();
    // Unit fixture exercises the shared IO barrier. Release interposer separately observes
    // the real usher_fs atomic temporary at rename, rather than this synthetic sibling.
    let temporary = dir.path().join("task.chat.json.tmp-active-fixture");
    std::fs::write(&temporary, "active").unwrap();
    let (done, completed) = mpsc::channel();
    let owner = sessions.clone();
    let cleanup = std::thread::spawn(move || {
        owner.reconcile().unwrap();
        done.send(()).unwrap();
    });
    assert!(completed.recv_timeout(Duration::from_millis(30)).is_err());
    assert!(temporary.exists());
    gate.release.send(()).unwrap();
    receipt.await.unwrap().unwrap();
    sessions.wait_attempt(&attempt).await;
    cleanup.join().unwrap();
    assert!(!temporary.exists());
    assert_eq!(
        sessions
            .read(attempt.id.clone())
            .await
            .unwrap()
            .unwrap()
            .conversation()[0]
            .content,
        "saved"
    );
}

#[tokio::test]
async fn reconciliation_rechecks_user_admission_after_acquiring_exclusive_io_and_never_resurrects_retired_task()
 {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, catalog, fallback) = setup(dir.path().into());
    let (reconcile_hook, reconcile_gate) = probe();
    *sessions.reconcile_probe.lock().unwrap() = Some(reconcile_hook);
    let (done, completed) = mpsc::channel();
    let owner = sessions.clone();
    let cleanup = std::thread::spawn(move || {
        owner.reconcile().unwrap();
        done.send(()).unwrap();
    });
    reconcile_gate
        .ready
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let (user_hook, user_gate) = probe();
    *sessions.user_probe.lock().unwrap() = Some(user_hook);
    let (attempt, receipt) = sessions
        .accept_user(
            fallback.task.id.clone(),
            message("accepted before retirement"),
            fallback.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    user_gate
        .ready
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    catalog.lock().unwrap().tasks.clear();
    reconcile_gate.release.send(()).unwrap();
    assert!(completed.recv_timeout(Duration::from_millis(30)).is_err());
    user_gate.release.send(()).unwrap();
    receipt.await.unwrap().unwrap();
    cleanup.join().unwrap();
    assert!(!dir.path().join("task.chat.json").exists());
    assert!(sessions.retry_user(attempt.clone()).await.is_err());
    let mut late = message("orphan");
    late.role = ReviewConversationRole::Assistant;
    assert!(
        sessions
            .append_assistant(attempt, late)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!dir.path().join("task.chat.json").exists());
}

#[tokio::test]
async fn review_promotion_freshly_merges_current_history_and_late_ask_uses_review_file() {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, _, fallback) = setup(dir.path().into());
    let (_, _) = accepted(&sessions, &fallback, "before promotion").await;
    let review = ReviewSession {
        task: fallback.task.clone(),
        input: fallback.input.clone(),
        result: ReviewResult {
            summary: "review".into(),
            findings: vec![],
            skipped_files: vec![],
        },
        conversation: vec![],
        prompt: Default::default(),
    };
    let promoted = sessions
        .complete_review(fallback.task.id.clone(), review)
        .await
        .unwrap();
    assert_eq!(promoted.conversation.len(), 1);
    let (_, after) = accepted(&sessions, &fallback, "after promotion").await;
    assert!(matches!(after, SessionSnapshot::Review(_)));
    let reopened = sessions
        .read(fallback.task.id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.conversation().len(), 2);
}

#[tokio::test]
async fn assistant_admitted_before_cancel_is_drained_and_cannot_overwrite_following_user() {
    let dir = tempfile::tempdir().unwrap();
    let (sessions, _, fallback) = setup(dir.path().into());
    let (old, _) = accepted(&sessions, &fallback, "old user").await;
    let (hook, gate) = probe();
    *sessions.write_probe.lock().unwrap() = Some(hook);
    let owner = sessions.clone();
    let ticket = old.clone();
    let mut assistant = message("accepted assistant");
    assistant.role = ReviewConversationRole::Assistant;
    let response = crate::app_runtime::shared_runtime()
        .spawn(async move { owner.append_assistant(ticket, assistant).await });
    gate.ready.recv_timeout(Duration::from_secs(2)).unwrap();
    old.cancelled.store(true, Ordering::Release);
    response.abort(); // owner-managed blocking JoinHandle must survive this dropped waiter.
    let (new, receipt) = sessions
        .accept_user(
            fallback.task.id.clone(),
            message("new user"),
            fallback.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    gate.release.send(()).unwrap();
    receipt.await.unwrap().unwrap();
    sessions.wait_attempt(&old).await;
    sessions.wait_attempt(&new).await;
    let reloaded = sessions
        .read(fallback.task.id.clone())
        .await
        .unwrap()
        .unwrap();
    for content in ["old user", "accepted assistant", "new user"] {
        assert!(reloaded.conversation().iter().any(|m| m.content == content));
    }
}

#[tokio::test]
async fn drain_collects_real_commit_errors_with_bounded_failure_samples() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("not-a-directory");
    std::fs::write(&blocked, "file").unwrap();
    let (sessions, _, fallback) = setup(blocked);
    for _ in 0..20 {
        let (_, receipt) = sessions
            .accept_user(
                fallback.task.id.clone(),
                message("failure"),
                fallback.clone(),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
        assert!(receipt.await.unwrap().is_err());
    }
    let error = sessions.drain().await.unwrap_err();
    assert_eq!(error.0.failure_count, 20);
    assert_eq!(error.0.failure_samples.len(), MAX_FAILURE_SAMPLES);
    assert_eq!(sessions.retained_task_owners(), 0);
}

#[test]
fn legacy_rows_and_identified_rows_do_not_content_alias_and_conflicting_identity_is_rejected() {
    let legacy = message("same");
    let mut rows = vec![legacy.clone()];
    merge_conversations(&mut rows, &[legacy]).unwrap();
    assert_eq!(rows.len(), 1);
    let mut identified = message("same");
    identified.message_id = Some("operation:user".into());
    merge_conversations(&mut rows, &[identified.clone()]).unwrap();
    assert_eq!(rows.len(), 2);
    let mut conflicting = identified.clone();
    conflicting.content = "different".into();
    assert!(merge_conversations(&mut rows, &[conflicting]).is_err());
    merge_conversations(&mut rows, &[identified]).unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn reconciliation_filesystem_error_is_included_in_session_io_drain_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("not-a-directory");
    std::fs::write(&blocked, "file").unwrap();
    let (sessions, _, _) = setup(blocked);
    assert!(sessions.reconcile().is_err());
    let error = sessions.drain().await.unwrap_err();
    assert_eq!(error.0.completed_io, 1);
    assert_eq!(error.0.failure_count, 1);
    assert_eq!(error.0.failure_samples.len(), 1);
}
