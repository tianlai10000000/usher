//! Header controls keep their tab, row identity and dialog-local draft ownership.

use super::*;
use crate::ai::settings_state::AiAgentSettingsFacade;
use crate::settings::{AiAgentConfiguration, AiAgentHeader, AiAgentProfile, AiAgentSettings};
use i_slint_backend_testing::ElementHandle;
use slint::Model;
use slint::platform::{PointerEventButton, WindowEvent};
use std::time::Duration;

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(120));
}

fn header(name: &str, value: &str, enabled: bool) -> AiAgentHeader {
    AiAgentHeader {
        enabled,
        name: name.into(),
        value: value.into(),
    }
}

fn fixture() -> (
    MainWindow,
    SettingsPageBridge,
    GitHubSettingsPage,
    AiAgentSettingsFacade,
) {
    let window = MainWindow::new().unwrap();
    crate::i18n::initialize(&window, "en").unwrap();
    window
        .window()
        .set_size(slint::LogicalSize::new(800.0, 600.0));
    let profiles = [
        ("empty", vec![]),
        (
            "rows",
            vec![
                header("X-First", "synthetic-first", true),
                header("X-Middle", "synthetic-middle", false),
                header("X-Last", "synthetic-last", true),
            ],
        ),
        ("peer", vec![header("X-Peer", "synthetic-peer", true)]),
    ]
    .into_iter()
    .map(|(id, headers)| AiAgentProfile {
        id: id.into(),
        name: format!("Agent {id}"),
        connection: AiAgentSettings {
            headers,
            ..Default::default()
        },
    })
    .collect();
    let facade = AiAgentSettingsFacade::memory(AiAgentConfiguration {
        active_id: "peer".into(),
        profiles,
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
    page.set_window_width(800.0);
    page.set_window_height(600.0);
    page.set_viewport_width(800.0);
    page.set_viewport_height(600.0);
    page.set_content_origin_x(0.0);
    settle();
    (window, bridge, page, facade)
}

fn find(page: &GitHubSettingsPage, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(page, id)
        .next()
        .unwrap_or_else(|| panic!("visible compiled Header control {id}"))
}

fn control(page: &GitHubSettingsPage, id: &str) -> ElementHandle {
    find(page, &format!("GitHubAiAgentHeaders::{id}"))
}

fn right(element: &ElementHandle) -> f32 {
    element.absolute_position().x + element.size().width
}

fn center_x(element: &ElementHandle) -> f32 {
    element.absolute_position().x + element.size().width / 2.0
}

fn bottom(element: &ElementHandle) -> f32 {
    element.absolute_position().y + element.size().height
}

fn open(window: &MainWindow, page: &GitHubSettingsPage, id: &str) {
    SettingsAppState::get(page).invoke_edit_ai_agent(id.into());
    settle();
    scroll_to_bottom(window, page);
}

fn scroll_to_bottom(window: &MainWindow, page: &GitHubSettingsPage) {
    let body = find(page, "GitHubAiAgentEditorDialog::editor-scroll");
    window
        .window()
        .dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(
                body.absolute_position().x + 24.0,
                body.absolute_position().y + 24.0,
            ),
            delta_x: 0.0,
            delta_y: -2000.0,
        });
    settle();
}

fn draft_headers(page: &GitHubSettingsPage) -> Vec<AiAgentHeader> {
    SettingsAppState::get(page)
        .get_ai_agent_editor_headers()
        .iter()
        .map(|row| AiAgentHeader {
            enabled: row.enabled,
            name: row.name.to_string(),
            value: row.value.to_string(),
        })
        .collect()
}

#[test]
fn header_add_belongs_to_the_rows_tab_content_and_centers_in_both_locales() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, bridge, page, facade) = fixture();
    let committed = facade.snapshot();
    let state = SettingsAppState::get(&page);
    for locale in ["en", "zh-CN", "en"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for id in ["empty", "rows"] {
            open(&window, &page, id);
            let tabs = control(&page, "mode-tabs");
            let toolbar = control(&page, "rows-toolbar");
            let add = control(&page, "add-header");
            assert!((toolbar.size().height - 32.0).abs() < 0.5);
            assert!(toolbar.absolute_position().y >= bottom(&tabs));
            assert!((center_x(&add) - center_x(&toolbar)).abs() < 0.5);
            assert!(add.absolute_position().y >= toolbar.absolute_position().y);
            assert!(bottom(&add) <= bottom(&toolbar) + 0.5);
            assert_eq!(
                add.accessible_label().unwrap(),
                crate::i18n::tr("settings.ai_agent.add_header").as_ref()
            );
            control(&page, "object-mode").mock_single_click(PointerEventButton::Left);
            settle();
            assert!(state.get_ai_agent_editor_headers_object_mode());
            for id in ["rows-toolbar", "add-header"] {
                assert!(
                    ElementHandle::find_by_element_id(
                        &page,
                        &format!("GitHubAiAgentHeaders::{id}")
                    )
                    .next()
                    .is_none(),
                    "JS object content must not expose {id}"
                );
            }
            control(&page, "rows-mode").mock_single_click(PointerEventButton::Left);
            settle();
            assert!(!state.get_ai_agent_editor_headers_object_mode());
            assert!(
                (center_x(&control(&page, "add-header"))
                    - center_x(&control(&page, "rows-toolbar")))
                .abs()
                    < 0.5
            );
            assert_eq!(facade.snapshot().state(), committed.state());
            assert_eq!(facade.snapshot().revision(), committed.revision());
        }
    }
    facade.shutdown();
}

