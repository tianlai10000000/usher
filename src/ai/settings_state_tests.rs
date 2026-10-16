use super::*;
use crate::settings::{AiAgentHeader, AiAgentSettings};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn connection() -> AiAgentSettings {
    AiAgentSettings {
        provider: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        model: "fixture-model".into(),
        ..Default::default()
    }
}
fn accepted(facade: &AiAgentSettingsFacade, draft: AiAgentSettings) -> AiAgentSaveReceipt {
    facade
        .update_profile(draft.into())
        .unwrap()
        .expect("save admitted")
}

#[test]
fn accepted_drafts_reduce_off_caller_in_order_and_close_drains_the_bounded_queue() {
    let caller = std::thread::current().id();
    let reduced = Arc::new(Mutex::new(Vec::new()));
    let reduction_threads = reduced.clone();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let persisted = writes.clone();
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let count = AtomicUsize::new(0);
    let facade = AiAgentSettingsFacade::with_persistence(
        connection().into(),
        move |current, action| {
            reduction_threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            reduce_ai_agent_settings(current, action)
        },
        move |state| {
            assert_ne!(std::thread::current().id(), caller);
            if count.fetch_add(1, Ordering::AcqRel) == 0 {
                entered.send(()).unwrap();
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            persisted.lock().unwrap().push(state.clone());
            Ok(())
        },
    )
    .unwrap();
    let mut draft = connection();
    draft.api_key = "synthetic-first-credential".into();
    draft.reply_language = "en".into();
    let first_draft = draft.clone();
    let mut receipts = vec![accepted(&facade, draft.clone())];
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        facade
            .update_profile(draft.clone().into())
            .unwrap()
            .is_none()
    );
    for index in 1..SAVE_CAPACITY {
        draft.model = format!("later-model-{index}");
        draft.api_key = format!("synthetic-later-credential-{index}");
        draft.reply_language = "zh-CN".into();
        if index == SAVE_CAPACITY - 1 {
            draft = first_draft.clone();
        }
        receipts.push(accepted(&facade, draft.clone()));
    }
    draft.model = "rejected-full".into();
    assert_eq!(
        facade.update_profile(draft.into()).err().unwrap(),
        "AI Agent settings save queue is full"
    );
    assert_eq!(reduced.lock().unwrap().len(), 1);
    assert_eq!(facade.snapshot().state().active().unwrap(), &first_draft);
    let stopping = facade.clone();
    let stop = std::thread::spawn(move || stopping.shutdown_checked());
    let deadline = Instant::now() + Duration::from_secs(5);
    while facade.commands.lock().unwrap().sender.is_some() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(facade.activate_profile("openai").is_err());
    assert!(!stop.is_finished());
    release.send(()).unwrap();
    for (index, receipt) in receipts.into_iter().enumerate() {
        assert_eq!(receipt.id, index as u64 + 1);
        assert_eq!(receipt.profile_id, "openai");
        let outcome = receipt.wait().unwrap();
        outcome.applied.unwrap();
        outcome.persisted.unwrap();
        assert_eq!(outcome.snapshot.revision(), index as u64 + 1);
        assert_eq!(outcome.snapshot.state(), &writes.lock().unwrap()[index]);
    }
    stop.join().unwrap().unwrap();
    facade.shutdown_checked().unwrap();
    assert_eq!(writes.lock().unwrap().len(), SAVE_CAPACITY);
    assert!(
        reduced
            .lock()
            .unwrap()
            .iter()
            .all(|thread| *thread != caller)
    );
    assert_eq!(facade.snapshot().state().active().unwrap(), &first_draft);
}

#[test]
fn explicit_same_value_retry_recovers_failure_without_new_business_revision() {
    let failing = Arc::new(AtomicBool::new(true));
    let failure = failing.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let saved = Arc::new(Mutex::new(None));
    let persisted = saved.clone();
    let facade = AiAgentSettingsFacade::with_persistence(
        connection().into(),
        reduce_ai_agent_settings,
        move |state| {
            count.fetch_add(1, Ordering::SeqCst);
            if failure.load(Ordering::SeqCst) {
                return Err("injected write failure".into());
            }
            *persisted.lock().unwrap() = Some(state.clone());
            Ok(())
        },
    )
    .unwrap();
    let mut draft = connection();
    draft.api_key = "synthetic-explicit-retry-key".into();
    let first = accepted(&facade, draft.clone()).wait().unwrap();
    assert_eq!(first.persisted.unwrap_err(), "injected write failure");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(saved.lock().unwrap().is_none());
    failing.store(false, Ordering::SeqCst);
    let retried = accepted(&facade, draft).wait().unwrap();
    retried.persisted.unwrap();
    assert_eq!(retried.snapshot.revision(), first.snapshot.revision());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        *saved.lock().unwrap(),
        Some(retried.snapshot.state().clone())
    );
    facade.shutdown_checked().unwrap();
}

