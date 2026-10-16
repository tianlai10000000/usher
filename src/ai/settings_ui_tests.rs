use super::*;
use crate::settings::{AiAgentConfiguration, AiAgentProfile, AiAgentSettings};
use slint::{Model, ModelRc, VecModel};
use usher_ui_settings::AiAgentHeaderItem;

fn profile(id: &str) -> AiAgentProfile {
    AiAgentProfile {
        id: id.into(),
        name: format!("Agent {id}"),
        connection: AiAgentSettings {
            base_url: "https://fixture.example/v1".into(),
            model: format!("model-{id}"),
            api_key: format!("synthetic-{id}"),
            ..Default::default()
        },
    }
}

fn setup(profiles: Vec<AiAgentProfile>) -> (MainWindow, GitHubSettingsPage, Binding) {
    let window = MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    let active_id = profiles
        .first()
        .map(|profile| profile.id.clone())
        .unwrap_or_default();
    let facade = AiAgentSettingsFacade::memory(AiAgentConfiguration {
        active_id,
        profiles,
    });
    crate::settings_page::ai_agent_drafts::bind(&page, Some(facade.clone()));
    bind_page(&window, &page, facade.clone());
    apply_ai_agent_choices_to_page(&page, facade.snapshot().state());
    let binding = Binding {
        window: window.as_weak(),
        page: page.as_weak(),
        state: Arc::default(),
        facade,
    };
    (window, page, binding)
}

fn request(
    binding: &Binding,
    action: UiAction,
    attempt: u64,
    page: &GitHubSettingsPage,
) -> UiRequest {
    UiRequest {
        _permit: binding.state.reserve(&action).unwrap().unwrap(),
        request: attempt,
        target_id: match &action {
            UiAction::Create(profile) | UiAction::Update(profile) => profile.id.clone(),
            UiAction::Activate(id) | UiAction::Delete(id) => id.clone(),
        },
        attempt,
        editor_revision: PageState::get(page).get_ai_agent_editor_revision(),
        header_input_revision: PageState::get(page).get_ai_agent_editor_headers_input_revision(),
        action,
    }
}

#[test]
fn list_search_pages_clamp_after_delete_and_locales_keep_names_and_models() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, page, binding) = setup((0..23).map(|id| profile(&format!("{id:02}"))).collect());
    let state = PageState::get(&page);
    let persisted_order = binding
        .facade
        .snapshot()
        .state()
        .profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let mut edited = profile("09");
    edited.name = "Renamed without moving".into();
    binding
        .facade
        .update_profile(edited)
        .unwrap()
        .unwrap()
        .wait()
        .unwrap()
        .applied
        .unwrap();
    binding
        .facade
        .activate_profile("09")
        .unwrap()
        .unwrap()
        .wait()
        .unwrap()
        .applied
        .unwrap();
    assert_eq!(
        binding
            .facade
            .snapshot()
            .state()
            .profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect::<Vec<_>>(),
        persisted_order,
        "editing and activation retain persisted insertion order"
    );
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        state.invoke_query_ai_agents("".into());
        assert_eq!(state.get_ai_agent_list().row_count(), 10);
        assert_eq!(state.get_ai_agent_total_pages(), 3);
        assert_eq!(state.get_ai_agent_list().row_data(0).unwrap().id, "22");
        assert_eq!(state.get_ai_agent_list().row_data(9).unwrap().id, "13");
        state.invoke_select_ai_agents_page(3);
        assert_eq!(state.get_ai_agent_list().row_count(), 3);
        assert_eq!(
            state.get_ai_agent_list().row_data(0).unwrap().name,
            "Agent 02"
        );
        assert_eq!(
            state.get_ai_agent_list().row_data(0).unwrap().model,
            "model-02"
        );
        state.invoke_query_ai_agents("AGENT 01".into());
        assert_eq!(state.get_ai_agent_page(), 1);
        assert_eq!(state.get_ai_agent_total_count(), 1);
        assert_eq!(state.get_ai_agent_list().row_data(0).unwrap().id, "01");
        state.invoke_query_ai_agents("synthetic".into());
        assert_eq!(
            state.get_ai_agent_total_count(),
            0,
            "credentials are not searchable"
        );
    }
    state.invoke_query_ai_agents("".into());
    state.invoke_select_ai_agents_page(3);
    for id in ["20", "21", "22"] {
        binding
            .facade
            .delete_profile(id)
            .unwrap()
            .unwrap()
            .wait()
            .unwrap();
    }
    apply_ai_agent_choices_to_page(&page, binding.facade.snapshot().state());
    assert_eq!(state.get_ai_agent_page(), 2);
    assert_eq!(state.get_ai_agent_list().row_count(), 10);
    state.invoke_query_ai_agents("no matching agent".into());
    state.invoke_open_new_ai_agent();
    state.set_ai_agent_editor_name("Newly added Agent".into());
    state.set_ai_agent_editor_base_url("https://fixture.example/v1".into());
    state.set_ai_agent_editor_model("new-model".into());
    let draft = ai_agent_profile_from_page(&page);
    let created_id = draft.id.clone();
    let create = request(&binding, UiAction::Create(draft.clone()), 1, &page);
    let outcome = binding
        .facade
        .create_profile(draft)
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    binding.complete(create, Ok(outcome));
    assert!(state.get_ai_agent_query().is_empty());
    assert_eq!(state.get_ai_agent_page(), 1);
    assert_eq!(state.get_ai_agent_list().row_count(), 10);
    assert_eq!(
        state.get_ai_agent_list().row_data(0).unwrap().name,
        "Newly added Agent"
    );
    assert_eq!(
        binding
            .facade
            .snapshot()
            .state()
            .profiles
            .last()
            .unwrap()
            .id,
        created_id,
        "Create appends durable storage while the UI projects newest first"
    );
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    drop(window);
}

