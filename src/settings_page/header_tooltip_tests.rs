//! Modal Header Delete uses its own fixed body-bounded Tooltip claim.

use std::{cell::Cell, rc::Rc, time::Duration};

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{ComponentHandle, Global, ModelRc, VecModel, platform::WindowEvent};
use usher_ui_settings::{
    AgentHeaderTooltipState, AiAgentHeaderItem, AppState as PageState, GitHubSettingsPage,
};

use super::SettingsPageBridge;
use crate::{MainWindow, Route};

fn tick(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn headers(value: &str) -> ModelRc<AiAgentHeaderItem> {
    ModelRc::new(VecModel::from(vec![AiAgentHeaderItem {
        enabled: true,
        name: "X-Fixture".into(),
        value: value.into(),
    }]))
}

fn fixture() -> (MainWindow, SettingsPageBridge, GitHubSettingsPage) {
    let window = MainWindow::new().unwrap();
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
    page.set_window_width(1200.0);
    page.set_viewport_width(1200.0);
    page.set_window_height(900.0);
    page.set_viewport_height(900.0);
    tick(2);
    ElementHandle::find_by_accessible_label(&page, "AI Agent")
        .find(|element| element.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(slint::platform::PointerEventButton::Left);
    let state = PageState::get(&page);
    state.set_ai_agent_editor_headers(headers("first"));
    state.set_ai_agent_editor_headers_object_mode(false);
    state.set_ai_agent_editor_open(true);
    tick(2);
    assert!(AgentHeaderTooltipState::get(&page).get_enabled());
    (window, bridge, page)
}

fn body(page: &GitHubSettingsPage) -> ElementHandle {
    ElementHandle::find_by_element_id(page, "GitHubAiAgentEditorDialog::editor-scroll")
        .next()
        .unwrap()
}

fn scroll(page: &GitHubSettingsPage, delta: f32) {
    let body = body(page);
    let p = body.absolute_position();
    let s = body.size();
    page.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0),
        delta_x: 0.0,
        delta_y: delta,
    });
    tick(16);
}

fn delete(page: &GitHubSettingsPage) -> ElementHandle {
    ElementHandle::find_by_element_id(page, "GitHubAiAgentHeaderRow::remove")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("Header's literal Delete icon")
}

fn hover(page: &GitHubSettingsPage, target: &ElementHandle) {
    let p = target.absolute_position();
    let s = target.size();
    page.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0),
    });
    tick(2);
}

fn assert_idle(page: &GitHubSettingsPage) {
    assert!(!AgentHeaderTooltipState::get(page).get_open());
    assert!(
        ElementHandle::find_by_element_id(
            page,
            "GitHubAiAgentHeaderTooltipOverlay::delete-surface"
        )
        .next()
        .is_none()
    );
}

#[test]
fn header_delete_tooltip_matches_its_icon_and_stays_inside_scrolling_body() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, bridge, page) = fixture();
    let removed = Rc::new(Cell::new(None));
    let captured = removed.clone();
    PageState::get(&page).on_remove_ai_agent_header(move |slot| captured.set(Some(slot)));
    for locale in ["en", "zh-CN", "en"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        scroll(&page, -4000.0);
        let action = delete(&page);
        assert_eq!(action.size(), slint::LogicalSize::new(32.0, 32.0));
        hover(&page, &action);
        assert!(AgentHeaderTooltipState::get(&page).get_open());
        let tip = ElementHandle::find_by_element_id(
            &page,
            "GitHubAiAgentHeaderTooltipOverlay::delete-surface",
        )
        .next()
        .unwrap();
        let body = body(&page);
        let p = body.absolute_position();
        let s = body.size();
        assert!(tip.absolute_position().x >= p.x + 7.5);
        assert!(tip.absolute_position().y >= p.y + 7.5);
        assert!(tip.absolute_position().x + tip.size().width <= p.x + s.width - 7.5);
        assert!(tip.absolute_position().y + tip.size().height <= p.y + s.height - 7.5);
        let label = crate::i18n::tr("common.delete");
        assert_eq!(action.accessible_label().as_deref(), Some(label.as_ref()));
        assert!(
            ElementHandle::find_by_accessible_label(&page, label.as_ref())
                .any(|element| element.accessible_role() == Some(AccessibleRole::Text))
        );
        action.mock_single_click(slint::platform::PointerEventButton::Left);
        tick(2);
        assert_eq!(removed.get(), Some(0));
        assert_idle(&page);
        page.window().dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(1.0, 1.0),
        });
        tick(2);
        hover(&page, &action);
        scroll(&page, 64.0);
        assert_idle(&page);
    }
    PageState::get(&page).set_ai_agent_editor_open(false);
    tick(2);
    assert_idle(&page);
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn header_tooltip_rejects_stale_claims_and_cancels_on_editor_boundaries() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, _bridge, page) = fixture();
    let state = AgentHeaderTooltipState::get(&page);
    let draft = PageState::get(&page);
    let body = body(&page);
    let p = body.absolute_position();
    for _ in 0..20 {
        let old = state.invoke_show(p.x + 16.0, p.y + 16.0, 32.0, 32.0);
        let current = state.invoke_show(p.x + 80.0, p.y + 16.0, 32.0, 32.0);
        let lease = state.get_lease();
        state.invoke_hide(old);
        state.invoke_renew(old, 0.0, 0.0, 32.0, 32.0);
        assert!(state.get_open());
        assert_eq!(state.get_token(), current);
        assert_eq!(state.get_lease(), lease);
        for _ in 0..3 {
            tick(49);
        }
        assert_idle(&page);
        state.invoke_renew(current, p.x, p.y, 32.0, 32.0);
        assert_idle(&page);
    }
    for boundary in 0..5 {
        let old = state.invoke_show(p.x + 16.0, p.y + 16.0, 32.0, 32.0);
        match boundary {
            0 => draft.set_ai_agent_editor_headers(headers("replaced")),
            1 => draft.set_ai_agent_editor_headers_input_revision(
                draft.get_ai_agent_editor_headers_input_revision() + 1,
            ),
            2 => draft.set_ai_agent_editor_headers_object_mode(true),
            3 => draft.set_ai_agent_editor_busy(true),
            _ => draft.set_ai_agent_editor_open(false),
        }
        tick(2);
        assert_idle(&page);
        state.invoke_renew(old, p.x, p.y, 32.0, 32.0);
        assert_idle(&page);
        if boundary == 2 {
            draft.set_ai_agent_editor_headers_object_mode(false);
            tick(2);
        }
        if boundary == 3 {
            draft.set_ai_agent_editor_busy(false);
            tick(2);
        }
    }
    assert!(!state.get_enabled());
}
