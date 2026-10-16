//! Compiled Settings factory regressions for independent provider tab drafts.

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::ComponentHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use usher_ui_settings::GitHubSettingsPage;

use super::{SettingsPageBridge, ai_agent_projection};

use crate::{MainWindow, Route};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
}

fn mounted_page(bridge: &SettingsPageBridge) -> GitHubSettingsPage {
    bridge
        .page
        .lock()
        .unwrap()
        .clone()
        .and_then(|page| page.upgrade())
        .expect("mounted Settings factory")
}

fn label(key: &str) -> String {
    crate::i18n::tr(key).to_string()
}

fn click_tab(page: &GitHubSettingsPage, text: &str) {
    ElementHandle::find_by_accessible_label(page, text)
        .find(|element| element.accessible_checked().is_some())
        .unwrap_or_else(|| panic!("visible Settings tab {text:?}"))
        .mock_single_click(PointerEventButton::Left);
    settle();
}

fn assert_selected(page: &GitHubSettingsPage, text: &str) {
    assert!(
        ElementHandle::find_by_accessible_label(page, text)
            .any(|element| element.accessible_checked() == Some(true)),
        "tab {text:?} must stay selected"
    );
}

fn field_value(page: &GitHubSettingsPage, text: &str) -> String {
    ElementHandle::find_by_accessible_label(page, text)
        .find_map(|element| element.accessible_value())
        .unwrap_or_else(|| panic!("visible Settings input {text:?}"))
        .to_string()
}

fn assert_provider_tabs_align_with_field(page: &GitHubSettingsPage, provider: &str) {
    let first_tab = ElementHandle::find_by_accessible_label(page, "Gitee")
        .find(|element| element.accessible_checked().is_some())
        .expect("first provider tab");
    let input_label = format!("{provider} {}", label("settings.code_hosting.api_base_url"));
    let field = ElementHandle::find_by_element_type_name(page, "GitHubTextField")
        .find(|field| {
            field
                .query_descendants()
                .match_id("GitHubTextField::field-input")
                .find_all()
                .into_iter()
                .any(|input| input.accessible_label().as_deref() == Some(input_label.as_str()))
        })
        .expect("selected provider field");
    let border = field
        .query_descendants()
        .match_id("GitHubTextField::field")
        .find_first()
        .expect("provider input border");
    assert!(
        (first_tab.absolute_position().x - border.absolute_position().x).abs() < 0.5,
        "provider tab starts at the field's outer edge"
    );
}

fn type_field(page: &GitHubSettingsPage, text: &str, value: &str) {
    let input = ElementHandle::find_by_accessible_label(page, text)
        .find(|element| element.accessible_value().is_some())
        .unwrap_or_else(|| panic!("editable Settings input {text:?}"));
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
    settle();
    assert_eq!(field_value(page, text), value);
}

fn resize_page(page: &GitHubSettingsPage, width: f32) {
    page.set_viewport_width(width);
    page.set_window_width(width);
    page.set_viewport_height(2200.0);
    page.set_window_height(2200.0);
    settle();
}

