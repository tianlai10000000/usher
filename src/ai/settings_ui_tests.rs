use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use crate::settings::AiAgentSettings;

fn request(id: u64, draft: &AiAgentSettings) -> SaveUiRequest {
    SaveUiRequest {
        _permit: None,
        attempt: id,
        request: id,
        context: None,
        draft: draft.clone(),
    }
}

#[test]
fn completed_receipts_keep_host_order_and_only_latest_attempt_owns_localized_feedback() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for locale in ["en", "zh-CN"] {
        let window = MainWindow::new().unwrap();
        crate::i18n::apply_preference(locale).unwrap();
        crate::i18n::sync_main_window(&window, locale);
        let page = SettingsPageBridge::default();
        let messages = Rc::new(RefCell::new(Vec::new()));
        let successes = messages.clone();
        ToastManager::get(&window).on_success(move |options| {
            successes
                .borrow_mut()
                .push((true, options.message.to_string()));
        });
        let errors = messages.clone();
        ToastManager::get(&window).on_error(move |options| {
            errors
                .borrow_mut()
                .push((false, options.message.to_string()));
        });
        let fail = Arc::new(AtomicBool::new(false));
        let failing = fail.clone();
        let facade =
            AiAgentSettingsFacade::with_test_persistence(AiAgentSettings::default(), move |_| {
                if failing.load(Ordering::Acquire) {
                    Err("injected disk failure".into())
                } else {
                    Ok(())
                }
            });
        let state = Arc::new(SaveUiState::default());
        let first =
            crate::ai::settings_state::apply_provider_preset(&AiAgentSettings::default(), "openai");
        let mut second = first.clone();
        second.model = "second-model".into();
        let first_receipt = facade.save_draft(first.clone()).unwrap().unwrap();
        let first_outcome = first_receipt.wait().unwrap();
        let second_receipt = facade.save_draft(second.clone()).unwrap().unwrap();
        let second_outcome = second_receipt.wait().unwrap();
        // Completion callbacks can enter the event loop in the opposite order
        // to their durable worker operations. The later host state must win.
        state.latest_attempt.store(2, Ordering::Release);
        assert!(
            messages.borrow().is_empty(),
            "durable work alone cannot touch Slint feedback"
        );
        complete_save(
            &window,
            page.clone(),
            state.clone(),
            request(2, &second),
            Ok(second_outcome),
        );
        complete_save(
            &window,
            page.clone(),
            state.clone(),
            request(1, &first),
            Ok(first_outcome),
        );
        assert_eq!(AppState::get(&window).get_ai_agent_model(), "second-model");
        assert_eq!(
            *messages.borrow(),
            vec![(
                true,
                i18n::tr("settings.feedback.ai_agent_saved").to_string()
            )]
        );
        fail.store(true, Ordering::Release);
        let mut third = second.clone();
        third.model = "failed-model".into();
        let failed = facade
            .save_draft(third.clone())
            .unwrap()
            .unwrap()
            .wait()
            .unwrap();
        let failed_revision = failed.snapshot.revision();
        state.latest_attempt.store(3, Ordering::Release);
        window.set_settings_ai_agent_saved(true);
        complete_save(
            &window,
            page.clone(),
            state.clone(),
            request(3, &third),
            Ok(failed),
        );
        assert!(!window.get_settings_ai_agent_saved());
        assert_eq!(
            messages.borrow().last().unwrap(),
            &(
                false,
                i18n::tr("settings.feedback.ai_agent_save_failed").to_string()
            )
        );
        fail.store(false, Ordering::Release);
        let recovered = facade
            .save_draft(third.clone())
            .unwrap()
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(recovered.snapshot.revision(), failed_revision);
        state.latest_attempt.store(4, Ordering::Release);
        complete_save(
            &window,
            page.clone(),
            state,
            request(4, &third),
            Ok(recovered),
        );
        assert_eq!(messages.borrow().len(), 3);
        assert_eq!(
            messages.borrow().last().unwrap(),
            &(
                true,
                i18n::tr("settings.feedback.ai_agent_saved").to_string()
            )
        );
        facade.shutdown_checked().unwrap();
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn durable_completion_without_ui_consumption_keeps_eight_owned_payloads_and_releases_permits() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let page = SettingsPageBridge::default();
    let facade = AiAgentSettingsFacade::memory(AiAgentSettings::default());
    let state = Arc::new(SaveUiState::default());
    let mut queued = Vec::new();
    let mut draft = AiAgentSettings::default();
    for index in 0..UI_SAVE_CAPACITY {
        draft.model = format!("queued-model-{index}");
        let permit = state.reserve(&draft).unwrap().unwrap();
        let receipt = facade.save_draft(draft.clone()).unwrap().unwrap();
        let mut request = request(receipt.id, &draft);
        request._permit = Some(permit);
        // The real reducer and durable save have finished, but the queued UI
        // callback still owns its permit and complete immutable request.
        queued.push((request, receipt.wait().unwrap()));
    }
    assert_eq!(state.pending.lock().unwrap().len(), UI_SAVE_CAPACITY);
    assert!(
        state.reserve(&draft).unwrap().is_none(),
        "duplicate UI request must not reserve another owner, even at the cap"
    );
    draft.model = "rejected-ui-backlog".into();
    assert!(state.reserve(&draft).is_err());
    assert_eq!(facade.snapshot().state().model, "queued-model-7");
    state.latest_attempt.store(99, Ordering::Release);
    let (first, outcome) = queued.remove(0);
    complete_save(&window, page.clone(), state.clone(), first, Ok(outcome));
    assert_eq!(state.pending.lock().unwrap().len(), UI_SAVE_CAPACITY - 1);
    let (latest, outcome) = queued.pop().unwrap();
    complete_save(&window, page, state.clone(), latest, Ok(outcome));
    // Once the newest operation has completed, an older still-pending equal
    // draft is a legitimate later revert, rather than an adjacent duplicate.
    let older = queued.last().unwrap().0.draft.clone();
    let reverted = state.reserve(&older).unwrap().unwrap();
    drop(reverted);
    let permit = state.reserve(&draft).unwrap().unwrap();
    let mut undeliverable = request(100, &draft);
    undeliverable._permit = Some(permit);
    let before_failed_delivery = state.pending.lock().unwrap().len();
    assert!(slint::invoke_from_event_loop(move || drop(undeliverable)).is_err());
    assert_eq!(
        state.pending.lock().unwrap().len(),
        before_failed_delivery - 1,
        "failed event-loop delivery must release its complete request owner"
    );
    drop(queued);
    assert!(state.pending.lock().unwrap().is_empty());
    let permit = state.reserve(&draft).unwrap().unwrap();
    let receipt = facade.save_draft(draft.clone()).unwrap().unwrap();
    let mut successful = request(receipt.id, &draft);
    successful._permit = Some(permit);
    state
        .latest_attempt
        .store(successful.attempt, Ordering::Release);
    complete_save(
        &window,
        SettingsPageBridge::default(),
        state.clone(),
        successful,
        Ok(receipt.wait().unwrap()),
    );
    assert!(
        state.pending.lock().unwrap().is_empty(),
        "a successful completion must release its permit before the Saved reset timer fires"
    );
    facade.shutdown_checked().unwrap();
}