#[test]
fn add_and_edit_scroll_the_whole_body_with_padding_inside_both_endpoints() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, bridge, page, facade) = fixture();
    let committed = facade.snapshot();
    let state = SettingsAppState::get(&page);
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for creating in [false, true] {
            if creating {
                state.invoke_open_new_ai_agent();
            } else {
                state.invoke_edit_ai_agent("rows".into());
            }
            settle();
            assert_eq!(state.get_ai_agent_editor_new(), creating);
            let surface = find(&page, "GitHubDialog::dialog-surface");
            let body = find(&page, "GitHubAiAgentEditorDialog::editor-scroll");
            let first_field = find(&page, "GitHubAiAgentEditorDialog::name-field");
            assert!(
                (body.absolute_position().y - surface.absolute_position().y - 56.0).abs() < 0.5
            );
            assert!((bottom(&surface) - bottom(&body) - 64.0).abs() < 0.5);
            assert!(
                (first_field.absolute_position().y - body.absolute_position().y - 16.0).abs() < 0.5
            );
            if creating {
                for index in 0..3 {
                    state.invoke_add_ai_agent_header();
                    state.invoke_set_ai_agent_header_name(index, format!("X-Draft-{index}").into());
                    state.invoke_set_ai_agent_header_value(index, "synthetic-draft".into());
                }
            }
            scroll_to_bottom(&window, &page);
            let last_row =
                ElementHandle::find_by_element_id(&page, "GitHubAiAgentHeaders::header-row")
                    .last()
                    .expect("last visible Header row after scrolling to the content endpoint");
            assert!((last_row.size().height - 40.0).abs() < 0.5);
            assert!((bottom(&body) - bottom(&last_row) - 16.0).abs() < 0.5);
            assert_eq!(facade.snapshot().state(), committed.state());
            assert_eq!(facade.snapshot().revision(), committed.revision());
            state.invoke_cancel_ai_agent_editor();
        }
    }
    facade.shutdown();
}

#[test]
fn header_delete_is_a_row_right_icon_and_first_click_removes_only_that_draft_row() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (window, bridge, page, facade) = fixture();
    let committed = facade.snapshot();
    let original = committed
        .state()
        .profile("rows")
        .unwrap()
        .connection
        .headers
        .clone();
    let state = SettingsAppState::get(&page);
    for locale in ["en", "zh-CN", "en"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        open(&window, &page, "rows");
        state.set_ai_agent_editor_name("Unsaved name".into());
        let edge = right(&control(&page, "rows-toolbar"));
        let remove = ElementHandle::find_by_element_id(&page, "GitHubAiAgentHeaderRow::remove")
            .collect::<Vec<_>>();
        assert_eq!(remove.len(), 3);
        for button in &remove {
            assert!((button.size().width - 32.0).abs() < 0.5);
            assert!((button.size().height - 32.0).abs() < 0.5);
            assert!((right(button) - edge).abs() < 0.5);
            assert_eq!(
                button.accessible_label().unwrap(),
                crate::i18n::tr("common.delete").as_ref()
            );
        }
        find(&page, "GitHubAiAgentHeaderRow::name-field")
            .mock_single_click(PointerEventButton::Left);
        remove[1].mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(
            draft_headers(&page),
            vec![original[0].clone(), original[2].clone()]
        );
        assert_eq!(state.get_ai_agent_editor_name(), "Unsaved name");
        assert_eq!(state.get_ai_agent_editor_id(), "rows");
        assert_eq!(facade.snapshot().state(), committed.state());
        assert_eq!(facade.snapshot().revision(), committed.revision());
        open(&window, &page, "peer");
        assert_eq!(
            draft_headers(&page),
            committed
                .state()
                .profile("peer")
                .unwrap()
                .connection
                .headers
        );
        open(&window, &page, "rows");
        assert_eq!(draft_headers(&page), original);
    }
    for _ in 0..crate::settings::MAX_AI_AGENT_HEADERS {
        state.invoke_add_ai_agent_header();
    }
    assert_eq!(
        state.get_ai_agent_editor_headers().row_count(),
        crate::settings::MAX_AI_AGENT_HEADERS
    );
    let capped = draft_headers(&page);
    let revision = state.get_ai_agent_editor_headers_input_revision();
    state.invoke_add_ai_agent_header();
    state.set_ai_agent_editor_busy(true);
    state.invoke_remove_ai_agent_header(0);
    state.invoke_add_ai_agent_header();
    assert_eq!(draft_headers(&page), capped);
    assert_eq!(state.get_ai_agent_editor_headers_input_revision(), revision);
    assert_eq!(facade.snapshot().state(), committed.state());
    facade.shutdown();
}
