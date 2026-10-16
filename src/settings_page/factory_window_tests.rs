use super::*;
use i_slint_core::{item_tree::ItemTreeRc, window::WindowInner};
use std::{rc::Rc, time::Duration};

fn shares_renderer(window: &MainWindow, page: &GitHubSettingsPage) -> bool {
    Rc::ptr_eq(
        &WindowInner::from_pub(window.window()).window_adapter(),
        &WindowInner::from_pub(page.window()).window_adapter(),
    )
}

#[test]
fn standalone_constructor_is_a_negative_control_for_host_cache_ownership() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let host = WindowInner::from_pub(window.window());
    let shell = host.component();
    let standalone = GitHubSettingsPage::new().unwrap();
    assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    assert!(
        !shares_renderer(&window, &standalone),
        "the old constructor must fail the embedded renderer ownership contract"
    );
    drop(standalone);
    assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
}

#[test]
fn shared_constructor_restores_shell_before_repeated_create_and_drop() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let host = WindowInner::from_pub(window.window());
    let shell = host.component();
    for _ in 0..20 {
        let page = create_page(&window).unwrap();
        assert!(shares_renderer(&window, &page));
        assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
        page.set_route_active(false);
        host.ensure_tree_instantiated();
        page.set_route_active(true);
        host.ensure_tree_instantiated();
        drop(page);
        assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    }
}

#[test]
fn actual_factory_keeps_host_renderer_and_shell_through_route_reentry() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let host = WindowInner::from_pub(window.window());
    let shell = host.component();
    let bridge = SettingsPageBridge::default();
    bridge.configure_ai_agent_settings(crate::ai::settings_state::AiAgentSettingsFacade::memory(
        crate::settings::AiAgentConfiguration::default(),
    ));
    bridge.install(&window);
    for (locale, width) in [("zh-CN", 1200.0), ("en", 550.0), ("zh-CN", 1200.0)] {
        crate::i18n::apply_preference(locale).unwrap();
        window.set_current_route(crate::Route::Settings);
        bridge.route_changed(&window, crate::Route::Settings);
        host.ensure_tree_instantiated();
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
        let page = bridge
            .page
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|page| page.upgrade())
            .expect("real Settings factory product");
        page.set_viewport_width(width);
        assert!(shares_renderer(&window, &page));
        assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
        let state = SettingsAppState::get(&page);
        state.set_ai_agent_editor_open(true);
        host.ensure_tree_instantiated();
        state.set_ai_agent_editor_open(false);
        host.ensure_tree_instantiated();
        window.set_current_route(crate::Route::Home);
        bridge.route_changed(&window, crate::Route::Home);
        host.ensure_tree_instantiated();
        assert!(!page.get_route_active());
        assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
