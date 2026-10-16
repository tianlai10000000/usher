//! Fixed Agent action identity, viewport bounds and stale-publisher lifecycle.

use std::{cell::Cell, rc::Rc, time::Duration};

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{ComponentHandle, Global, ModelRc, VecModel, platform::WindowEvent};
use usher_ui_settings::{
    AgentTooltipKind as Kind, AgentTooltipState, AiAgentListItem, AppState as PageState,
    GitHubSettingsPage,
};

use super::SettingsPageBridge;
use crate::{MainWindow, Route};

fn tick(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn copy(key: &str) -> String {
    crate::i18n::tr(key).into_owned()
}

fn fixture() -> (MainWindow, SettingsPageBridge, GitHubSettingsPage) {
    let window = MainWindow::new().unwrap();
    // The Rust catalog outlives each test's fresh Slint translation context.
    // Serializing locale changes alone does not reset either side's selection.
    crate::i18n::initialize(&window, "en").unwrap();
    window
        .window()
        .set_size(slint::LogicalSize::new(1200.0, 900.0));
    let bridge = SettingsPageBridge::default();
    bridge.install(&window);
    window.set_current_route(Route::Settings);
    tick(2);
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    page.set_content_origin_x(0.0);
    page.set_content_origin_y(0.0);
    resize(&page, 1200.0);
    ElementHandle::find_by_accessible_label(&page, &copy("settings.ai_agent.title"))
        .find(|element| element.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(slint::platform::PointerEventButton::Left);
    let rows = (0..10)
        .map(|index| AiAgentListItem {
            id: format!("agent-{index}").into(),
            name: format!("Agent {index}").into(),
            model: "fixture-model".into(),
            provider: "custom".into(),
            active: index == 0,
        })
        .collect::<Vec<_>>();
    PageState::get(&page).set_ai_agent_list(ModelRc::new(VecModel::from(rows)));
    tick(2);
    assert!(AgentTooltipState::get(&page).get_enabled());
    (window, bridge, page)
}

fn resize(page: &GitHubSettingsPage, width: f32) {
    page.set_window_width(width);
    page.set_viewport_width(width);
    page.set_window_height(900.0);
    page.set_viewport_height(900.0);
    tick(2);
}

fn button(page: &GitHubSettingsPage, key: &str, row: usize) -> ElementHandle {
    ElementHandle::find_by_accessible_label(page, &copy(key))
        .filter(|element| {
            matches!(
                element.accessible_role(),
                Some(AccessibleRole::Button | AccessibleRole::Switch)
            )
        })
        .nth(row)
        .unwrap_or_else(|| panic!("visible Agent button {key}, row {row}"))
}

fn hover(page: &GitHubSettingsPage, button: &ElementHandle) {
    let position = button.absolute_position();
    let size = button.size();
    page.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(
            position.x + size.width / 2.0,
            position.y + size.height / 2.0,
        ),
    });
    tick(2);
}

fn leave(page: &GitHubSettingsPage) {
    page.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(1.0, 1.0),
    });
    tick(2);
}

fn surface(page: &GitHubSettingsPage, name: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(
        page,
        &format!("GitHubAiAgentTooltipOverlay::surface-{name}"),
    )
    .next()
    .unwrap_or_else(|| panic!("visible fixed Agent tooltip surface {name}"))
}

fn assert_idle(page: &GitHubSettingsPage) {
    assert_eq!(AgentTooltipState::get(page).get_kind(), Kind::None);
    for name in [
        "add",
        "activate",
        "current",
        "test-connection",
        "edit",
        "delete",
        "deletedisabled",
    ] {
        assert!(
            ElementHandle::find_by_element_id(
                page,
                &format!("GitHubAiAgentTooltipOverlay::surface-{name}")
            )
            .next()
            .is_none(),
            "idle portal must hide the full-window geometry subtree"
        );
    }
}

