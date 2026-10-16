//! Real factory geometry and feedback ownership in both packaged locales.

use super::*;
use crate::settings::{AiAgentConfiguration, AiAgentProfile, AiAgentSettings};
use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use std::time::Duration;

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
}

fn fixture() -> (
    MainWindow,
    SettingsPageBridge,
    GitHubSettingsPage,
    crate::ai::settings_state::AiAgentSettingsFacade,
) {
    let window = MainWindow::new().unwrap();
    window
        .window()
        .set_size(slint::LogicalSize::new(800.0, 600.0));
    let facade = crate::ai::settings_state::AiAgentSettingsFacade::memory(AiAgentConfiguration {
        active_id: "saved".into(),
        profiles: vec![AiAgentProfile {
            id: "saved".into(),
            name: "Saved Agent".into(),
            connection: AiAgentSettings {
                base_url: "https://fixture.invalid/v1".into(),
                model: "fixture-model".into(),
                ..Default::default()
            },
        }],
    });
    let bridge = SettingsPageBridge::default();
    bridge.configure_ai_agent_settings(facade.clone());
    bridge.install(&window);
    window.set_current_route(crate::Route::Settings);
    settle();
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    SettingsAppState::get(&page).invoke_edit_ai_agent("saved".into());
    settle();
    (window, bridge, page, facade)
}

fn element(page: &GitHubSettingsPage, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(page, id)
        .next()
        .unwrap_or_else(|| panic!("visible compiled element {id}"))
}

fn editor(page: &GitHubSettingsPage, id: &str) -> ElementHandle {
    element(page, &format!("GitHubAiAgentEditorDialog::{id}"))
}

fn right(element: &ElementHandle) -> f32 {
    element.absolute_position().x + element.size().width
}

fn bottom(element: &ElementHandle) -> f32 {
    element.absolute_position().y + element.size().height
}

fn center_y(element: &ElementHandle) -> f32 {
    element.absolute_position().y + element.size().height / 2.0
}

fn scroll_body(window: &MainWindow, page: &GitHubSettingsPage) {
    let body = editor(page, "editor-scroll");
    window
        .window()
        .dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(
                body.absolute_position().x + 24.0,
                body.absolute_position().y + 24.0,
            ),
            delta_x: 0.0,
            delta_y: -280.0,
        });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
}

fn resize(page: &GitHubSettingsPage, width: f32, height: f32, origin: f32) {
    page.set_window_width(width);
    page.set_window_height(height);
    page.set_viewport_width(width);
    page.set_viewport_height(height);
    page.set_content_origin_x(origin);
    settle();
}