#[test]
fn hosting_provider_tabs_keep_endpoint_fields_separate_from_peers_and_ai_tabs() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    let bridge = SettingsPageBridge::default();
    bridge.install(&window);
    window.set_current_route(Route::Settings);
    settle();
    let page = mounted_page(&bridge);
    resize_page(&page, 1200.0);
    let initial_ai = ai_agent_projection::ai_agent_draft_from_page(&page);
    click_tab(&page, &label("settings.code_hosting.title"));
    for provider in ["Gitee", "GitHub", "GitLab"] {
        click_tab(&page, provider);
        type_field(
            &page,
            &format!("{provider} {}", label("settings.code_hosting.api_base_url")),
            &format!("https://{}.draft.invalid/api", provider.to_lowercase()),
        );
        type_field(
            &page,
            &format!("{provider} {}", label("settings.code_hosting.enterprise")),
            &format!("{provider}-draft-enterprise"),
        );
        assert_selected(&page, provider);
        assert_eq!(
            ai_agent_projection::ai_agent_draft_from_page(&page),
            initial_ai
        );
    }
    for locale in ["zh-CN", "en"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        for width in [800.0, 1200.0] {
            resize_page(&page, width);
            for provider in ["GitLab", "Gitee", "GitHub"] {
                click_tab(&page, provider);
                assert_selected(&page, provider);
                assert_provider_tabs_align_with_field(&page, provider);
                assert_eq!(
                    field_value(
                        &page,
                        &format!("{provider} {}", label("settings.code_hosting.api_base_url"))
                    ),
                    format!("https://{}.draft.invalid/api", provider.to_lowercase())
                );
                assert_eq!(
                    field_value(
                        &page,
                        &format!("{provider} {}", label("settings.code_hosting.enterprise"))
                    ),
                    format!("{provider}-draft-enterprise")
                );
                assert_eq!(
                    ai_agent_projection::ai_agent_draft_from_page(&page),
                    initial_ai
                );
            }
        }
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
#[test]
fn stable_settings_factory_binds_list_activation_directly_to_agent_facade() {
    use crate::settings::{AiAgentConfiguration, AiAgentProfile, AiAgentSettings};
    use slint::Global;
    use usher_ui_settings::AppState as PageState;
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let profile = |id: &str| AiAgentProfile {
        id: id.into(),
        name: format!("Fixture {id}"),
        connection: AiAgentSettings {
            model: format!("model-{id}"),
            ..Default::default()
        },
    };
    let facade = crate::ai::settings_state::AiAgentSettingsFacade::memory(AiAgentConfiguration {
        active_id: "a".into(),
        profiles: vec![profile("a"), profile("b")],
    });
    let bridge = SettingsPageBridge::default();
    bridge.configure_ai_agent_settings(facade.clone());
    bridge.install(&window);
    window.set_current_route(Route::Settings);
    settle();
    let page = mounted_page(&bridge);
    PageState::get(&page).invoke_activate_ai_agent("b".into());
    // A following FIFO receipt proves the callback reached the real facade.
    facade
        .update_profile(profile("a"))
        .unwrap()
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(facade.snapshot().state().active_id, "b");
    facade.shutdown();
}

#[test]
fn hosting_more_keeps_selection_endpoints_and_dismissal_isolated_in_both_locales() {
    use slint::Global;
    use usher_ui_settings::GitHubSelectState;

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let bridge = SettingsPageBridge::default();
    bridge.install(&window);
    window.set_current_route(Route::Settings);
    settle();
    let page = mounted_page(&bridge);
    let select = GitHubSelectState::get(&page);
    let open_more = || {
        ElementHandle::find_by_accessible_label(&page, &label("navigation.more_tabs"))
            .find(|element| element.accessible_checked().is_some())
            .expect("hosting More trigger at forced narrow width")
            .mock_single_click(PointerEventButton::Left);
        settle();
        assert!(select.get_open(), "hosting More must open its root popup");
        assert_eq!(select.get_owner(), "settings-hosting-provider-overflow");
    };

    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        bridge.route_changed(&window, Route::Settings);
        resize_page(&page, 1200.0);
        click_tab(&page, &label("settings.code_hosting.title"));
        for provider in ["Gitee", "GitLab"] {
            click_tab(&page, provider);
            type_field(
                &page,
                &format!("{provider} {}", label("settings.code_hosting.api_base_url")),
                &format!("https://{}.overflow.invalid/api", provider.to_lowercase()),
            );
        }
        click_tab(&page, "Gitee");
        assert_selected(&page, "Gitee");

        resize_page(&page, 196.0);
        open_more();
        click_tab(&page, "GitLab");
        assert!(!select.get_open(), "choosing GitLab closes hosting More");
        resize_page(&page, 1200.0);
        assert_selected(&page, "GitLab");
        assert_eq!(
            field_value(
                &page,
                &format!("GitLab {}", label("settings.code_hosting.api_base_url"))
            ),
            "https://gitlab.overflow.invalid/api"
        );
        click_tab(&page, "Gitee");
        assert_eq!(
            field_value(
                &page,
                &format!("Gitee {}", label("settings.code_hosting.api_base_url"))
            ),
            "https://gitee.overflow.invalid/api"
        );

        resize_page(&page, 196.0);
        open_more();
        page.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        page.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Escape.into(),
        });
        settle();
        assert!(!select.get_open(), "Escape dismisses hosting More");
        resize_page(&page, 1200.0);
        assert_selected(&page, "Gitee");

        resize_page(&page, 196.0);
        open_more();
        bridge.route_changed(&window, Route::Home);
        settle();
        assert!(!page.get_route_active());
        assert!(
            !select.get_open(),
            "hiding the factory dismisses hosting More"
        );
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn agent_actions_fit_the_live_window_when_the_sidebar_retains_a_wide_viewport() {
    use slint::{Global, ModelRc, VecModel};
    use std::{cell::Cell, rc::Rc};
    use usher_ui_settings::{AiAgentListItem, AppState as PageState};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    let bridge = SettingsPageBridge::default();
    bridge.install(&window);
    window.set_current_route(Route::Settings);
    settle();
    let page = mounted_page(&bridge);
    let state = PageState::get(&page);
    let edits = Rc::new(Cell::new(0));
    let deletes = Rc::new(Cell::new(0));
    let uses = Rc::new(Cell::new(0));
    let probes = Rc::new(Cell::new(0));
    let edited = edits.clone();
    state.on_edit_ai_agent(move |_| edited.set(edited.get() + 1));
    let deleted = deletes.clone();
    state.on_delete_ai_agent(move |_| deleted.set(deleted.get() + 1));
    let used = uses.clone();
    state.on_activate_ai_agent(move |_| used.set(used.get() + 1));
    let tested = probes.clone();
    state.on_test_ai_agent(move |_| tested.set(tested.get() + 1));

    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        page.set_content_origin_x(0.0);
        resize_page(&page, 1200.0);
        click_tab(&page, &label("settings.ai_agent.title"));
        state.set_ai_agent_list(ModelRc::new(VecModel::from(vec![AiAgentListItem {
            id: "bounded-actions".into(),
            name: "A long Agent name must leave room for every trailing action".into(),
            model: "a-long-model-name-that-must-elide-within-its-own-row".into(),
            provider: "custom".into(),
            active: false,
        }])));
        state.set_ai_agent_total_count(1);
        // The shell can publish a translated origin before shrinking its page.
        // The actual 800px window still bounds the factory's usable right edge.
        page.set_window_width(800.0);
        page.set_viewport_width(800.0);
        page.set_window_height(600.0);
        page.set_viewport_height(600.0);
        page.set_content_origin_x(240.0);
        settle();

        let action = |key: &str| {
            let key = label(key);
            let button = ElementHandle::find_by_accessible_label(&page, &key)
                .find(|element| {
                    matches!(
                        element.accessible_role(),
                        Some(
                            i_slint_backend_testing::AccessibleRole::Button
                                | i_slint_backend_testing::AccessibleRole::Switch
                        )
                    )
                })
                .unwrap_or_else(|| panic!("visible Agent action {key:?}"));
            let position = button.absolute_position();
            let size = button.size();
            assert!(position.x >= 240.0, "{key:?} stays after the sidebar");
            assert!(
                position.x + size.width <= 800.5,
                "{key:?} must fit the live window: x={}, width={}",
                position.x,
                size.width
            );
            if button.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::Switch) {
                assert_eq!(size, slint::LogicalSize::new(48.0, 24.0));
            } else {
                assert!(size.width >= 32.0 && size.height >= 32.0);
            }
            button
        };
        action("settings.ai_agent.add");
        let use_action = action("settings.ai_agent.activate");
        let test_action = action("common.test_connection");
        let edit_action = action("settings.ai_agent.edit");
        let delete_action = action("common.delete");
        assert_eq!(
            use_action.absolute_position().y + use_action.size().height / 2.0,
            edit_action.absolute_position().y + edit_action.size().height / 2.0
        );
        assert_eq!(
            test_action.absolute_position().y,
            edit_action.absolute_position().y
        );
        assert_eq!(
            edit_action.absolute_position().y,
            delete_action.absolute_position().y
        );
        assert!(
            (delete_action.absolute_position().x + delete_action.size().width
                - use_action.absolute_position().x
                - 168.0)
                .abs()
                < 0.5,
            "the four trailing actions reserve exactly 168px"
        );
        assert_eq!(
            test_action.absolute_position().x - use_action.absolute_position().x,
            56.0
        );
        let before = (edits.get(), deletes.get(), uses.get(), probes.get());
        test_action.mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(probes.get(), before.3 + 1, "first pointer click tests");
        assert_eq!(edits.get(), before.0, "test is outside row edit hit target");
        delete_action.mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(deletes.get(), before.1 + 1, "first pointer click deletes");
        assert_eq!(edits.get(), before.0, "row hit target excludes actions");
        edit_action.mock_single_click(PointerEventButton::Left);
        use_action.mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(edits.get(), before.0 + 1);
        assert_eq!(uses.get(), before.2 + 1);
    }
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
