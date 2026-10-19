//! Select portals retain their real window anchor inside the scrolling editor.

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::{
    ComponentHandle, Global,
    platform::{Key, PointerEventButton, WindowEvent},
};
use usher_ui_settings::{AppState as PageState, GitHubSelectState, GitHubSettingsPage};

use super::SettingsPageBridge;
use crate::{MainWindow, Route};

fn tick() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

fn find(page: &GitHubSettingsPage, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(page, id)
        .next()
        .unwrap_or_else(|| panic!("visible element {id}"))
}

fn scroll(page: &GitHubSettingsPage, delta: f32) {
    let body = find(page, "GitHubAiAgentEditorDialog::editor-scroll");
    let p = body.absolute_position();
    page.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(p.x + 24.0, p.y + 24.0),
        delta_x: 0.0,
        delta_y: delta,
    });
    tick();
}

fn reply_trigger(page: &GitHubSettingsPage) -> ElementHandle {
    ElementHandle::find_by_element_type_name(page, "ManifestReplyLanguagePicker")
        .next()
        .expect("visible reply-language picker")
        .query_descendants()
        .match_type_name("GitHubSettingsSectionMenuButton")
        .find_first()
        .expect("reply-language trigger")
}

fn press(window: &MainWindow, key: Key) {
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    tick();
}

fn pointer_click(window: &MainWindow, position: slint::LogicalPosition) {
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
}

