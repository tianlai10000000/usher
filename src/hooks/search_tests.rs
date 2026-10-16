//! Search projection, stable row actions and the mounted Settings toolbar.

use super::*;
use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::{ComponentHandle, Model, platform::PointerEventButton};
use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

fn search_state() -> HookState {
    let definitions = [
        (
            "ai-hook",
            "Release AI",
            "ai.review.completed.v1",
            HookLanguage::JavaScriptTypeScript,
        ),
        (
            "hidden-hook",
            "Unrelated",
            PR_REVIEW_APPROVED,
            HookLanguage::Shell,
        ),
        (
            "pr-one",
            "Release one",
            PR_REVIEW_APPROVED,
            HookLanguage::Shell,
        ),
        ("pr-two", "Release two", PR_MERGED, HookLanguage::Shell),
    ]
    .into_iter()
    .map(|(id, name, event, language)| HookDefinition {
        id: id.into(),
        name: name.into(),
        event: event.into(),
        language,
        ..hook("echo fixture")
    })
    .collect::<Vec<_>>();
    HookState {
        ready: true,
        enabled: true,
        hooks: definitions.into(),
        events: vec![
            EventDefinition {
                name: PR_REVIEW_APPROVED.into(),
                category: CATEGORY_PR_REVIEW.into(),
                project_scoped: true,
                sample: serde_json::json!({}),
            },
            EventDefinition {
                name: PR_MERGED.into(),
                category: CATEGORY_PR_REVIEW.into(),
                project_scoped: true,
                sample: serde_json::json!({}),
            },
            EventDefinition {
                name: "ai.review.completed.v1".into(),
                category: CATEGORY_AI_REVIEW.into(),
                project_scoped: true,
                sample: serde_json::json!({}),
            },
        ]
        .into(),
        ..Default::default()
    }
}

fn tick() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
}

fn open_hooks(page: &GitHubSettingsPage) {
    page.set_window_width(1200.0);
    page.set_viewport_width(1200.0);
    page.set_window_height(900.0);
    page.set_viewport_height(900.0);
    tick();
    ElementHandle::find_by_accessible_label(page, "Hooks")
        .find(|element| element.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    tick();
}

#[test]
fn hook_search_matches_metadata_and_rebuilds_only_visible_group_geometry() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let _page = GitHubSettingsPage::new().unwrap();
    let state = search_state();
    crate::i18n::apply_preference("en").unwrap();
    for (query, expected) in [
        ("  RELEASE  ", vec!["pr-one", "pr-two", "ai-hook"]),
        (PR_MERGED, vec!["pr-two"]),
        ("AI CODE REVIEW", vec!["ai-hook"]),
        ("JavaScript", vec!["ai-hook"]),
        ("Pull request approved", vec!["hidden-hook", "pr-one"]),
        ("no matching metadata", vec![]),
        ("   ", vec!["hidden-hook", "pr-one", "pr-two", "ai-hook"]),
    ] {
        let rows = bridge::hook_summaries(&state, query);
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            expected,
            "query {query:?}"
        );
    }
    let rows = bridge::hook_summaries(&state, "release");
    assert!(rows[0].category_start && !rows[0].category_end);
    assert!(!rows[1].category_start && rows[1].category_end);
    assert_eq!(rows[0].category_size, 2);
    assert_eq!(rows[1].category_index, 0);
    assert_eq!(rows[2].category_index, 1);
    assert_eq!(rows[2].category_size, 1);
    assert!(rows[2].category_start && rows[2].category_end);
    let ai_only = bridge::hook_summaries(&state, "JavaScript");
    assert_eq!(
        ai_only[0].category_index, 0,
        "filtered-out groups leave no gap"
    );

    crate::i18n::apply_preference("zh-CN").unwrap();
    let rows = bridge::hook_summaries(&state, "审核通过");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "hidden-hook");
    assert_eq!(rows[1].id, "pr-one");
    let rows = bridge::hook_summaries(&state, "AI 评审");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "ai-hook");
    assert_eq!(rows[0].category_index, 0);
    assert_eq!(state.hooks.len(), 4);
    assert_eq!(
        state.hooks[0].id, "ai-hook",
        "search does not reorder durable state"
    );
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn filtered_hook_row_body_and_trailing_actions_use_the_saved_id_on_the_first_click() {
    use std::{cell::RefCell, rc::Rc};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    open_hooks(&page);
    let global = page.global::<HookSettingsState>();
    global.set_query("Release two".into());
    bridge::apply_settings(&page, &search_state());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = calls.clone();
    global.on_choose(move |kind, id| {
        observed
            .borrow_mut()
            .push((kind.to_string(), id.to_string()))
    });
    let observed = calls.clone();
    global.on_request_delete(move |id| {
        observed
            .borrow_mut()
            .push(("delete".into(), id.to_string()))
    });
    let observed = calls.clone();
    global.on_set_hook_enabled(move |id, enabled| {
        observed
            .borrow_mut()
            .push((format!("enabled:{enabled}"), id.to_string()))
    });
    tick();
    assert_eq!(global.get_summaries().row_count(), 1);
    let body = ElementHandle::find_by_accessible_label(&page, "Release two")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("filtered row body");
    let toggle = ElementHandle::find_by_accessible_label(&page, "Enable this Hook: Release two")
        .next()
        .unwrap();
    assert!(body.absolute_position().x + body.size().width < toggle.absolute_position().x);
    for label in [
        "Edit Hook: Release two",
        "Delete: Release two",
        "Enable this Hook: Release two",
    ] {
        // Move focus away before each action so a stale row focus cannot hide a
        // first-click hit-target regression.
        ElementHandle::find_by_accessible_label(&page, "Search Hooks")
            .find(|element| element.accessible_value().is_some())
            .unwrap()
            .mock_single_click(PointerEventButton::Left);
        ElementHandle::find_by_accessible_label(&page, label)
            .next()
            .unwrap()
            .mock_single_click(PointerEventButton::Left);
    }
    body.mock_single_click(PointerEventButton::Left);
    assert_eq!(
        calls.borrow().as_slice(),
        &[
            ("hook".into(), "pr-two".into()),
            ("delete".into(), "pr-two".into()),
            ("enabled:false".into(), "pr-two".into()),
            ("hook".into(), "pr-two".into()),
        ]
    );
    assert_eq!(global.get_query(), "Release two");
}

