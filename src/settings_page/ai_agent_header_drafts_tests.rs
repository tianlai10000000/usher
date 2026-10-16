use super::*;
use crate::settings::AiAgentProfile;

fn open(page: &GitHubSettingsPage, id: &str, headers: Vec<AiAgentHeader>) {
    super::super::ai_agent_projection::apply_ai_agent_profile_to_page(
        page,
        &AiAgentProfile {
            id: id.into(),
            name: format!("Agent {id}"),
            connection: AiAgentSettings {
                headers,
                ..Default::default()
            },
        },
    );
    AppState::get(page).set_ai_agent_editor_open(true);
}

#[test]
fn modes_preserve_disabled_rows_and_duplicates_and_invalid_sources_stay_editable() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    bind(&page);
    let original = vec![
        AiAgentHeader {
            enabled: true,
            name: "X-Test".into(),
            value: "one".into(),
        },
        AiAgentHeader {
            enabled: false,
            name: "invalid name".into(),
            value: "synthetic-disabled\r\n".into(),
        },
        AiAgentHeader {
            enabled: true,
            name: "X-Test".into(),
            value: "two".into(),
        },
    ];
    open(&page, "a", original.clone());
    let state = AppState::get(&page);
    state.invoke_select_ai_agent_header_mode(true);
    let source = state.get_ai_agent_editor_headers_object_text();
    assert_eq!(normalized(&page).unwrap(), original);
    assert!(!source.contains("synthetic-disabled"));
    state.invoke_select_ai_agent_header_mode(false);
    assert_eq!(rows(&page), original);
    state.invoke_select_ai_agent_header_mode(true);
    let revision = state.get_ai_agent_editor_headers_input_revision();
    state.invoke_edit_ai_agent_header_object("{Authorization: () => 'synthetic-secret'}".into());
    assert!(state.get_ai_agent_editor_headers_input_revision() > revision);
    assert!(!state.get_ai_agent_editor_feedback_open());
    state.invoke_select_ai_agent_header_mode(false);
    assert!(!state.get_ai_agent_editor_headers_object_mode());
    assert!(!state.get_ai_agent_editor_feedback_open());
    assert_eq!(normalized(&page).unwrap(), original);
    assert_eq!(rows(&page), original);
    let parked_source = state.get_ai_agent_editor_headers_object_text();
    state.invoke_select_ai_agent_header_mode(true);
    assert!(state.get_ai_agent_editor_headers_object_mode());
    assert!(state.get_ai_agent_editor_feedback_open());
    assert_eq!(normalized(&page), Err(INVALID_OBJECT));
    assert_eq!(
        state.get_ai_agent_editor_headers_object_text(),
        parked_source
    );
    assert_eq!(rows(&page), original);
    assert!(
        state
            .get_ai_agent_editor_headers_object_text()
            .contains("synthetic-secret")
    );
    assert!(
        !state
            .get_ai_agent_editor_error()
            .contains("synthetic-secret")
    );
    let english = state.get_ai_agent_editor_error();
    crate::i18n::apply_preference("zh-CN").unwrap();
    crate::ai::settings_ui::sync_language(&page);
    assert_ne!(state.get_ai_agent_editor_error(), english);
    state.invoke_edit_ai_agent_header_object("{'X-New': ['new-one', 'new-two']}".into());
    state.invoke_select_ai_agent_header_mode(false);
    assert!(!state.get_ai_agent_editor_headers_object_mode());
    assert_eq!(rows(&page)[1], original[1]);
    assert_eq!(rows(&page).len(), 3);
    assert_eq!(rows(&page)[0].value, "new-one");
    assert_eq!(rows(&page)[2].value, "new-two");
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn raw_edits_invalidate_probes_and_opening_another_agent_clears_object_secrets() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    bind(&page);
    let canceled = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = canceled.clone();
    AppState::get(&page).on_cancel_ai_agent_test(move || count.set(count.get() + 1));
    open(&page, "a", Vec::new());
    let state = AppState::get(&page);
    state.invoke_select_ai_agent_header_mode(true);
    state.invoke_edit_ai_agent_header_object("{Authorization:'synthetic-secret'}".into());
    let normalized_before = normalized(&page).unwrap();
    let revision = state.get_ai_agent_editor_headers_input_revision();
    state.set_ai_agent_editor_test_result("stale result".into());
    state.set_ai_agent_editor_test_result_key("settings.feedback.ai_agent_connected".into());
    state.set_ai_agent_editor_feedback_open(true);
    state.invoke_edit_ai_agent_header_object("{ Authorization: 'synthetic-secret', }".into());
    assert_eq!(normalized(&page).unwrap(), normalized_before);
    assert!(state.get_ai_agent_editor_headers_input_revision() > revision);
    assert!(state.get_ai_agent_editor_test_result().is_empty());
    assert!(!state.get_ai_agent_editor_feedback_open());
    assert_eq!(canceled.get(), 3);
    open(&page, "b", Vec::new());
    assert!(!state.get_ai_agent_editor_headers_object_mode());
    assert!(state.get_ai_agent_editor_headers_object_text().is_empty());
    assert!(rows(&page).is_empty());
    assert_eq!(state.get_ai_agent_editor_id(), "b");
}

