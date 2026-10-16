//! Saved-Agent probe actions retain row identity and content-owned feedback.

use std::{cell::RefCell, rc::Rc, time::Duration};

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{ComponentHandle, Global, ModelRc, VecModel, platform::PointerEventButton};
use usher_ui_settings::{AiAgentListItem, AppState as PageState, GitHubSettingsPage};

use super::SettingsPageBridge;
use crate::{MainWindow, Route};

fn tick(ms: u64) {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(ms));
}

fn label(key: &str) -> String {
    crate::i18n::tr(key).into_owned()
}

fn button(page: &GitHubSettingsPage, key: &str, index: usize) -> ElementHandle {
    ElementHandle::find_by_accessible_label(page, &label(key))
        .filter(|element| {
            matches!(
                element.accessible_role(),
                Some(AccessibleRole::Button | AccessibleRole::Switch)
            )
        })
        .nth(index)
        .unwrap_or_else(|| panic!("visible action {key} row {index}"))
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
    // Commit the wide layout and its deferred initial-section sync before
    // clicking navigation, as the mounted Settings Tooltip fixture does.
    tick(2);
    let agent_tab =
        ElementHandle::find_by_accessible_label(&page, &label("settings.ai_agent.title"))
            .find(|element| element.accessible_checked().is_some())
            .unwrap();
    agent_tab.mock_single_click(PointerEventButton::Left);
    tick(2);
    assert_eq!(agent_tab.accessible_checked(), Some(true));
    PageState::get(&page).set_ai_agent_list(ModelRc::new(VecModel::from(vec![
        AiAgentListItem {
            id: "saved-a".into(),
            name: "Agent A".into(),
            model: "model-a".into(),
            provider: "custom".into(),
            active: true,
        },
        AiAgentListItem {
            id: "saved-b".into(),
            name: "Agent B".into(),
            model: "model-b".into(),
            provider: "openai".into(),
            active: false,
        },
    ])));
    tick(2);
    assert!(page.get_ai_agent_list_active());
    (window, bridge, page)
}

#[test]
fn list_probe_first_click_and_neighbor_actions_keep_saved_row_identity() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, bridge, page) = fixture();
    let state = PageState::get(&page);
    let calls = Rc::new(RefCell::new(Vec::<(&str, String)>::new()));
    let captured = calls.clone();
    state.on_test_ai_agent(move |id| captured.borrow_mut().push(("test", id.to_string())));
    let captured = calls.clone();
    state.on_activate_ai_agent(move |id| captured.borrow_mut().push(("activate", id.to_string())));
    let captured = calls.clone();
    state.on_edit_ai_agent(move |id| captured.borrow_mut().push(("edit", id.to_string())));
    let captured = calls.clone();
    state.on_delete_ai_agent(move |id| captured.borrow_mut().push(("delete", id.to_string())));
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for (width, origin) in [(1200.0, 0.0), (800.0, 0.0), (800.0, 240.0)] {
            page.set_window_width(width);
            page.set_viewport_width(width);
            page.set_content_origin_x(origin);
            tick(2);
            calls.borrow_mut().clear();
            let test_a = button(&page, "common.test_connection", 0);
            let test_b = button(&page, "common.test_connection", 1);
            assert_eq!(test_b.size(), slint::LogicalSize::new(32.0, 32.0));
            assert!(test_b.absolute_position().x >= origin);
            assert!(test_b.absolute_position().x + 32.0 <= width - 15.5);
            test_b.mock_single_click(PointerEventButton::Left);
            tick(2);
            assert_eq!(&*calls.borrow(), &[("test", "saved-b".to_owned())]);
            assert!(
                !state.get_ai_agent_editor_open(),
                "probe never opens the editor"
            );
            assert!(!state.get_ai_agent_delete_confirm_open());
            test_a.mock_single_click(PointerEventButton::Left);
            button(&page, "settings.ai_agent.activate", 0)
                .mock_single_click(PointerEventButton::Left);
            button(&page, "settings.ai_agent.edit", 1).mock_single_click(PointerEventButton::Left);
            button(&page, "common.delete", 0).mock_single_click(PointerEventButton::Left);
            tick(2);
            assert_eq!(
                &*calls.borrow(),
                &[
                    ("test", "saved-b".to_owned()),
                    ("test", "saved-a".to_owned()),
                    ("activate", "saved-b".to_owned()),
                    ("edit", "saved-b".to_owned()),
                    ("delete", "saved-b".to_owned()),
                ]
            );
            state.set_ai_agent_list_testing_id("saved-b".into());
            tick(2);
            assert_eq!(test_a.accessible_enabled(), Some(false));
            assert_eq!(
                test_b.accessible_enabled(),
                Some(false),
                "loading blocks repeated activation"
            );
            let before = calls.borrow().len();
            test_a.mock_single_click(PointerEventButton::Left);
            test_b.mock_single_click(PointerEventButton::Left);
            tick(2);
            assert_eq!(calls.borrow().len(), before);
            state.set_ai_agent_list_testing_id("".into());
        }
    }
}