#[test]
fn editor_reply_select_at_footer_keeps_anchor_surface_and_option_identity() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    crate::i18n::initialize(&window, "en").unwrap();
    let bridge = SettingsPageBridge::default();
    bridge.install(&window);
    crate::transient_input::install_for_window(&window).unwrap();
    window.set_current_route(Route::Settings);
    tick();
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for (width, height, sidebar) in [
            (800.0, 600.0, false),
            (800.0, 600.0, true),
            (1200.0, 900.0, true),
        ] {
            window
                .window()
                .set_size(slint::LogicalSize::new(width, height));
            window.set_show_navigation_drawer(sidebar);
            PageState::get(&page).set_ai_agent_editor_open(true);
            tick();
            let body = find(&page, "GitHubAiAgentEditorDialog::editor-scroll");
            let provider = ElementHandle::find_by_element_type_name(
                &page,
                "GitHubAiAgentEditorProviderPicker",
            )
            .next()
            .unwrap()
            .query_descendants()
            .match_type_name("GitHubSettingsSectionMenuButton")
            .find_first()
            .unwrap();
            provider.mock_single_click(PointerEventButton::Left);
            tick();
            let menu = find(&page, "GitHubSelectPopup::menu");
            assert!(
                menu.absolute_position().y
                    >= provider.absolute_position().y + provider.size().height + 3.5
            );
            assert!(
                menu.absolute_position().y + menu.size().height
                    <= body.absolute_position().y + body.size().height - 7.5
            );
            press(&window, Key::End);
            press(&window, Key::Return);
            assert!(!GitHubSelectState::get(&page).get_open());
            assert_eq!(
                PageState::get(&page).get_ai_agent_editor_provider(),
                "openai"
            );
            scroll(&page, -280.0);
            let trigger = reply_trigger(&page);
            let body = find(&page, "GitHubAiAgentEditorDialog::editor-scroll");
            let target = body.absolute_position().y + body.size().height - trigger.size().height;
            scroll(&page, target - trigger.absolute_position().y);
            let trigger = reply_trigger(&page);
            let p = trigger.absolute_position();
            let size = trigger.size();
            trigger.mock_single_click(PointerEventButton::Left);
            for _ in 0..3 {
                tick();
                let state = GitHubSelectState::get(&page);
                assert!(
                    state.get_open(),
                    "menu remains open after release and layout"
                );
                assert_eq!(state.get_owner(), "ManifestReplyLanguagePicker");
                assert!(
                    (state.get_anchor_x() - p.x).abs() < 0.5,
                    "anchor x {} vs trigger {}",
                    state.get_anchor_x(),
                    p.x
                );
                assert!(
                    (state.get_anchor_y() - p.y).abs() < 0.5,
                    "anchor y {} vs trigger {}",
                    state.get_anchor_y(),
                    p.y
                );
                let menu = find(&page, "GitHubSelectPopup::menu");
                let m = menu.absolute_position();
                let s = menu.size();
                assert!(
                    s.width > 100.0 && s.height >= 103.5,
                    "three language options retain nonzero surface: {m:?} {s:?}"
                );
                assert!(
                    m.x >= 7.5 && m.y >= 7.5,
                    "surface {m:?} {s:?} trigger {p:?} {size:?}"
                );
                assert!(
                    m.x + s.width <= width - 7.5 && m.y + s.height <= height - 7.5,
                    "surface {m:?} {s:?} in {width}x{height}"
                );
                if p.y > height / 2.0 {
                    assert!(m.y >= body.absolute_position().y + 7.5);
                    assert!(
                        m.y + s.height <= body.absolute_position().y + body.size().height - 7.5,
                        "the lower-window consumer stays clear of its fixed footer"
                    );
                    assert!(
                        m.y + s.height <= p.y - 3.5,
                        "the lower-window Select must open toward the larger space above"
                    );
                }
                let surface = state.get_surface();
                assert!((surface.x - m.x).abs() < 0.5 && (surface.y - m.y).abs() < 0.5);
            }
            let english = ElementHandle::find_by_accessible_label(&page, "English")
                .find(|option| option.accessible_checked().is_some())
                .expect("English option is actually visible");
            english.mock_single_click(PointerEventButton::Left);
            tick();
            assert!(!GitHubSelectState::get(&page).get_open());
            assert_eq!(
                PageState::get(&page).get_ai_agent_editor_reply_language(),
                "en"
            );
            reply_trigger(&page).mock_single_click(PointerEventButton::Left);
            press(&window, Key::End);
            press(&window, Key::Return);
            assert!(!GitHubSelectState::get(&page).get_open());
            assert_eq!(
                PageState::get(&page).get_ai_agent_editor_reply_language(),
                "zh-CN"
            );
            reply_trigger(&page).mock_single_click(PointerEventButton::Left);
            assert!(window.invoke_settings_hook_dialog_key_pressed(
                Key::Escape.into(),
                false,
                false,
                false,
                false,
            ));
            tick();
            assert!(!GitHubSelectState::get(&page).get_open());
            assert!(
                PageState::get(&page).get_ai_agent_editor_open(),
                "Escape dismisses only Select"
            );
            reply_trigger(&page).mock_single_click(PointerEventButton::Left);
            scroll(&page, 16.0);
            assert!(
                !GitHubSelectState::get(&page).get_open(),
                "scroll dismisses the old anchor"
            );
            PageState::get(&page).set_ai_agent_editor_open(false);
            tick();
        }
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn editor_partially_clipped_reply_select_opens_from_visible_pointer_hit() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    crate::i18n::initialize(&window, "en").unwrap();
    let bridge = SettingsPageBridge::default();
    bridge.install(&window);
    crate::transient_input::install_for_window(&window).unwrap();
    window.set_current_route(Route::Settings);
    tick();
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for sidebar in [false, true] {
            window
                .window()
                .set_size(slint::LogicalSize::new(800.0, 600.0));
            window.set_show_navigation_drawer(sidebar);
            PageState::get(&page).set_ai_agent_editor_open(true);
            tick();
            scroll(&page, -280.0);
            let trigger = reply_trigger(&page);
            let body = find(&page, "GitHubAiAgentEditorDialog::editor-scroll");
            let body_bottom = body.absolute_position().y + body.size().height;
            let half_visible_top = body_bottom - trigger.size().height / 2.0;
            scroll(&page, half_visible_top - trigger.absolute_position().y);

            let trigger = reply_trigger(&page);
            let p = trigger.absolute_position();
            let size = trigger.size();
            let visible_height = body_bottom - p.y;
            assert!(
                p.y > body.absolute_position().y
                    && (visible_height - size.height / 2.0).abs() < 0.5
                    && p.y + size.height > body_bottom + 1.0,
                "precondition: lower half is clipped, locale={locale}, sidebar={sidebar}, \
                 trigger={p:?} {size:?}, body_bottom={body_bottom}"
            );
            let selection = PageState::get(&page).get_ai_agent_editor_reply_language();
            // The trigger's center is clipped. Use its actually visible upper
            // half through the mounted MainWindow, including post-dispatch
            // release forwarding and Flickable's delayed child press.
            pointer_click(
                &window,
                slint::LogicalPosition::new(p.x + size.width / 2.0, p.y + visible_height / 2.0),
            );
            for frame in 0..3 {
                tick();
                let state = GitHubSelectState::get(&page);
                assert!(
                    state.get_open(),
                    "visible-half click opens and retains menu: locale={locale}, \
                     sidebar={sidebar}, frame={frame}"
                );
                assert_eq!(state.get_owner(), "ManifestReplyLanguagePicker");
                assert_eq!(
                    PageState::get(&page).get_ai_agent_editor_reply_language(),
                    selection,
                    "opening does not commit a language"
                );
                let actual_trigger = reply_trigger(&page);
                let anchor = actual_trigger.absolute_position();
                let anchor_size = actual_trigger.size();
                assert!(
                    (anchor.y - p.y).abs() < 0.5,
                    "pointer activation must not scroll the partially clipped trigger: \
                     locale={locale}, sidebar={sidebar}, frame={frame}, before={}, after={}",
                    p.y,
                    anchor.y
                );
                assert!(
                    (state.get_anchor_x() - anchor.x).abs() < 0.5
                        && (state.get_anchor_y() - anchor.y).abs() < 0.5
                        && (state.get_anchor_width() - anchor_size.width).abs() < 0.5
                        && (state.get_anchor_height() - anchor_size.height).abs() < 0.5,
                    "menu retains the actual scrolled trigger anchor"
                );
                let menu = find(&page, "GitHubSelectPopup::menu");
                let m = menu.absolute_position();
                let s = menu.size();
                assert!(
                    s.width > 100.0 && s.height >= 103.5,
                    "all three options have a visible surface: {m:?} {s:?}"
                );
                assert!(
                    m.x >= 7.5 && m.y >= 7.5 && m.x + s.width <= 792.5 && m.y + s.height <= 592.5,
                    "root portal stays inside 800x600: {m:?} {s:?}"
                );
                assert!(
                    m.y + s.height <= anchor.y - 3.5,
                    "footer-adjacent trigger opens toward the larger space above"
                );
                let surface = state.get_surface();
                assert!(
                    (surface.x - m.x).abs() < 0.5
                        && (surface.y - m.y).abs() < 0.5
                        && (surface.width - s.width).abs() < 0.5
                        && (surface.height - s.height).abs() < 0.5,
                    "outside-dismiss surface matches the actually mounted menu"
                );
            }
            PageState::get(&page).set_ai_agent_editor_open(false);
            tick();
            assert!(!GitHubSelectState::get(&page).get_open());
        }
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