#[test]
fn object_mode_uses_the_same_header_security_boundary_and_caps_total_retained_rows() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    bind(&page);
    open(&page, "a", Vec::new());
    let state = AppState::get(&page);
    state.invoke_select_ai_agent_header_mode(true);
    for source in [
        "{Host:'evil.example'}",
        "{'X-Test':'synthetic\\r\\nInjected: yes'}",
        "{'bad name':'synthetic'}",
    ] {
        state.invoke_edit_ai_agent_header_object(source.into());
        assert_eq!(
            normalized(&page),
            Err("settings.feedback.ai_agent_invalid_headers")
        );
        state.invoke_select_ai_agent_header_mode(false);
        assert!(!state.get_ai_agent_editor_headers_object_mode());
        assert!(normalized(&page).unwrap().is_empty());
        assert!(state.get_ai_agent_editor_error().is_empty());
        state.invoke_select_ai_agent_header_mode(true);
        assert!(state.get_ai_agent_editor_headers_object_mode());
        assert!(state.get_ai_agent_editor_feedback_open());
        assert_eq!(
            normalized(&page),
            Err("settings.feedback.ai_agent_invalid_headers")
        );
    }
    state.invoke_edit_ai_agent_header_object(
        "{Authorization:'Bearer synthetic', 'X-Test':['one','two']}".into(),
    );
    assert_eq!(normalized(&page).unwrap().len(), 3);
    let disabled = (0..32)
        .map(|_| AiAgentHeader {
            enabled: false,
            ..Default::default()
        })
        .collect();
    open(&page, "b", disabled);
    state.invoke_select_ai_agent_header_mode(true);
    state.invoke_edit_ai_agent_header_object("{'X-Test':'one'}".into());
    assert_eq!(normalized(&page), Err(INVALID_OBJECT));
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn parked_invalid_objects_survive_noops_and_real_row_edits_supersede_them() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for source in ["", "{X: () => 'synthetic-parked-secret'}"] {
        for mutation in 0..5 {
            let page = GitHubSettingsPage::new().unwrap();
            bind(&page);
            let original = vec![
                AiAgentHeader {
                    enabled: true,
                    name: "X-Original".into(),
                    value: "original".into(),
                },
                AiAgentHeader {
                    enabled: false,
                    name: "disabled".into(),
                    value: "retained".into(),
                },
            ];
            open(&page, "a", original.clone());
            let state = AppState::get(&page);
            state.invoke_select_ai_agent_header_mode(true);
            state.invoke_edit_ai_agent_header_object(source.into());
            let revision = state.get_ai_agent_editor_headers_input_revision();
            state.invoke_select_ai_agent_header_mode(false);
            assert!(!state.get_ai_agent_editor_headers_object_mode());
            assert!(state.get_ai_agent_editor_headers_input_revision() > revision);
            assert_eq!(state.get_ai_agent_editor_headers_object_text(), source);
            assert!(state.get_ai_agent_editor_error().is_empty());
            assert!(!state.get_ai_agent_editor_feedback_open());
            assert_eq!(
                crate::settings_page::ai_agent_projection::validated_ai_agent_profile_from_page(
                    &page
                )
                .unwrap()
                .connection
                .headers,
                original
            );
            let parked_revision = state.get_ai_agent_editor_headers_input_revision();
            state.invoke_set_ai_agent_header_name(0, "X-Original".into());
            state.invoke_set_ai_agent_header_value(0, "original".into());
            state.invoke_set_ai_agent_header_enabled(0, true);
            state.invoke_select_ai_agent_header_mode(false);
            assert_eq!(
                state.get_ai_agent_editor_headers_input_revision(),
                parked_revision
            );
            state.invoke_select_ai_agent_header_mode(true);
            assert!(state.get_ai_agent_editor_headers_object_mode());
            assert_eq!(state.get_ai_agent_editor_headers_object_text(), source);
            assert_eq!(state.get_ai_agent_editor_error_key(), INVALID_OBJECT);
            assert!(state.get_ai_agent_editor_feedback_open());
            assert_eq!(
                crate::settings_page::ai_agent_projection::validated_ai_agent_profile_from_page(
                    &page
                ),
                Err(INVALID_OBJECT)
            );
            state.invoke_select_ai_agent_header_mode(false);
            match mutation {
                0 => state.invoke_set_ai_agent_header_name(0, "X-Changed".into()),
                1 => state.invoke_set_ai_agent_header_value(0, "changed".into()),
                2 => state.invoke_set_ai_agent_header_enabled(0, false),
                3 => {
                    state.invoke_add_ai_agent_header();
                    state.invoke_set_ai_agent_header_name(2, "X-New".into());
                    state.invoke_set_ai_agent_header_value(2, "new".into());
                }
                _ => state.invoke_remove_ai_agent_header(0),
            }
            let changed = rows(&page);
            assert_ne!(changed, original);
            assert!(state.get_ai_agent_editor_headers_object_text().is_empty());
            state.invoke_select_ai_agent_header_mode(true);
            assert_eq!(
                state.get_ai_agent_editor_headers_object_text(),
                ai_agent_header_object::format(&changed)
            );
            assert_eq!(normalized(&page).unwrap(), changed);
            assert!(!state.get_ai_agent_editor_feedback_open());
        }
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn busy_guards_and_editor_reset_preserve_mode_ownership_and_retire_parked_secrets() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    bind(&page);
    let canceled = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = canceled.clone();
    let state = AppState::get(&page);
    state.on_cancel_ai_agent_test(move || count.set(count.get() + 1));
    open(&page, "a", Vec::new());
    state.invoke_select_ai_agent_header_mode(true);
    let source = "{X: () => 'synthetic-session-secret'}";
    state.invoke_edit_ai_agent_header_object(source.into());
    let revision = state.get_ai_agent_editor_headers_input_revision();
    let prior_canceled = canceled.get();
    state.set_ai_agent_editor_busy(true);
    state.invoke_select_ai_agent_header_mode(false);
    state.invoke_edit_ai_agent_header_object("{}".into());
    state.invoke_add_ai_agent_header();
    assert!(state.get_ai_agent_editor_headers_object_mode());
    assert_eq!(state.get_ai_agent_editor_headers_object_text(), source);
    assert_eq!(state.get_ai_agent_editor_headers_input_revision(), revision);
    assert_eq!(canceled.get(), prior_canceled);
    state.set_ai_agent_editor_busy(false);
    state.set_ai_agent_editor_test_result_key("settings.feedback.ai_agent_connected".into());
    state.set_ai_agent_editor_test_result("stale probe result".into());
    state.set_ai_agent_editor_feedback_open(true);
    state.invoke_select_ai_agent_header_mode(false);
    assert_eq!(canceled.get(), prior_canceled + 1);
    assert!(state.get_ai_agent_editor_test_result().is_empty());
    assert!(!state.get_ai_agent_editor_feedback_open());
    assert_eq!(state.get_ai_agent_editor_headers_object_text(), source);
    super::super::ai_agent_drafts::close(&page);
    assert!(!state.get_ai_agent_editor_open());
    assert!(state.get_ai_agent_editor_headers_object_text().is_empty());
    open(&page, "b", Vec::new());
    state.invoke_select_ai_agent_header_mode(true);
    assert_eq!(state.get_ai_agent_editor_id(), "b");
    assert_eq!(
        state.get_ai_agent_editor_headers_object_text(),
        ai_agent_header_object::format(&[])
    );
    assert!(normalized(&page).unwrap().is_empty());
    assert!(!state.get_ai_agent_editor_feedback_open());
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn explicit_provider_selection_replaces_only_known_presets_and_preserves_custom_drafts() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    super::super::ai_agent_drafts::bind(&page, None);
    let state = AppState::get(&page);
    state.set_ai_agent_editor_name("Named draft".into());
    state.set_ai_agent_editor_model("custom-model".into());
    state.set_ai_agent_editor_api_key("synthetic-key".into());
    state.invoke_add_ai_agent_header();
    state.invoke_set_ai_agent_header_name(0, "X-Test".into());
    state.invoke_set_ai_agent_header_value(0, "synthetic-header".into());
    let headers = rows(&page);
    state.invoke_select_ai_agent_provider("deepseek".into());
    assert_eq!(
        state.get_ai_agent_editor_base_url(),
        "https://api.deepseek.com"
    );
    state.invoke_select_ai_agent_provider("openai".into());
    assert_eq!(
        state.get_ai_agent_editor_base_url(),
        "https://api.openai.com/v1"
    );
    state.set_ai_agent_editor_base_url("https://api.openai.com/v1/".into());
    state.invoke_select_ai_agent_provider("deepseek".into());
    assert_eq!(
        state.get_ai_agent_editor_base_url(),
        "https://api.deepseek.com"
    );
    state.invoke_select_ai_agent_provider("custom".into());
    assert_eq!(
        state.get_ai_agent_editor_base_url(),
        "https://api.deepseek.com"
    );
    state.set_ai_agent_editor_base_url("https://custom.fixture/v1".into());
    state.invoke_select_ai_agent_provider("openai".into());
    assert_eq!(
        state.get_ai_agent_editor_base_url(),
        "https://custom.fixture/v1"
    );
    assert_eq!(state.get_ai_agent_editor_provider(), "openai");
    assert_eq!(state.get_ai_agent_editor_name(), "Named draft");
    assert_eq!(state.get_ai_agent_editor_model(), "custom-model");
    assert_eq!(state.get_ai_agent_editor_api_key(), "synthetic-key");
    assert_eq!(rows(&page), headers);
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