#[test]
fn form_fields_leave_scrollbar_gutter_and_reveal_label_shares_switch_centerline() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, bridge, page, facade) = fixture();
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for (width, height, origin) in [
            (800.0, 600.0, 0.0),
            (800.0, 600.0, 240.0),
            (1200.0, 900.0, 240.0),
        ] {
            window
                .window()
                .set_size(slint::LogicalSize::new(width, height));
            resize(&page, width, height, origin);
            SettingsAppState::get(&page).invoke_edit_ai_agent("saved".into());
            settle();
            let body = editor(&page, "editor-scroll");
            let form = editor(&page, "form");
            let track = ElementHandle::find_by_element_id(&page, "GitHubScrollView::scrollbar-hit")
                .find(|track| {
                    (right(track) - right(&body)).abs() < 0.5
                        && track.absolute_position().y >= body.absolute_position().y - 0.5
                        && bottom(track) <= bottom(&body) + 0.5
                })
                .expect("editor body has its actual overlay scrollbar");
            assert!(track.size().width >= 7.5);
            assert!(track.absolute_position().x - right(&form) >= 7.5);
            let mut seen_fields = std::collections::BTreeSet::new();
            for pass in 0..2 {
                let fields = form
                    .query_descendants()
                    .match_id("GitHubTextField::field")
                    .find_all();
                assert!(!fields.is_empty(), "inspect real visible field borders");
                for field in fields {
                    assert!(track.absolute_position().x - right(&field) >= 7.5);
                    seen_fields.insert(
                        (field.absolute_position().y - form.absolute_position().y).round() as i32,
                    );
                }
                if pass == 0 {
                    scroll_body(&window, &page);
                }
            }
            assert!(
                seen_fields.len() >= 4,
                "scroll exposes all four field borders"
            );
            let toggle = editor(&page, "reveal-toggle");
            let label = editor(&page, "reveal-label");
            assert!((center_y(&toggle) - center_y(&label)).abs() < 0.5);
        }
    }
    facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn test_spinner_and_outcome_icons_share_the_buttons_trailing_slot() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, bridge, page, facade) = fixture();
    let state = SettingsAppState::get(&page);
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for (testing, key, success) in [
            (false, "", true),
            (true, "settings.feedback.ai_agent_test_failed", false),
            (false, "settings.feedback.ai_agent_connected", true),
            (false, "settings.feedback.ai_agent_test_failed", false),
        ] {
            state.set_ai_agent_editor_testing(testing);
            state.set_ai_agent_editor_test_result_key(key.into());
            state.set_ai_agent_editor_test_success(success);
            settle();
            let button = editor(&page, "test-button");
            let spinner =
                ElementHandle::find_by_element_id(&page, "GitHubAiAgentEditorDialog::test-spinner")
                    .next();
            let status =
                ElementHandle::find_by_element_id(&page, "GitHubAiAgentEditorDialog::test-status")
                    .next();
            assert_eq!(spinner.is_some(), testing);
            assert_eq!(status.is_some(), !testing && !key.is_empty());
            if let Some(icon) = spinner.or(status) {
                assert!((icon.absolute_position().x - right(&button) - 8.0).abs() < 0.5);
                assert!((center_y(&icon) - center_y(&button)).abs() < 0.5);
                assert!((icon.size().width - 16.0).abs() < 0.5);
                assert!((icon.size().height - 16.0).abs() < 0.5);
            }
        }
    }
    state.set_ai_agent_editor_testing(false);
    facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn dismissing_bottom_center_feedback_preserves_outcome_draft_and_committed_agents() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, bridge, page, facade) = fixture();
    let state = SettingsAppState::get(&page);
    state.set_ai_agent_editor_name("Unsaved name".into());
    let draft = ai_agent_projection::ai_agent_profile_from_page(&page);
    let committed = facade.snapshot();
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        assert!(
            !state.get_ai_agent_editor_feedback_open(),
            "locale refresh must not reopen dismissed feedback"
        );
        for key in [
            "settings.feedback.ai_agent_connected",
            "settings.feedback.ai_agent_test_failed",
        ] {
            state.set_ai_agent_editor_testing(false);
            state.set_ai_agent_editor_test_result_key(key.into());
            state.set_ai_agent_editor_test_result(crate::i18n::tr(key).into_owned().into());
            state.set_ai_agent_editor_test_success(key.ends_with("connected"));
            state.set_ai_agent_editor_feedback_open(true);
            settle();
            let toast = element(&page, "GitHubToast::toast-host");
            let body = editor(&page, "editor-scroll");
            let footer = editor(&page, "test-button");
            assert!(
                (toast.absolute_position().x + toast.size().width / 2.0
                    - body.absolute_position().x
                    - body.size().width / 2.0)
                    .abs()
                    < 0.5
            );
            assert!(toast.absolute_position().y >= body.absolute_position().y);
            assert!(bottom(&toast) <= bottom(&body) - 7.5);
            assert!(bottom(&toast) < footer.absolute_position().y);
            let position = toast.absolute_position();
            scroll_body(&window, &page);
            assert_eq!(
                toast.absolute_position(),
                position,
                "Toast is outside the scrolling form"
            );
            ElementHandle::find_by_accessible_label(&page, &crate::i18n::tr("common.dismiss"))
                .next()
                .expect("Toast dismiss control")
                .mock_single_click(PointerEventButton::Left);
            settle();
            assert!(!state.get_ai_agent_editor_feedback_open());
            assert_eq!(state.get_ai_agent_editor_test_result_key(), key);
            assert_eq!(
                ai_agent_projection::ai_agent_profile_from_page(&page),
                draft
            );
            assert_eq!(facade.snapshot().revision(), committed.revision());
            assert_eq!(*facade.snapshot().state(), *committed.state());
            assert!(state.get_ai_agent_editor_open());
            assert!(
                ElementHandle::find_by_element_id(&page, "GitHubToast::toast-host")
                    .next()
                    .is_none()
            );
        }
    }
    facade.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