#[test]
fn agent_tooltip_literals_match_actions_and_window_bounds_in_both_locales() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, bridge, page) = fixture();
    for locale in ["en", "zh-CN", "en"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for width in [1200.0, 550.0, 800.0] {
            leave(&page);
            resize(&page, width);
            for (key, row, kind, fixed) in [
                ("settings.ai_agent.add", 0, Kind::Add, "add"),
                ("settings.ai_agent.activate", 0, Kind::Activate, "activate"),
                (
                    "settings.ai_agent.active_badge",
                    0,
                    Kind::Current,
                    "current",
                ),
                (
                    "common.test_connection",
                    1,
                    Kind::TestConnection,
                    "test-connection",
                ),
                ("settings.ai_agent.edit", 1, Kind::Edit, "edit"),
                ("common.delete", 0, Kind::Delete, "delete"),
                (
                    "settings.ai_agent.delete_active_disabled",
                    0,
                    Kind::DeleteDisabled,
                    "deletedisabled",
                ),
                ("settings.ai_agent.edit", 0, Kind::Edit, "edit"),
            ] {
                let action = button(&page, key, row);
                hover(&page, &action);
                assert_eq!(AgentTooltipState::get(&page).get_kind(), kind, "{key}");
                let tip = surface(&page, fixed);
                let position = tip.absolute_position();
                let size = tip.size();
                assert!(position.x >= 7.5 && position.y >= 7.5);
                assert!(position.x + size.width <= width - 7.5);
                assert!(position.y + size.height <= 892.5);
                assert!(size.width > 20.0 && size.height >= 24.0);
                assert!(
                    ElementHandle::find_by_accessible_label(&page, &copy(key))
                        .any(|element| element.accessible_role() == Some(AccessibleRole::Text)),
                    "fixed tooltip paints the same semantic label as its action"
                );
                if matches!(kind, Kind::Current | Kind::DeleteDisabled) {
                    assert_eq!(action.accessible_enabled(), Some(false));
                }
                if matches!(kind, Kind::Activate | Kind::Current) {
                    assert_eq!(action.accessible_role(), Some(AccessibleRole::Switch));
                    assert_eq!(action.size(), slint::LogicalSize::new(48.0, 24.0));
                    assert_eq!(action.accessible_checked(), Some(kind == Kind::Current));
                }
            }
        }
    }
    leave(&page);
    assert_idle(&page);
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn agent_tooltip_stale_tokens_cannot_steal_claims_and_unleased_portals_expire() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, _bridge, page) = fixture();
    let state = AgentTooltipState::get(&page);
    for _ in 0..20 {
        let previous = state.invoke_show(Kind::Edit, 100.0, 100.0, 32.0, 32.0);
        let current = state.invoke_show(Kind::DeleteDisabled, 1180.0, 880.0, 32.0, 32.0);
        let lease = state.get_lease();
        state.invoke_hide(previous);
        state.invoke_renew(previous, Kind::Edit, 0.0, 0.0, 32.0, 32.0);
        assert_eq!(state.get_kind(), Kind::DeleteDisabled);
        assert_eq!(state.get_token(), current);
        assert_eq!(state.get_lease(), lease);
        tick(2);
        let tip = surface(&page, "deletedisabled");
        assert!(tip.absolute_position().x + tip.size().width <= 1192.5);
        assert!(tip.absolute_position().y + tip.size().height <= 892.5);
        for _ in 0..3 {
            tick(49);
        }
        assert_idle(&page);
        state.invoke_renew(current, Kind::DeleteDisabled, 1100.0, 800.0, 32.0, 32.0);
        assert_idle(&page);
    }
    state.invoke_show(Kind::Edit, 100.0, 100.0, 32.0, 32.0);
    state.invoke_dismiss();
    assert_idle(&page);
}

#[test]
fn agent_activation_toggle_admits_only_turning_on_an_inactive_agent() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    // Reproduce a previous test leaving the process catalog in another locale.
    usher_i18n::activate_locale("zh-CN").unwrap();
    let (_window, _bridge, page) = fixture();
    let first = ElementHandle::find_by_accessible_label(&page, "Agent 0")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .unwrap();
    let second = ElementHandle::find_by_accessible_label(&page, "Agent 1")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .unwrap();
    assert_eq!(first.size().height, 72.0);
    assert_eq!(
        second.absolute_position().y - first.absolute_position().y,
        72.0
    );
    let frame = ElementHandle::find_by_element_id(&page, "GitHubAiAgentList::group-frame")
        .next()
        .unwrap();
    assert_eq!(frame.size().height, 10.0 * 72.0 + 2.0);
    let activations = Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = activations.clone();
    PageState::get(&page)
        .on_activate_ai_agent(move |id| captured.borrow_mut().push(id.to_string()));
    let current = button(&page, "settings.ai_agent.active_badge", 0);
    let inactive = button(&page, "settings.ai_agent.activate", 0);
    assert_eq!(current.accessible_checked(), Some(true));
    assert_eq!(current.accessible_enabled(), Some(false));
    current.mock_single_click(slint::platform::PointerEventButton::Left);
    assert!(
        activations.borrow().is_empty(),
        "the active toggle cannot request deactivation"
    );
    inactive.mock_single_click(slint::platform::PointerEventButton::Left);
    assert_eq!(&*activations.borrow(), &["agent-1"]);
    assert_eq!(
        current.accessible_checked(),
        Some(true),
        "the domain remains authoritative until activation commits"
    );
}

#[test]
fn agent_tooltip_dismisses_on_filter_page_scroll_dialog_route_and_first_click() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    usher_i18n::activate_locale("zh-CN").unwrap();
    let (window, bridge, page) = fixture();
    let state = PageState::get(&page);
    let edits = Rc::new(Cell::new(0));
    let captured = edits.clone();
    state.on_edit_ai_agent(move |_| captured.set(captured.get() + 1));
    let edit = button(&page, "settings.ai_agent.edit", 0);
    hover(&page, &edit);
    edit.mock_single_click(slint::platform::PointerEventButton::Left);
    tick(2);
    assert_eq!(
        edits.get(),
        1,
        "an unfocused icon activates on the first press"
    );
    assert_idle(&page);
    for boundary in 0..5 {
        leave(&page);
        let edit = button(&page, "settings.ai_agent.edit", 0);
        hover(&page, &edit);
        assert_eq!(AgentTooltipState::get(&page).get_kind(), Kind::Edit);
        match boundary {
            0 => state.set_ai_agent_query("new filter".into()),
            1 => state.set_ai_agent_page(2),
            2 => {
                let position = edit.absolute_position();
                page.window().dispatch_event(WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(position.x + 16.0, position.y + 16.0),
                    delta_x: 0.0,
                    delta_y: -64.0,
                });
            }
            3 => state.set_ai_agent_editor_open(true),
            _ => bridge.route_changed(&window, Route::Home),
        }
        tick(2);
        assert_idle(&page);
        tick(80);
        assert_idle(&page);
        if boundary == 3 {
            state.set_ai_agent_editor_open(false);
            tick(2);
        }
    }
}