#[test]
fn mounted_hook_search_filters_immediately_and_plus_stays_aligned_after_locale_and_width_changes() {
    use slint::platform::{Key, WindowEvent};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = crate::MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    let (_dir, service) = service();
    let fixtures = search_state();
    for event in fixtures.events.iter().skip(1) {
        service.register_event(event.clone()).unwrap();
    }
    for definition in fixtures.hooks.iter() {
        service.save(definition.clone()).unwrap();
    }
    bridge::bind_settings_with_service(&window, &page, service.clone());
    open_hooks(&page);
    let global = page.global::<HookSettingsState>();
    for width in [1200.0, 550.0, 800.0] {
        page.set_window_width(width);
        page.set_viewport_width(width);
        tick();
        // Crossing the responsive breakpoint may replace the section subtree.
        // Locale-only updates below must preserve the input within that layout.
        let mounted_input = ElementHandle::find_by_accessible_label(
            &page,
            crate::i18n::tr("hooks.search").as_ref(),
        )
        .find(|element| element.accessible_value().is_some())
        .unwrap();
        let search_field =
            ElementHandle::find_by_element_id(&page, "GitHubHookSettingsSection::search-field")
                .next()
                .unwrap();
        for locale in ["en", "zh-CN", "en"] {
            crate::i18n::apply_preference(locale).unwrap();
            bridge::apply_settings(&page, service.state.snapshot().state());
            tick();
            assert!(
                mounted_input.is_valid(),
                "locale switches keep the mounted input"
            );
            assert_eq!(
                mounted_input.accessible_label().unwrap(),
                crate::i18n::tr("hooks.search").as_ref()
            );
            let add = ElementHandle::find_by_accessible_label(
                &page,
                crate::i18n::tr("hooks.new").as_ref(),
            )
            .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
            .unwrap();
            assert!((add.size().width - 32.0).abs() < 0.5);
            assert!((add.size().height - 32.0).abs() < 0.5);
            assert!(
                mounted_input.absolute_position().x + mounted_input.size().width
                    < add.absolute_position().x
            );
            assert!(add.absolute_position().x + add.size().width <= width);
            assert!(
                (search_field.absolute_position().y + search_field.size().height / 2.0
                    - add.absolute_position().y
                    - add.size().height / 2.0)
                    .abs()
                    < 1.0
            );

            let type_query = |query: &str| {
                mounted_input.mock_single_click(PointerEventButton::Left);
                let previous = mounted_input.accessible_value().unwrap();
                mounted_input.set_accessible_selection_offsets(0, previous.len() as i32);
                for text in [slint::SharedString::from(Key::Backspace), query.into()] {
                    page.window()
                        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
                    page.window()
                        .dispatch_event(WindowEvent::KeyReleased { text });
                }
                tick();
            };
            type_query("Release two");
            assert_eq!(global.get_query(), "Release two");
            assert_eq!(global.get_summaries().row_count(), 1);
            assert_eq!(global.get_summaries().row_data(0).unwrap().id, "pr-two");
            bridge::apply_settings(&page, service.state.snapshot().state());
            assert_eq!(
                global.get_query(),
                "Release two",
                "refresh preserves the query"
            );
            type_query("no matching metadata");
            assert_eq!(global.get_summaries().row_count(), 0);
            assert!(
                ElementHandle::find_by_accessible_label(
                    &page,
                    crate::i18n::tr("hooks.search_empty").as_ref()
                )
                .next()
                .is_some()
            );
            add.mock_single_click(PointerEventButton::Left);
            assert_eq!(global.get_dialog_mode(), "add");
            assert_eq!(global.get_query(), "no matching metadata");
            global.invoke_cancel();
            global.set_busy(true);
            assert_eq!(add.accessible_enabled(), Some(false));
            global.set_busy(false);
            global.set_ready(false);
            assert_eq!(add.accessible_enabled(), Some(false));
            global.set_ready(true);
            assert_eq!(add.accessible_enabled(), Some(true));
        }
    }
    service.state.shutdown();
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}
