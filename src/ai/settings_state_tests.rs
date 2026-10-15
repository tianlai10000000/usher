use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn accepted(facade: &AiAgentSettingsFacade, draft: AiAgentSettings) -> AiAgentSaveReceipt {
    facade
        .save_draft(draft)
        .unwrap()
        .expect("new save admitted")
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
        AiAgentSettings::default(),
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
    let mut draft = apply_provider_preset(&AiAgentSettings::default(), "openai");
    draft.api_key = "synthetic-first-credential".into();
    draft.reply_language = "en".into();
    let first_draft = draft.clone();
    let mut receipts = vec![accepted(&facade, draft.clone())];
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        facade.save_draft(draft.clone()).unwrap().is_none(),
        "duplicate Save must not create another operation"
    );
    // Each later request owns its complete draft now, not the Store/UI value
    // observed when the earlier blocked durable save eventually finishes.
    for index in 1..SAVE_CAPACITY {
        draft.model = format!("later-model-{index}");
        draft.api_key = format!("synthetic-later-credential-{index}");
        draft.reply_language = "zh-CN".into();
        if index == SAVE_CAPACITY - 1 {
            // A → B → A is an explicit later revert, not a duplicate A.
            draft = first_draft.clone();
        }
        receipts.push(accepted(&facade, draft.clone()));
    }
    draft.model = "rejected-full".into();
    assert_eq!(
        facade.save_draft(draft).err().unwrap(),
        "AI Agent settings save queue is full"
    );
    assert_eq!(
        reduced.lock().unwrap().len(),
        1,
        "only the first accepted request may reduce before its durable barrier"
    );
    assert_eq!(facade.snapshot().state(), &first_draft);
    let stopping = facade.clone();
    let stop = std::thread::spawn(move || stopping.shutdown_checked());
    let deadline = Instant::now() + Duration::from_secs(5);
    while facade.commands.lock().unwrap().sender.is_some() {
        assert!(
            Instant::now() < deadline,
            "shutdown must close command admission before waiting"
        );
        std::thread::yield_now();
    }
    assert!(facade.save_draft(AiAgentSettings::default()).is_err());
    assert!(
        !stop.is_finished(),
        "shutdown must await the blocked accepted write"
    );
    release.send(()).unwrap();
    for (index, receipt) in receipts.into_iter().enumerate() {
        assert_eq!(receipt.id, index as u64 + 1);
        let outcome = receipt.wait().unwrap();
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
    assert_eq!(facade.snapshot().state(), &first_draft);
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
        AiAgentSettings::default(),
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
    let configuration = apply_provider_preset(&AiAgentSettings::default(), "openai");
    let first = accepted(&facade, configuration.clone()).wait().unwrap();
    assert_eq!(first.persisted.unwrap_err(), "injected write failure");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a changed Save must not retry its own failed automatic write"
    );
    assert!(saved.lock().unwrap().is_none());
    failing.store(false, Ordering::SeqCst);
    let retried = accepted(&facade, configuration.clone()).wait().unwrap();
    retried.persisted.unwrap();
    assert_eq!(retried.snapshot.revision(), first.snapshot.revision());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(*saved.lock().unwrap(), Some(configuration));
    facade.shutdown_checked().unwrap();
}

#[test]
fn repeated_failure_and_shutdown_report_failure_without_hidden_retry() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let facade = AiAgentSettingsFacade::with_persistence(
        AiAgentSettings::default(),
        reduce_ai_agent_settings,
        move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Err("disk unavailable".into())
        },
    )
    .unwrap();
    let configuration = apply_provider_preset(&AiAgentSettings::default(), "openai");
    for _ in 0..2 {
        assert_eq!(
            accepted(&facade, configuration.clone())
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
fn provider_selection_updates_the_complete_preset_without_overwriting_other_draft_fields() {
    let initial = AiAgentSettings {
        provider: "deepseek".into(),
        base_url: DEEPSEEK_BASE_URL.into(),
        api_key: "synthetic-secret".into(),
        model: DEEPSEEK_MODEL.into(),
        reply_language: "en".into(),
    };
    let draft = apply_provider_preset(&initial, "openai");
    assert_eq!(draft.provider, "openai");
    assert_eq!(draft.base_url, OPENAI_BASE_URL);
    assert_eq!(draft.model, OPENAI_MODEL);
    assert_eq!(draft.api_key, initial.api_key);
    assert_eq!(draft.reply_language, initial.reply_language);
}