#[test]
fn repeated_failure_and_shutdown_report_failure_without_hidden_retry() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let facade = AiAgentSettingsFacade::with_persistence(
        connection().into(),
        reduce_ai_agent_settings,
        move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Err("disk unavailable".into())
        },
    )
    .unwrap();
    let mut draft = connection();
    draft.api_key = "synthetic-repeated-failure-key".into();
    for _ in 0..2 {
        assert_eq!(
            accepted(&facade, draft.clone())
                .wait()
                .unwrap()
                .persisted
                .unwrap_err(),
            "disk unavailable"
        );
    }
    assert_eq!(facade.snapshot().revision(), 1);
    assert_eq!(facade.shutdown_checked().unwrap_err(), "disk unavailable");
    assert_eq!(facade.shutdown_checked().unwrap_err(), "disk unavailable");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn crud_checks_active_and_existence_when_executed_without_reviving_deleted_agents() {
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let facade = AiAgentSettingsFacade::with_persistence(
        AiAgentConfiguration::default(),
        reduce_ai_agent_settings,
        move |_| {
            if count.fetch_add(1, Ordering::AcqRel) == 0 {
                entered.send(()).unwrap();
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            Ok(())
        },
    )
    .unwrap();
    let mut a = AiAgentProfile::from(connection());
    a.id = "a".into();
    a.name = "Agent A".into();
    a.connection.api_key = "synthetic-A-only".into();
    let first = facade.create_profile(a.clone()).unwrap().unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut b = a.clone();
    b.id = "b".into();
    b.name = "Agent B".into();
    b.connection.api_key = "synthetic-B-only".into();
    b.connection.headers = vec![AiAgentHeader {
        enabled: true,
        name: "X-B-Key".into(),
        value: "synthetic-B-header".into(),
    }];
    let second = facade.create_profile(b.clone()).unwrap().unwrap();
    let third = facade.activate_profile("b").unwrap().unwrap();
    let fourth = facade.delete_profile("b").unwrap().unwrap();
    let fifth = facade.delete_profile("a").unwrap().unwrap();
    let sixth = facade.update_profile(a).unwrap().unwrap();
    release.send(()).unwrap();
    for (index, receipt) in [first, second, third, fourth, fifth, sixth]
        .into_iter()
        .enumerate()
    {
        assert_eq!(receipt.id, index as u64 + 1);
        let outcome = receipt.wait().unwrap();
        if index == 3 || index == 5 {
            assert!(outcome.applied.is_err());
            assert!(outcome.persisted.is_err());
        } else {
            outcome.applied.unwrap();
            outcome.persisted.unwrap();
        }
        if index == 0 {
            assert_eq!(outcome.snapshot.state().active_id, "a");
        }
        if index == 1 {
            assert_eq!(outcome.snapshot.state().active_id, "a");
        }
        if index >= 2 {
            assert_eq!(outcome.snapshot.state().active_id, "b");
        }
    }
    let state = facade.snapshot();
    assert_eq!(state.revision(), 4);
    assert_eq!(state.state().profiles, [b.clone()]);
    assert_eq!(state.state().active_profile(), Some(&b));
    assert_eq!(attempts.load(Ordering::SeqCst), 4);
    assert!(
        facade
            .activate_profile("missing")
            .unwrap()
            .unwrap()
            .wait()
            .unwrap()
            .applied
            .is_err()
    );
    assert!(
        facade
            .create_profile(b)
            .unwrap()
            .unwrap()
            .wait()
            .unwrap()
            .applied
            .is_err()
    );
    assert_eq!(facade.snapshot().revision(), 4);
    facade.shutdown_checked().unwrap();
}

#[test]
fn create_assigns_unique_stable_ids_and_only_first_creation_activates() {
    let facade = AiAgentSettingsFacade::memory(AiAgentConfiguration::default());
    let first = facade
        .create_profile(AiAgentProfile::default())
        .unwrap()
        .unwrap();
    let first_id = first.profile_id.clone();
    assert!(!first_id.is_empty());
    first.wait().unwrap().persisted.unwrap();
    let second = facade
        .create_profile(AiAgentProfile::default())
        .unwrap()
        .unwrap();
    let second_id = second.profile_id.clone();
    assert_ne!(first_id, second_id);
    second.wait().unwrap().persisted.unwrap();
    assert_eq!(facade.snapshot().state().active_id, first_id);
    let mut edited = facade
        .snapshot()
        .state()
        .profile(&second_id)
        .unwrap()
        .clone();
    edited.name = "Renamed peer".into();
    facade
        .update_profile(edited)
        .unwrap()
        .unwrap()
        .wait()
        .unwrap()
        .persisted
        .unwrap();
    assert_eq!(facade.snapshot().state().active_id, first_id);
    facade.shutdown_checked().unwrap();
}
