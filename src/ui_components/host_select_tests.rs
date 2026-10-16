//! Shared Select policy controls on a real Settings product in MainWindow.
use crate::MainWindow;
use i_slint_core::window::WindowInner;
use slint::ComponentHandle;
use std::time::Duration;
use usher_ui_settings::GitHubSettingsPage;

#[test]
fn settings_select_tab_restoration_is_opt_in_and_preserves_legacy_host_policy() {
    use i_slint_backend_testing::ElementHandle;
    use i_slint_core::input::FocusReason;
    use slint::platform::{Key, PointerEventButton, WindowEvent};
    use usher_ui_settings::GitHubSelectState;

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.invoke_restore_navigation_drawer_state(false);
    window
        .window()
        .set_size(slint::LogicalSize::new(600.0, 800.0));
    let host = WindowInner::from_pub(window.window());
    let shell = host.component();
    let page = GitHubSettingsPage::new_with_existing_window(window.window()).unwrap();
    host.set_component(&shell);
    page.set_viewport_width(600.0);
    page.set_window_width(600.0);
    page.set_viewport_height(800.0);
    page.set_window_height(800.0);
    let weak = page.as_weak();
    window.set_settings_page_factory(slint::ComponentFactory::new(move |_| weak.upgrade()));
    window.set_current_route(crate::Route::Settings);
    let settle = || i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    settle();
    let key = |text: slint::SharedString| {
        window
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        window
            .window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    };
    // Only Settings is installed: no playground or Inbox mounted-host Tab
    // adapter may hide a change in ordinary Slint Tab behavior.
    let select = page.global::<GitHubSelectState>();
    assert!(
        !select.get_restore_before_tab(),
        "ordinary hosts keep the default policy"
    );
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        settle();
        let trigger = ElementHandle::find_by_element_id(
            &window,
            "GitHubSettingsPage::general-section-trigger",
        )
        .next()
        .expect("compact Settings section opener");
        trigger.mock_single_click(PointerEventButton::Left);
        assert!(page.global::<GitHubSelectState>().get_open());
        key(Key::Escape.into());
        let opener = host
            .focus_item
            .borrow()
            .upgrade()
            .expect("restored Settings opener");
        for (text, shift) in [(Key::Tab, false), (Key::Tab, true), (Key::Backtab, false)] {
            let dismiss = |policy: Option<bool>| {
                if let Some(policy) = policy {
                    select.set_restore_before_tab(policy);
                }
                host.set_focus_item(&opener, true, FocusReason::Programmatic);
                key(Key::Return.into());
                assert!(select.get_open());
                if shift {
                    window.window().dispatch_event(WindowEvent::KeyPressed {
                        text: Key::Shift.into(),
                    });
                }
                key(text.into());
                if shift {
                    window.window().dispatch_event(WindowEvent::KeyReleased {
                        text: Key::Shift.into(),
                    });
                }
                assert!(!select.get_open());
                host.focus_item
                    .borrow()
                    .upgrade()
                    .expect("ordinary host retains a focus target")
            };
            assert!(!select.get_restore_before_tab());
            let default_target = dismiss(None);
            assert!(!select.get_restore_focus());
            let explicit_false_target = dismiss(Some(false));
            assert!(!select.get_restore_focus());
            assert_eq!(
                default_target, explicit_false_target,
                "{locale}: {text:?}, shift={shift}; default policy must preserve explicit close(false)"
            );
            assert_ne!(
                default_target, opener,
                "the legacy policy must not restore the opener"
            );
            eprintln!(
                "legacy Settings target: locale={locale}, key={text:?}, shift={shift}, item={default_target:?}"
            );
            assert_eq!(
                dismiss(Some(true)),
                opener,
                "explicit restoration without a host flush stops on the opener"
            );
            select.set_restore_before_tab(false);
        }
    }
}