fn toast(page: &GitHubSettingsPage) -> ElementHandle {
    ElementHandle::find_by_element_id(page, "GitHubToast::toast-host")
        .next()
        .expect("visible list feedback card")
}

#[test]
fn list_probe_toast_uses_content_bounds_and_dismissal_preserves_semantic_feedback() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_window, bridge, page) = fixture();
    let state = PageState::get(&page);
    let cancels = Rc::new(RefCell::new(0));
    let captured = cancels.clone();
    state.on_cancel_ai_agent_list_test(move || *captured.borrow_mut() += 1);
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for origin in [0.0, 240.0] {
            page.set_window_width(800.0);
            page.set_viewport_width(800.0);
            page.set_content_origin_x(origin);
            page.set_window_height(600.0);
            page.set_viewport_height(560.0);
            page.set_content_origin_y(24.0);
            tick(2);
            let key = "settings.feedback.ai_agent_connected";
            state.set_ai_agent_list_feedback_key(key.into());
            state.set_ai_agent_list_feedback(label(key).into());
            state.set_ai_agent_list_feedback_success(true);
            state.set_ai_agent_list_feedback_open(true);
            tick(2);
            let card = toast(&page);
            let p = card.absolute_position();
            let s = card.size();
            assert!((p.x + s.width / 2.0 - (origin + (800.0 - origin) / 2.0)).abs() < 0.6);
            assert!(p.y + s.height <= 584.0 && p.y + s.height >= 567.5);
            let dismiss = button(&page, "common.dismiss", 0);
            dismiss.mock_single_click(PointerEventButton::Left);
            tick(2);
            assert!(!state.get_ai_agent_list_feedback_open());
            assert_eq!(state.get_ai_agent_list_feedback_key(), key);
            assert_eq!(state.get_ai_agent_list_feedback(), label(key));
            assert!(!state.get_ai_agent_editor_open());
            state.set_ai_agent_list_feedback_open(true);
            // Flush the lazy running binding before advancing the timer's
            // clock; otherwise this advance only starts the reopened timer.
            tick(2);
            tick(2900);
            assert!(
                state.get_ai_agent_list_feedback_open(),
                "reopened feedback remains visible before its deadline"
            );
            tick(101);
            assert!(
                !state.get_ai_agent_list_feedback_open(),
                "list toast expires without erasing its result"
            );
            assert_eq!(state.get_ai_agent_list_feedback_key(), key);
            let before = *cancels.borrow();
            state.set_ai_agent_list_feedback_open(true);
            state.set_ai_agent_editor_open(true);
            tick(2);
            assert!(!page.get_ai_agent_list_active());
            assert!(*cancels.borrow() > before);
            assert!(
                ElementHandle::find_by_element_id(&page, "GitHubSettingsPage::agent-list-toast")
                    .next()
                    .is_none()
            );
            state.set_ai_agent_editor_open(false);
            state.set_ai_agent_list_feedback_open(false);
            tick(2);
            assert!(page.get_ai_agent_list_active());
        }
    }
}