#[test]
fn isolated_dialog_headers_cancel_and_route_reset_do_not_change_saved_agents() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = setup(vec![profile("a"), profile("b")]);
    let state = PageState::get(&page);
    state.invoke_edit_ai_agent("b".into());
    let original = binding.facade.snapshot().state().clone();
    let revision = state.get_ai_agent_editor_revision();
    state.set_ai_agent_editor_name("Edited only".into());
    state.set_ai_agent_editor_model("edited-model".into());
    state.invoke_add_ai_agent_header();
    state.invoke_set_ai_agent_header_name(0, "X-Fixture".into());
    state.invoke_set_ai_agent_header_value(0, "synthetic-header".into());
    assert_eq!(
        state
            .get_ai_agent_editor_headers()
            .row_data(0)
            .unwrap()
            .value,
        "synthetic-header"
    );
    assert_eq!(state.get_ai_agent_editor_provider(), "custom");
    assert_eq!(*binding.facade.snapshot().state(), original);
    state.invoke_cancel_ai_agent_editor();
    assert!(!state.get_ai_agent_editor_open());
    assert!(state.get_ai_agent_editor_revision() > revision);
    assert!(state.get_ai_agent_editor_api_key().is_empty());
    state.invoke_edit_ai_agent("a".into());
    assert_eq!(ai_agent_profile_from_page(&page), profile("a"));
    state.invoke_edit_ai_agent("b".into());
    assert_eq!(ai_agent_profile_from_page(&page), profile("b"));
    state.invoke_open_new_ai_agent();
    let first_id = state.get_ai_agent_editor_id();
    assert!(!first_id.is_empty());
    assert!(state.get_ai_agent_editor_headers().row_count() == 0);
    assert!(state.get_ai_agent_editor_api_key().is_empty());
    crate::settings_page::ai_agent_projection::restore_ai_agent_configuration_to_page(
        &page, &original,
    );
    assert!(!state.get_ai_agent_editor_open());
    assert_eq!(*binding.facade.snapshot().state(), original);
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn older_receipt_projects_latest_activation_and_never_closes_a_newer_dialog() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, page, binding) = setup(vec![profile("a"), profile("b")]);
    let state = PageState::get(&page);
    state.invoke_edit_ai_agent("a".into());
    state.set_ai_agent_editor_name("Saved A".into());
    let draft = ai_agent_profile_from_page(&page);
    let save_request = request(&binding, UiAction::Update(draft.clone()), 1, &page);
    let saved = binding
        .facade
        .update_profile(draft)
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    let activation_request = request(&binding, UiAction::Activate("b".into()), 2, &page);
    let activated = binding
        .facade
        .activate_profile("b")
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    state.invoke_cancel_ai_agent_editor();
    state.invoke_edit_ai_agent("b".into());
    state.set_ai_agent_editor_name("Unsaved B".into());
    binding.state.latest_attempt.store(2, Ordering::Release);
    binding.complete(save_request, Ok(saved));
    assert_eq!(crate::AppState::get(&window).get_ai_agent_name(), "Agent b");
    assert_eq!(
        crate::AppState::get(&window).get_ai_agent_model(),
        "model-b"
    );
    assert!(state.get_ai_agent_editor_open());
    assert_eq!(state.get_ai_agent_editor_name(), "Unsaved B");
    binding.complete(activation_request, Ok(activated));
    assert!(state.get_ai_agent_editor_open());
    assert_eq!(state.get_ai_agent_editor_name(), "Unsaved B");
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn delete_requires_confirmation_with_exact_named_copy_and_protects_active_agent() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = setup(vec![profile("a"), profile("b")]);
    crate::i18n::apply_preference("zh-CN").unwrap();
    let state = PageState::get(&page);
    state.invoke_delete_ai_agent("a".into());
    assert!(!state.get_ai_agent_delete_confirm_open());
    state.invoke_delete_ai_agent("b".into());
    assert!(state.get_ai_agent_delete_confirm_open());
    assert_eq!(
        state.get_ai_agent_delete_message(),
        "你确定要删除名为 [Agent b] 的 AI Agent吗？"
    );
    assert!(binding.facade.snapshot().state().profile("b").is_some());
    state.invoke_cancel_delete_ai_agent();
    assert!(binding.facade.snapshot().state().profile("b").is_some());
    state.invoke_delete_ai_agent("b".into());
    binding
        .facade
        .activate_profile("b")
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    apply_ai_agent_choices_to_page(&page, binding.facade.snapshot().state());
    assert!(!state.get_ai_agent_delete_confirm_open());
    assert!(
        binding
            .facade
            .delete_profile("b")
            .unwrap()
            .unwrap()
            .wait()
            .unwrap()
            .applied
            .is_err()
    );
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn header_validation_blocks_save_and_admission_remains_bounded_until_ui_completion() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = setup(vec![profile("a")]);
    let state = PageState::get(&page);
    state.invoke_edit_ai_agent("a".into());
    state.set_ai_agent_editor_headers(ModelRc::new(VecModel::from(vec![AiAgentHeaderItem {
        enabled: true,
        name: "X-Fixture".into(),
        value: "synthetic\r\nInjected: yes".into(),
    }])));
    state.invoke_save_ai_agent_editor();
    assert!(!state.get_ai_agent_editor_error().is_empty());
    assert!(
        binding
            .facade
            .snapshot()
            .state()
            .profile("a")
            .unwrap()
            .connection
            .headers
            .is_empty()
    );
    let mut requests = Vec::new();
    for index in 0..admission::UI_SAVE_CAPACITY {
        let action = UiAction::Activate(format!("{index}"));
        requests.push(binding.state.reserve(&action).unwrap().unwrap());
    }
    assert!(
        binding
            .state
            .reserve(&UiAction::Delete("other".into()))
            .is_err()
    );
    assert!(
        binding
            .state
            .reserve(&UiAction::Activate("7".into()))
            .unwrap()
            .is_none()
    );
    drop(requests);
    assert!(binding.state.pending.lock().unwrap().is_empty());
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn stale_edit_never_recreates_a_deleted_agent_and_feedback_retranslates() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = setup(vec![profile("a"), profile("b")]);
    crate::i18n::apply_preference("en").unwrap();
    let state = PageState::get(&page);
    state.invoke_edit_ai_agent("b".into());
    assert!(!state.get_ai_agent_editor_new());
    let draft = ai_agent_profile_from_page(&page);
    binding
        .facade
        .delete_profile("b")
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    let request = request(&binding, UiAction::Update(draft.clone()), 1, &page);
    let outcome = binding
        .facade
        .update_profile(draft.clone())
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    binding.complete(request, Ok(outcome));
    assert!(binding.facade.snapshot().state().profile("b").is_none());
    assert!(state.get_ai_agent_editor_open());
    assert_eq!(ai_agent_profile_from_page(&page), draft);
    assert_eq!(
        state.get_ai_agent_editor_error_key(),
        "settings.feedback.ai_agent_save_failed"
    );
    let english = state.get_ai_agent_editor_error();
    state.set_ai_agent_editor_test_result_key("settings.feedback.ai_agent_test_failed".into());
    crate::i18n::apply_preference("zh-CN").unwrap();
    sync_language(&page);
    assert_ne!(state.get_ai_agent_editor_error(), english);
    assert_eq!(
        state.get_ai_agent_editor_error(),
        i18n::tr("settings.feedback.ai_agent_save_failed").as_ref()
    );
    assert_eq!(
        state.get_ai_agent_editor_test_result(),
        i18n::tr("settings.feedback.ai_agent_test_failed").as_ref()
    );
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn legacy_unnamed_agent_confirmation_uses_the_same_localized_list_name() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let mut unnamed = profile("b");
    unnamed.name.clear();
    let (_window, page, binding) = setup(vec![profile("a"), unnamed]);
    let state = PageState::get(&page);
    for (locale, expected) in [("en", "Custom"), ("zh-CN", "自定义")] {
        crate::i18n::apply_preference(locale).unwrap();
        apply_ai_agent_choices_to_page(&page, binding.facade.snapshot().state());
        state.invoke_delete_ai_agent("b".into());
        assert_eq!(state.get_ai_agent_delete_name(), expected);
        assert_eq!(
            state
                .get_ai_agent_list()
                .iter()
                .find(|profile| profile.id == "b")
                .unwrap()
                .name,
            expected
        );
        assert!(
            state
                .get_ai_agent_delete_message()
                .contains(&format!("[{expected}]"))
        );
        state.invoke_cancel_delete_ai_agent();
    }
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn typing_in_named_agent_dialog_preserves_provider_and_other_agent_configuration() {
    use i_slint_backend_testing::ElementHandle;
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, binding) = setup(vec![profile("a"), profile("b")]);
    let state = PageState::get(&page);
    page.set_viewport_width(1200.0);
    page.set_window_width(1200.0);
    page.set_viewport_height(1200.0);
    page.set_window_height(1200.0);
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        state.invoke_edit_ai_agent("b".into());
        state.invoke_select_ai_agent_provider("deepseek".into());
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(2));
        for (key, value) in [
            ("settings.ai_agent.name", "Typed Agent"),
            ("settings.ai_agent.model", "typed-model"),
        ] {
            if key == "settings.ai_agent.model" {
                page.window().dispatch_event(WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(600.0, 600.0),
                    delta_x: 0.0,
                    delta_y: -500.0,
                });
                i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(2));
            }
            let label = i18n::tr(key);
            let input = ElementHandle::find_by_accessible_label(&page, label.as_ref())
                .find(|element| element.accessible_value().is_some())
                .expect("dialog input");
            input.mock_single_click(PointerEventButton::Left);
            let previous = input.accessible_value().unwrap();
            input.set_accessible_selection_offsets(0, previous.len() as i32);
            let keys: [slint::SharedString; 2] = [Key::Backspace.into(), value.into()];
            for text in keys {
                page.window()
                    .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
                page.window()
                    .dispatch_event(WindowEvent::KeyReleased { text });
            }
            i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(2));
            assert_eq!(state.get_ai_agent_editor_provider(), "deepseek");
            assert_eq!(input.accessible_value().unwrap(), value);
        }
        assert_eq!(
            binding.facade.snapshot().state().profile("a").unwrap(),
            &profile("a")
        );
        assert_eq!(
            binding.facade.snapshot().state().profile("b").unwrap(),
            &profile("b")
        );
        state.invoke_cancel_ai_agent_editor();
        state.invoke_edit_ai_agent("a".into());
        assert_eq!(ai_agent_profile_from_page(&page), profile("a"));
    }
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn applied_create_with_disk_failure_retries_same_identity_as_update() {
    use std::sync::atomic::AtomicBool;
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, page, mut binding) = setup(Vec::new());
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    let failing = Arc::new(AtomicBool::new(true));
    let injected = failing.clone();
    binding.facade =
        AiAgentSettingsFacade::with_test_persistence(AiAgentConfiguration::default(), move |_| {
            if injected.load(Ordering::Acquire) {
                Err("synthetic disk failure".into())
            } else {
                Ok(())
            }
        });
    let state = PageState::get(&page);
    state.invoke_open_new_ai_agent();
    state.set_ai_agent_editor_name("Retry Agent".into());
    state.set_ai_agent_editor_base_url("https://fixture.example/v1".into());
    state.set_ai_agent_editor_model("retry-model".into());
    let draft = ai_agent_profile_from_page(&page);
    let first = request(&binding, UiAction::Create(draft.clone()), 1, &page);
    let outcome = binding
        .facade
        .create_profile(draft.clone())
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    assert!(outcome.applied.is_ok());
    assert!(outcome.persisted.is_err());
    binding.complete(first, Ok(outcome));
    assert!(state.get_ai_agent_editor_open());
    assert!(!state.get_ai_agent_editor_new());
    assert_eq!(state.get_ai_agent_editor_id(), draft.id);
    assert_eq!(binding.facade.snapshot().state().profiles.len(), 1);
    failing.store(false, Ordering::Release);
    let retry = request(&binding, UiAction::Update(draft.clone()), 2, &page);
    let outcome = binding
        .facade
        .update_profile(draft)
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    assert!(outcome.persisted.is_ok());
    binding.complete(retry, Ok(outcome));
    assert!(!state.get_ai_agent_editor_open());
    assert_eq!(binding.facade.snapshot().state().profiles.len(), 1);
    binding.facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn completed_create_preserves_newer_raw_object_source_even_when_normalized_headers_match() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for raw_edit in [false, true] {
        let (_window, page, binding) = setup(vec![profile("a"), profile("b")]);
        let state = PageState::get(&page);
        assert!(page.get_route_active());
        state.invoke_open_new_ai_agent();
        state.set_ai_agent_editor_name("Created object Agent".into());
        state.set_ai_agent_editor_base_url("https://fixture.example/v1".into());
        state.set_ai_agent_editor_model("object-model".into());
        state.invoke_select_ai_agent_header_mode(true);
        state.invoke_edit_ai_agent_header_object("{Authorization:'synthetic-header'}".into());
        let draft = validated_ai_agent_profile_from_page(&page).unwrap();
        let pending = request(&binding, UiAction::Create(draft.clone()), 1, &page);
        let original_revision = state.get_ai_agent_editor_headers_input_revision();
        let equivalent_source = "{ Authorization: 'synthetic-header', }";
        if raw_edit {
            state.invoke_edit_ai_agent_header_object(equivalent_source.into());
            assert!(state.get_ai_agent_editor_headers_input_revision() > original_revision);
            assert_eq!(validated_ai_agent_profile_from_page(&page).unwrap(), draft);
        }
        state.set_ai_agent_editor_busy(true);
        let outcome = binding
            .facade
            .create_profile(draft.clone())
            .unwrap()
            .unwrap()
            .wait()
            .unwrap();
        assert!(outcome.applied.is_ok());
        assert!(outcome.persisted.is_ok());
        binding.complete(pending, Ok(outcome));
        let saved = binding.facade.snapshot();
        assert_eq!(saved.state().profile(&draft.id), Some(&draft));
        assert_eq!(saved.state().active_id, "a");
        assert_eq!(saved.state().profile("a"), Some(&profile("a")));
        assert_eq!(saved.state().profile("b"), Some(&profile("b")));
        assert!(page.get_route_active());
        assert!(!state.get_ai_agent_editor_busy());
        assert_eq!(state.get_ai_agent_editor_open(), raw_edit);
        if raw_edit {
            assert!(
                !state.get_ai_agent_editor_new(),
                "An applied Create must retry as Update"
            );
            assert_eq!(state.get_ai_agent_editor_id(), draft.id);
            assert!(state.get_ai_agent_editor_headers_object_mode());
            assert_eq!(
                state.get_ai_agent_editor_headers_object_text(),
                equivalent_source
            );
            assert_eq!(validated_ai_agent_profile_from_page(&page).unwrap(), draft);
            state.invoke_cancel_ai_agent_editor();
        }
        assert!(state.get_ai_agent_editor_headers_object_text().is_empty());
        state.invoke_edit_ai_agent(draft.id.clone().into());
        assert!(!state.get_ai_agent_editor_new());
        assert!(!state.get_ai_agent_editor_headers_object_mode());
        assert_eq!(validated_ai_agent_profile_from_page(&page).unwrap(), draft);
        binding.facade.shutdown();
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
