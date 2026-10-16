use super::*;
use std::time::Duration;

#[path = "search_tests.rs"]
mod search_tests;

pub(super) fn service() -> (tempfile::TempDir, Arc<HookService>) {
    service_with_selector(Arc::new(|_, _| false))
}

fn service_with_selector(selector: ProjectHookSelector) -> (tempfile::TempDir, Arc<HookService>) {
    let dir = tempfile::tempdir().unwrap();
    let service = HookService::with_project_selector(
        dir.path().join("hooks.json"),
        dir.path().join("runs"),
        selector,
    )
    .unwrap();
    service
        .register_event(EventDefinition {
            name: PR_REVIEW_APPROVED.into(),
            category: CATEGORY_PR_REVIEW.into(),
            project_scoped: true,
            sample: serde_json::json!({"number": "123"}),
        })
        .unwrap();
    service.state.hydrate().unwrap();
    (dir, service)
}

fn hook(script: &str) -> HookDefinition {
    HookDefinition {
        enabled: true,
        script: script.into(),
        ..Default::default()
    }
}

#[test]
fn hook_editor_defaults_to_the_approved_event() {
    use slint::ComponentHandle;
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    assert_eq!(
        page.global::<HookSettingsState>().get_draft_event(),
        PR_REVIEW_APPROVED
    );
}

#[test]
fn legacy_javascript_and_typescript_languages_load_as_the_combined_runtime() {
    for legacy in [
        "\"java_script\"",
        "\"type_script\"",
        "\"javascript\"",
        "\"typescript\"",
    ] {
        let language: HookLanguage = serde_json::from_str(legacy).unwrap();
        assert_eq!(language, HookLanguage::JavaScriptTypeScript);
    }
    assert_eq!(
        serde_json::to_string(&HookLanguage::JavaScriptTypeScript).unwrap(),
        "\"javascript_typescript\""
    );
}

#[test]
fn legacy_review_submitted_hooks_migrate_to_the_approved_event() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hooks.json");
    let mut legacy = hook("echo approved");
    legacy.event = LEGACY_PR_REVIEW_SUBMITTED.into();
    std::fs::write(
        &path,
        serde_json::to_vec(&HookState {
            hooks: vec![legacy].into(),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    let service = HookService::new(path, dir.path().join("runs")).unwrap();
    service
        .register_event(EventDefinition {
            name: PR_REVIEW_APPROVED.into(),
            category: CATEGORY_PR_REVIEW.into(),
            project_scoped: true,
            sample: serde_json::json!({}),
        })
        .unwrap();
    service.state.hydrate().unwrap();
    assert_eq!(
        service.state.snapshot().state().hooks[0].event,
        PR_REVIEW_APPROVED
    );
    service.state.shutdown();
}

#[test]
fn row_switch_updates_only_the_selected_hook_and_advances_its_revision() {
    let (_dir, service) = service();
    let first = hook("echo first");
    let mut second = hook("echo second");
    second.id = "hook-second".into();
    service.save(first.clone()).unwrap();
    service.save(second.clone()).unwrap();
    service.set_hook_enabled(first.id.clone(), false).unwrap();

    let state = service.state.snapshot();
    let first = state
        .state()
        .hooks
        .iter()
        .find(|hook| hook.id == first.id)
        .unwrap();
    let second = state
        .state()
        .hooks
        .iter()
        .find(|hook| hook.id == second.id)
        .unwrap();
    assert!(!first.enabled);
    assert_eq!(first.revision, 2);
    assert!(second.enabled);
    assert_eq!(second.revision, 1);
    service.state.shutdown();
}

async fn finished(service: &HookService, id: &str) -> HookRun {
    tokio::time::timeout(Duration::from_secs(15), service.wait(id))
        .await
        .expect("Hook must terminate")
        .unwrap()
}

#[tokio::test]
async fn clearing_history_persists_and_preserves_active_runs_and_definitions() {
    let (dir, service) = service();
    let definition = hook("echo retained");
    service.save(definition.clone()).unwrap();
    let trigger = HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({}));
    let mut completed = HookRun::queued(&definition, &trigger);
    completed.conclusion = Some(HookConclusion::Success);
    let pending = HookRun::queued(&definition, &trigger);
    let mut closing = HookRun::queued(&definition, &trigger);
    closing.conclusion = Some(HookConclusion::Success);
    let (cancel, _receiver) = watch::channel(false);
    service
        .active
        .lock()
        .unwrap()
        .insert(closing.id.clone(), cancel);
    for run in [&completed, &pending, &closing] {
        service.state.dispatch(Action::Run(run.clone())).unwrap();
    }
    std::fs::create_dir_all(&service.directory).unwrap();
    let completed_log = service.directory.join(format!("{}.jsonl", completed.id));
    let closing_log = service.directory.join(format!("{}.jsonl", closing.id));
    std::fs::write(&completed_log, "completed").unwrap();
    std::fs::write(&closing_log, "closing").unwrap();
    service.clear_history().await.unwrap();
    let persisted: HookState =
        serde_json::from_slice(&std::fs::read(dir.path().join("hooks.json")).unwrap()).unwrap();
    assert_eq!(persisted.hooks[0].id, definition.id);
    assert_eq!(persisted.runs.len(), 2);
    assert!(persisted.runs.iter().any(|run| run.id == pending.id));
    assert!(persisted.runs.iter().any(|run| run.id == closing.id));
    assert!(!completed_log.exists());
    assert!(closing_log.exists());
    service.active.lock().unwrap().clear();
    service.shutdown().await;
}

#[test]
fn hook_tooltips_keep_literal_labels_and_fit_after_action_changes() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{
        ComponentHandle,
        platform::{PointerEventButton, WindowEvent},
    };
    use usher_ui_settings::{
        GitHubSettingsPage, HookSettingsState, HookSummary, HookTooltipKind, HookTooltipState,
    };
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    let g = page.global::<HookSettingsState>();
    g.set_ready(true);
    g.set_can_clear_history(true);
    g.set_summaries(slint::ModelRc::new(slint::VecModel::from(vec![
        HookSummary {
            id: "one".into(),
            name: "One".into(),
            enabled: true,
            description: "Shell".into(),
            category: "Pull request review".into(),
            category_index: 0,
            category_start: true,
            category_end: true,
            category_size: 1,
        },
    ])));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|e| e.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    for locale in ["en", "zh-CN", "en"] {
        crate::i18n::apply_preference(locale).unwrap();
        for width in [550.0, 1200.0, 550.0] {
            page.set_viewport_width(width);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));
            for _ in 0..3 {
                for (key, row_action, kind, surface) in [
                    ("hooks.new", false, HookTooltipKind::Add, "add-surface"),
                    ("hooks.edit", true, HookTooltipKind::Edit, "edit-surface"),
                    (
                        "common.delete",
                        true,
                        HookTooltipKind::Delete,
                        "delete-surface",
                    ),
                    (
                        "hooks.clear_history",
                        false,
                        HookTooltipKind::ClearHistory,
                        "clearhistory-surface",
                    ),
                    ("hooks.new", false, HookTooltipKind::Add, "add-surface"),
                ] {
                    let text = crate::i18n::tr(key);
                    let label = if row_action {
                        format!("{text}: One")
                    } else {
                        text.to_string()
                    };
                    let button = ElementHandle::find_by_accessible_label(&page, &label)
                        .find(|element| {
                            element.accessible_role()
                                == Some(i_slint_backend_testing::AccessibleRole::Button)
                        })
                        .unwrap();
                    let pos = button.absolute_position();
                    page.window().dispatch_event(WindowEvent::PointerMoved {
                        position: slint::LogicalPosition::new(pos.x + 16.0, pos.y + 16.0),
                    });
                    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
                    assert_eq!(
                        page.global::<HookTooltipState>().get_kind(),
                        kind,
                        "{label}"
                    );
                    let tip = ElementHandle::find_by_element_id(
                        &page,
                        &format!("GitHubHookTooltipOverlay::{surface}"),
                    )
                    .next()
                    .unwrap();
                    assert!(tip.absolute_position().x >= 0.0);
                    assert!(tip.absolute_position().x + tip.size().width <= width);
                    assert!(tip.absolute_position().y >= 0.0);
                    assert!(tip.absolute_position().y + tip.size().height <= 900.0);
                    assert!(tip.size().width > 40.0 && tip.size().height >= 24.0);
                    assert!(
                        ElementHandle::find_by_accessible_label(&page, text.as_ref())
                            .any(|element| element.accessible_role()
                                == Some(i_slint_backend_testing::AccessibleRole::Text)),
                        "fixed surface paints {key} in {locale}"
                    );
                    let expected_center = (pos.x + button.size().width / 2.0).clamp(
                        8.0 + tip.size().width / 2.0,
                        width - 8.0 - tip.size().width / 2.0,
                    );
                    assert!(
                        (tip.absolute_position().x + tip.size().width / 2.0 - expected_center)
                            .abs()
                            <= 1.0
                    );
                }
            }
        }
    }
    page.set_route_active(false);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(
        page.global::<HookTooltipState>().get_kind(),
        HookTooltipKind::None
    );
    assert!(
        ElementHandle::find_by_element_id(&page, "GitHubHookTooltipOverlay::add-surface")
            .next()
            .is_none()
    );
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn hook_history_select_flips_above_the_settings_viewport_bottom() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{ComponentHandle, ModelRc, VecModel, platform::PointerEventButton};
    use usher_ui_settings::{GitHubSettingsPage, HookChoice, HookSettingsState, HookSummary};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_window_width(799.0);
    page.set_window_height(592.0);
    page.set_content_origin_x(240.0);
    page.set_content_origin_y(38.0);
    page.set_viewport_width(559.0);
    page.set_viewport_height(505.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));

    let section_trigger = ElementHandle::find_by_accessible_label(&page, "General")
        .find(|element| element.accessible_expandable().is_some())
        .expect("compact Settings section trigger");
    section_trigger.mock_single_click(PointerEventButton::Left);
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|element| element.accessible_checked().is_some())
        .expect("Hooks section option")
        .mock_single_click(PointerEventButton::Left);

    let state = page.global::<HookSettingsState>();
    state.set_ready(true);
    state.set_summaries(ModelRc::new(VecModel::from(vec![HookSummary {
        id: "hook-one".into(),
        name: "Hook One".into(),
        description: "Pull request approved · Shell".into(),
        enabled: true,
        category: "Pull request review".into(),
        category_index: 0,
        category_start: true,
        category_end: true,
        category_size: 1,
    }])));
    state.set_history(ModelRc::new(VecModel::from(vec![
        HookChoice {
            value: "run-one".into(),
            label: "Run one".into(),
        },
        HookChoice {
            value: "run-two".into(),
            label: "Run two".into(),
        },
        HookChoice {
            value: "run-three".into(),
            label: "Run three".into(),
        },
        HookChoice {
            value: "run-four".into(),
            label: "Run four".into(),
        },
    ])));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(2));

    let scroll = ElementHandle::find_by_element_id(&page, "GitHubSettingsPage::settings-scroll")
        .next()
        .expect("Settings scroll view");
    scroll.scroll(0.0, -10_000.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let trigger = ElementHandle::find_by_accessible_label(&page, "Execution history")
        .find(|element| element.accessible_expandable().is_some())
        .expect("Hook history trigger");
    trigger.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let menu = ElementHandle::find_by_element_id(&page, "GitHubSelectPopup::menu")
        .next()
        .expect("Hook history menu");
    let viewport_bottom = page.get_content_origin_y() + page.get_viewport_height();
    assert!(
        menu.absolute_position().y + menu.size().height <= viewport_bottom - 7.5,
        "history menu must remain above the Settings viewport bottom: trigger={:?}, menu={:?}",
        trigger.absolute_position(),
        menu.absolute_position(),
    );
    assert!(
        menu.absolute_position().y + menu.size().height <= trigger.absolute_position().y - 3.5,
        "history menu near the bottom must flip above its trigger: trigger={:?} size={:?}, menu={:?} size={:?}",
        trigger.absolute_position(),
        trigger.size(),
        menu.absolute_position(),
        menu.size(),
    );
}

#[test]
fn hook_dialog_shortcuts_submit_once_preserve_newlines_and_gate_busy_state() {
    use slint::{ComponentHandle, platform::WindowEvent};
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    let g = page.global::<HookSettingsState>();
    page.set_viewport_height(1200.0);
    g.set_draft_language(0);
    let saves = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = saves.clone();
    g.on_save(move || observed.set(observed.get() + 1));
    let cancels = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = cancels.clone();
    g.on_cancel(move || observed.set(observed.get() + 1));
    g.set_ready(true);
    g.set_dialog_mode("edit".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let key = |value: slint::SharedString, pressed| {
        page.window().dispatch_event(if pressed {
            WindowEvent::KeyPressed { text: value }
        } else {
            WindowEvent::KeyReleased { text: value }
        })
    };
    let submit = || {
        key(slint::platform::Key::Control.into(), true);
        key(slint::platform::Key::Return.into(), true);
        key(slint::platform::Key::Return.into(), false);
        key(slint::platform::Key::Control.into(), false);
    };
    let input = i_slint_backend_testing::ElementHandle::find_by_element_id(
        &page,
        "GitHubHookEditorForm::script-input",
    )
    .next()
    .unwrap();
    input.mock_single_click(slint::platform::PointerEventButton::Left);
    key(slint::platform::Key::Return.into(), true);
    key(slint::platform::Key::Return.into(), false);
    assert_eq!(g.get_draft_script(), "\n");
    assert_eq!(saves.get(), 0);
    submit();
    assert_eq!(saves.get(), 1);
    g.set_busy(true);
    submit();
    key(slint::platform::Key::Escape.into(), true);
    key(slint::platform::Key::Escape.into(), false);
    assert_eq!(saves.get(), 1);
    assert_eq!(cancels.get(), 0);
    g.set_busy(false);
    key(slint::platform::Key::Escape.into(), true);
    key(slint::platform::Key::Escape.into(), false);
    assert_eq!(cancels.get(), 1);
}

#[test]
fn hook_editor_code_view_scrolls_and_reopens_at_the_top() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{ComponentHandle, platform::WindowEvent};
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let script = (0..137)
        .map(|line| format!("echo line-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (_dir, service) = service();
    let saved = hook(&script);
    service.save(saved.clone()).unwrap();
    let window = crate::MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    bridge::bind_settings_with_service(&window, &page, service.clone());
    let state = page.global::<HookSettingsState>();
    state.invoke_choose("hook".into(), saved.id.clone().into());
    assert!(state.get_draft_script_loading());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    assert_eq!(state.get_draft_script(), script);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert!(state.get_code_highlight_ready());

    let input = ElementHandle::find_by_element_id(&page, "GitHubHookEditorForm::script-input")
        .next()
        .expect("code input");
    let inner_text = ElementHandle::find_by_element_type_name(&page, "TextInput")
        .find(|element| element.size().height > 100.0)
        .expect("multiline TextEdit content");
    let highlight = ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
        .next()
        .expect("shared-scroll syntax projection");
    let before = input.absolute_position().y;
    let inner_before = inner_text.absolute_position().y;
    assert_eq!(highlight.absolute_position().y, inner_before);
    assert!((highlight.size().height - inner_text.size().height).abs() <= 0.5);
    let input_position = input.absolute_position();
    let input_size = input.size();
    page.window().dispatch_event(WindowEvent::PointerMoved {
        position: slint::LogicalPosition::new(
            input_position.x + input_size.width / 2.0,
            input_position.y + input_size.height / 2.0,
        ),
    });
    page.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(
            input_position.x + input_size.width / 2.0,
            input_position.y + input_size.height / 2.0,
        ),
        delta_x: 0.0,
        delta_y: -240.0,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));

    let after = input.absolute_position().y;
    let inner_after = inner_text.absolute_position().y;
    assert_eq!(
        after, before,
        "outer dialog form must not move for code wheel"
    );
    assert!(
        inner_after < inner_before,
        "inner TextEdit content should move upward after code wheel"
    );

    // Repeated wheel events must reach the actual end of the native viewport,
    // leaving the final line visible instead of stopping with a clipped tail.
    for _ in 0..8 {
        page.window().dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(
                input_position.x + input_size.width / 2.0,
                input_position.y + input_size.height / 2.0,
            ),
            delta_x: 0.0,
            delta_y: -240.0,
        });
    }
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
    let bottom = inner_text.absolute_position().y + inner_text.size().height;
    let highlight_bottom = highlight.absolute_position().y + highlight.size().height;
    let viewport_bottom = input.absolute_position().y + input.size().height;
    assert!(
        (bottom - (viewport_bottom - 10.0)).abs() <= 2.0,
        "native editor must expose the complete tail above its padding: bottom={bottom}, viewport={viewport_bottom}"
    );
    assert_eq!(highlight_bottom, bottom);
    page.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(
            input_position.x + input_size.width / 2.0,
            input_position.y + input_size.height / 2.0,
        ),
        delta_x: 0.0,
        delta_y: 10000.0,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
    assert_eq!(inner_text.absolute_position().y, inner_before);
    assert_eq!(highlight.absolute_position().y, inner_before);

    // Close the active session while it is still at the bottom. Reopening via
    // the real delayed-hydration path must create fresh text/layout owners and
    // must not restore this viewport.
    page.window().dispatch_event(WindowEvent::PointerScrolled {
        position: slint::LogicalPosition::new(
            input_position.x + input_size.width / 2.0,
            input_position.y + input_size.height / 2.0,
        ),
        delta_x: 0.0,
        delta_y: -10000.0,
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
    assert!(inner_text.absolute_position().y < inner_before);
    state.invoke_cancel();
    assert!(
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::text-input")
            .next()
            .is_none(),
        "closing the dialog must unmount the native editor item tree"
    );
    assert!(
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
            .next()
            .is_none(),
        "closing the dialog must unmount the rich-text projection"
    );
    assert!(
        !highlight.is_valid(),
        "the old renderer owner must be dropped"
    );
    assert!(
        !inner_text.is_valid(),
        "the old native editor must be dropped"
    );

    state.invoke_choose("hook".into(), saved.id.into());
    assert!(state.get_draft_script_loading());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    assert_eq!(state.get_draft_script(), script);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert!(state.get_code_highlight_ready());
    let reopened_text = ElementHandle::find_by_element_id(&page, "HookCodeEditor::text-input")
        .next()
        .expect("fresh native source editor");
    let reopened_highlight =
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
            .next()
            .expect("fresh syntax projection");
    assert!((reopened_text.absolute_position().y - inner_before).abs() <= 0.5);
    assert!((reopened_highlight.absolute_position().y - inner_before).abs() <= 0.5);
    crate::app_runtime::shared_runtime().block_on(service.shutdown());
}

#[test]
fn highlighted_hook_editor_keeps_native_cursor_blink_and_selection() {
    use i_slint_backend_testing::ElementHandle;
    use i_slint_core::{items::TextInput, window::WindowInner};
    use slint::{ComponentHandle, platform::PointerEventButton};
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    let state = page.global::<HookSettingsState>();
    state.set_ready(true);
    state.set_draft_language(1);
    state.set_native_cursor_uses_selection_color(cfg!(target_os = "macos"));
    state.set_draft_script("const value = 42;\nconsole.log(value);".into());
    state.set_dialog_mode("edit".into());
    bridge::update_hook_highlight(&page);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(150));

    let editor = ElementHandle::find_by_element_id(&page, "HookCodeEditor::text-input")
        .next()
        .expect("native source editor");
    let position = editor.absolute_position();
    let position = slint::LogicalPosition::new(position.x + 25.0, position.y + 8.0);
    page.window()
        .dispatch_event(slint::platform::WindowEvent::PointerMoved { position });
    page.window()
        .dispatch_event(slint::platform::WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
    page.window()
        .dispatch_event(slint::platform::WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    let window = WindowInner::from_pub(page.window());
    let focused = window
        .focus_item
        .borrow()
        .upgrade()
        .expect("source must receive focus through rich text");
    let input = focused
        .downcast::<TextInput>()
        .expect("native TextInput focus");
    let input = input.as_pin_ref();
    assert!(state.get_code_highlight_ready());
    assert!(
        !input.color().is_transparent(),
        "transparent text skips the entire native drawing pass"
    );
    assert!(input.text_cursor_width().get() >= 1.0);
    let visible = input.visual_representation();
    assert!(visible.cursor_position.is_some());
    assert_eq!(visible.cursor_color.alpha(), 255);
    if cfg!(target_os = "macos") {
        assert_eq!(input.color().color().alpha(), 1);
    }

    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(501));
    assert!(input.visual_representation().cursor_position.is_none());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(501));
    assert!(input.visual_representation().cursor_position.is_some());
    page.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::Shift.into(),
        });
    page.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
            text: slint::platform::Key::LeftArrow.into(),
        });
    page.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::LeftArrow.into(),
        });
    page.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
            text: slint::platform::Key::Shift.into(),
        });
    let selected = input.visual_representation();
    assert!(!selected.selection_range.is_empty());
    assert!(input.selection_background_color().alpha() > 0);
    assert!(selected.cursor_position.is_none());
    state.set_native_cursor_uses_selection_color(false);
    assert_eq!(
        input.color().color().alpha(),
        255,
        "text-colored cursor platforms must use opaque native editing"
    );
}

#[test]
fn hook_styled_code_preserves_literal_source_and_whitespace() {
    let colors = [slint::Color::from_rgb_u8(207, 34, 46); 6];
    for source in [
        "import crypto from 'node:crypto'\nconsole.log(222)\n",
        "\n\n  const  name = '你好'\n\tif (name) {\n    console.log(name)\n}\n\n",
        "const markup = '<font color=red>**[x]**</font>'\n// # _ * ~ ` \\ & < >",
        "const x = '\"&amp;\"'\r\n// note\r\n",
    ] {
        let styled = bridge::hook_styled_code(source, 1, &colors)
            .expect("bounded literal code must produce styled text");
        assert_eq!(i_slint_core::styled_text::get_raw_text(&styled), source);
        assert_ne!(styled, slint::StyledText::from_plain_text(source));
    }
}

#[test]
fn hook_highlight_styles_tokens_without_changing_source_geometry() {
    use slint::Model;

    let source = "export interface Result {\n  ok: boolean\n}\nconst value = await fetch('https://example.test') // note\n";
    let lines = bridge::hook_code_lines(source, 1);
    assert_eq!(lines.len(), source.split('\n').count());
    assert_eq!(lines.last().unwrap().text, "");

    for (source_line, highlighted) in source.split('\n').zip(&lines) {
        let rendered = highlighted
            .tokens
            .iter()
            .map(|token| token.text.to_string())
            .collect::<String>();
        assert_eq!(rendered, source_line);
    }

    let styles = lines
        .iter()
        .flat_map(|line| line.tokens.iter())
        .map(|token| (token.text.to_string(), token.style))
        .collect::<Vec<_>>();
    assert!(
        styles
            .iter()
            .any(|(text, style)| { text == "export" && *style == bridge::HIGHLIGHT_STYLE_KEYWORD })
    );
    assert!(
        styles
            .iter()
            .any(|(text, style)| { text == "Result" && *style == bridge::HIGHLIGHT_STYLE_ENTITY })
    );
    assert!(styles.iter().any(|(text, style)| {
        text == "'https://example.test'" && *style == bridge::HIGHLIGHT_STYLE_STRING
    }));
    assert!(
        styles.iter().any(|(text, style)| {
            text == "// note" && *style == bridge::HIGHLIGHT_STYLE_COMMENT
        })
    );
    assert!(
        styles
            .iter()
            .any(|(text, style)| text.contains("value") && *style == 0)
    );

    let over_budget = (0..=bridge::MAX_HOOK_HIGHLIGHT_LINES)
        .map(|line| format!("const line{line} = {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(bridge::hook_code_lines(&over_budget, 1).is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn disabled_matching_deduplication_and_config_snapshot() {
    let (_dir, service) = service_with_selector(Arc::new(|project, _| project == "project-a"));
    let definition = hook("printf 'done\\n'");
    service.save(definition.clone()).unwrap();
    let mut event = HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({}));
    event.project = Some("project-a".into());
    let disabled = service.emit(event.clone()).await.unwrap();
    assert!(disabled.runs.is_empty());
    assert_eq!(disabled.skipped.len(), 1);
    service.enable(true).unwrap();
    let receipt = service.emit(event.clone()).await.unwrap();
    assert_eq!(receipt.event_id, event.event_id);
    let run = finished(&service, &receipt.runs[0]).await;
    assert_eq!(run.config_revision, 1);
    assert_eq!(run.project.as_deref(), Some("project-a"));
    assert_eq!(run.conclusion, Some(HookConclusion::Success));
    assert!(service.emit(event).await.unwrap().runs.is_empty());
    let mut filtered = definition;
    filtered.project = "other-project".into();
    service.save(filtered).unwrap();
    let mut event = HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({}));
    event.project = Some("project-a".into());
    assert!(service.emit(event).await.unwrap().runs.is_empty());
    service.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn project_opt_in_filters_dispatch_and_rechecks_queued_runs() {
    let selection = Arc::new(Mutex::new(Vec::<String>::new()));
    let lookup = selection.clone();
    let (dir, service) = service_with_selector(Arc::new(move |project, hook| {
        project == "project-a" && lookup.lock().unwrap().iter().any(|id| id == hook)
    }));
    let selected = hook("printf selected > should-not-run.txt");
    let unselected = hook("printf unselected > unselected.txt");
    service.save(selected.clone()).unwrap();
    service.save(unselected.clone()).unwrap();
    service.enable(true).unwrap();
    let event = |project: &str| {
        let mut event = HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({}));
        event.project = Some(project.into());
        event.cwd = Some(dir.path().to_owned());
        event
    };
    let none = service.emit(event("project-a")).await.unwrap();
    assert!(none.runs.is_empty());
    assert!(
        none.skipped
            .iter()
            .all(|reason| reason.contains("not selected by project"))
    );
    selection.lock().unwrap().push(selected.id.clone());
    assert!(
        service
            .emit(event("project-b"))
            .await
            .unwrap()
            .runs
            .is_empty()
    );
    assert!(
        service
            .emit(HookTrigger::new(
                PR_REVIEW_APPROVED,
                "test",
                serde_json::json!({})
            ))
            .await
            .unwrap_err()
            .contains("require a project ID")
    );

    let lock = service.project_lock("project-a");
    let guard = lock.lock().await;
    let receipt = service.emit(event("project-a")).await.unwrap();
    assert_eq!(receipt.runs.len(), 1);
    assert_eq!(receipt.skipped.len(), 1);
    selection.lock().unwrap().clear();
    drop(guard);
    let run = finished(&service, &receipt.runs[0]).await;
    assert_eq!(run.conclusion, Some(HookConclusion::Skipped));
    assert!(run.detail.contains("no longer selected"));
    assert!(
        service
            .events_after(&run.id, 0)
            .await
            .unwrap()
            .iter()
            .all(|event| event.kind != "execution.started")
    );
    assert!(!dir.path().join("should-not-run.txt").exists());
    assert!(!dir.path().join("unselected.txt").exists());

    let mut explicit = hook("printf explicit");
    explicit.project = "project-b".into();
    let id = service.test(explicit, None).await.unwrap();
    assert_eq!(
        finished(&service, &id).await.conclusion,
        Some(HookConclusion::Success)
    );
    service
        .register_event(EventDefinition {
            name: "application.ready.v1".into(),
            category: "application".into(),
            project_scoped: false,
            sample: serde_json::json!({}),
        })
        .unwrap();
    let mut global = hook("printf global");
    global.event = "application.ready.v1".into();
    service.save(global).unwrap();
    let receipt = service
        .emit(HookTrigger::new(
            "application.ready.v1",
            "app",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(receipt.runs.len(), 1);
    assert_eq!(
        finished(&service, &receipt.runs[0]).await.conclusion,
        Some(HookConclusion::Success)
    );
    service.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_bypasses_switches_reports_stderr_and_preserves_host_lifecycle() {
    let (_dir, service) = service();
    let mut definition =
        hook("printf 'hook.finished\\n'; printf 'specific failure: missing file\\n' >&2; exit 7");
    definition.enabled = false;
    let id = service.test(definition, None).await.unwrap();
    let run = finished(&service, &id).await;
    assert!(run.test);
    assert_eq!(run.conclusion, Some(HookConclusion::Failure));
    assert_eq!(run.exit_code, Some(7));
    assert!(run.detail.contains("specific failure: missing file"));
    let events = service.events_after(&id, 0).await.unwrap();
    let lifecycle: Vec<_> = events
        .iter()
        .filter(|e| e.kind != "output")
        .map(|e| e.kind.as_str())
        .collect();
    assert_eq!(
        lifecycle,
        [
            "hook.triggered",
            "hook.queued",
            "execution.started",
            "execution.finished",
            "hook.finished"
        ]
    );
    assert!(events.windows(2).all(|pair| pair[0].seq + 1 == pair[1].seq));
    assert!(
        service
            .events_after(&id, events.last().unwrap().seq)
            .await
            .unwrap()
            .is_empty()
    );
    service.shutdown().await;
}

#[tokio::test]
async fn missing_interpreter_and_bad_working_directory_are_start_failures() {
    let (_dir, service) = service();
    let mut definition = hook("console.log('hello')");
    definition.language = HookLanguage::JavaScriptTypeScript;
    definition.interpreter = "/missing/usher-node-runtime".into();
    let id = service.test(definition.clone(), None).await.unwrap();
    let run = finished(&service, &id).await;
    assert_eq!(run.conclusion, Some(HookConclusion::StartFailed));
    assert!(run.detail.contains("/missing/usher-node-runtime"));
    assert_eq!(run.failure_kind, Some(HookFailureKind::InterpreterStart));
    assert!(!run.detail_text().source_text().contains("/missing/"));
    assert!(run.started_at_ms.is_none());
    let events = service.events_after(&id, 0).await.unwrap();
    assert!(!events.iter().any(|e| e.kind == "execution.started"));
    definition.id = new_id("hook");
    let id = service
        .test(definition, Some("/missing/usher-working-directory".into()))
        .await
        .unwrap();
    let run = finished(&service, &id).await;
    assert!(run.detail.contains("Working directory"));
    assert_eq!(run.failure_kind, Some(HookFailureKind::WorkingDirectory));
    service.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_cancel_and_duplicate_test_have_terminal_results() {
    let (_dir, service) = service();
    let mut definition = hook("sleep 30");
    definition.timeout_seconds = 1;
    let id = service.test(definition.clone(), None).await.unwrap();
    assert!(
        service
            .test(definition, None)
            .await
            .unwrap_err()
            .contains("already running")
    );
    assert_eq!(
        finished(&service, &id).await.conclusion,
        Some(HookConclusion::TimedOut)
    );
    let id = service.test(hook("sleep 30"), None).await.unwrap();
    service.cancel(&id);
    assert_eq!(
        finished(&service, &id).await.conclusion,
        Some(HookConclusion::Cancelled)
    );
    service.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn output_is_bounded_even_without_newlines_and_context_is_data() {
    let (dir, service) = service();
    let id = service.test(hook("printf '%s' \"$USHER_HOOK_TEST\"; cat \"$USHER_HOOK_CONTEXT\"; head -c 2500000 /dev/zero"), Some(dir.path().into())).await.unwrap();
    let run = finished(&service, &id).await;
    assert_eq!(run.conclusion, Some(HookConclusion::Success));
    assert!(run.log.len() <= MAX_TAIL_BYTES);
    let mut events = Vec::new();
    loop {
        let page = service
            .events_after(&id, events.last().map_or(0, |e: &HookEvent| e.seq))
            .await
            .unwrap();
        if page.is_empty() {
            break;
        }
        events.extend(page);
    }
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == "output.truncated")
            .count(),
        1
    );
    assert_eq!(events.last().unwrap().kind, "hook.finished");
    assert!(events.iter().any(|e| {
        e.kind == "output"
            && e.data["text"]
                .as_str()
                .unwrap_or_default()
                .contains("settings.test")
    }));
    service.shutdown().await;
}

#[tokio::test]
async fn external_node_executes_javascript_typescript_and_reports_errors() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("External Node.js is not installed; Node integration cases skipped.");
        return;
    }
    let (_dir, service) = service();
    for (language, script, expected, detail) in [
        (
            HookLanguage::JavaScriptTypeScript,
            "console.log('node-ok')",
            HookConclusion::Success,
            "",
        ),
        (
            HookLanguage::JavaScriptTypeScript,
            "const n: number = 42; console.log(n)",
            HookConclusion::Success,
            "",
        ),
        (
            HookLanguage::JavaScriptTypeScript,
            "const = broken;",
            HookConclusion::Failure,
            "SyntaxError",
        ),
        (
            HookLanguage::JavaScriptTypeScript,
            "throw new Error('specific-node-failure')",
            HookConclusion::Failure,
            "specific-node-failure",
        ),
        (
            HookLanguage::JavaScriptTypeScript,
            "enum Value { A }; console.log(Value)",
            HookConclusion::Failure,
            "TypeScript",
        ),
    ] {
        let mut definition = hook(script);
        definition.language = language;
        let id = service.test(definition, None).await.unwrap();
        let run = finished(&service, &id).await;
        assert_eq!(run.conclusion, Some(expected), "{}", run.detail);
        assert!(run.detail.contains(detail), "{}", run.detail);
    }
    service.shutdown().await;
}

#[test]
fn persistence_restores_rules_and_interrupts_incomplete_runs_without_replay() {
    let (dir, service) = service();
    let definition = hook("echo safe");
    service.save(definition.clone()).unwrap();
    let event = HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({}));
    let run = HookRun::queued(&definition, &event);
    service.state.dispatch(state::Action::Run(run)).unwrap();
    service.state.flush().unwrap();
    service.state.shutdown();
    let restored =
        HookService::new(dir.path().join("hooks.json"), dir.path().join("runs")).unwrap();
    restored.state.hydrate().unwrap();
    assert_eq!(restored.state.snapshot().state().hooks.len(), 1);
    assert_eq!(
        restored.state.snapshot().state().runs[0].conclusion,
        Some(HookConclusion::Interrupted)
    );
    assert!(restored.active.lock().unwrap().is_empty());
    restored.state.shutdown();
}

#[test]
fn corrupt_configuration_is_preserved_and_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, "not json").unwrap();
    let service = HookService::new(path.clone(), dir.path().join("runs")).unwrap();
    assert!(service.state.hydrate().is_err());
    assert!(!service.state.snapshot().state().ready);
    assert!(service.enable(true).is_err());
    service.state.shutdown();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "not json");
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_cancels_running_and_queued_work_and_prevents_descendant_effects() {
    let (dir, service) = service();
    let id = service
        .test(
            hook("(sleep 2; echo escaped > escaped.txt) & wait"),
            Some(dir.path().into()),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if service
                .state
                .snapshot()
                .state()
                .runs
                .iter()
                .any(|r| r.id == id && r.started_at_ms.is_some())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let queued = service
        .test(
            hook("echo should-not-run > queued.txt"),
            Some(dir.path().into()),
        )
        .await
        .unwrap();
    service.shutdown().await;
    assert_eq!(
        finished(&service, &id).await.conclusion,
        Some(HookConclusion::Cancelled)
    );
    assert_eq!(
        finished(&service, &queued).await.conclusion,
        Some(HookConclusion::Cancelled)
    );
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert!(!dir.path().join("escaped.txt").exists());
    assert!(!dir.path().join("queued.txt").exists());
    assert!(service.test(hook("echo no"), None).await.is_err());
    assert!(service.save(hook("echo no")).is_err());
    assert!(service.enable(true).is_err());
    assert!(service.delete("hook-unused".into()).is_err());
    assert!(
        service
            .register_event(EventDefinition {
                name: "another.module.completed.v1".into(),
                category: "another_module".into(),
                project_scoped: false,
                sample: serde_json::json!({}),
            })
            .is_err()
    );
}

#[test]
fn history_picker_rows_do_not_overlap_and_select_their_own_run() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{ComponentHandle, ModelRc, VecModel, platform::PointerEventButton};
    use usher_ui_settings::{GitHubSettingsPage, HookChoice, HookSettingsState, HookSummary};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(1800.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|e| e.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    let state = page.global::<HookSettingsState>();
    state.set_ready(true);
    state.set_history(ModelRc::new(VecModel::from(
        (0..3)
            .map(|index| HookChoice {
                value: format!("run-{index}").into(),
                label: format!("PR Review success 11:14:2{index}").into(),
            })
            .collect::<Vec<_>>(),
    )));
    let chosen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = chosen.clone();
    state.on_choose(move |kind, value| {
        observed
            .borrow_mut()
            .push((kind.to_string(), value.to_string()))
    });
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        let label = crate::i18n::tr("hooks.history").into_owned();
        let trigger = ElementHandle::find_by_accessible_label(&page, &label)
            .find(|element| element.accessible_expandable().is_some())
            .expect("history trigger");
        for _ in 0..2 {
            trigger.mock_single_click(PointerEventButton::Left);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
            assert_eq!(trigger.accessible_expanded(), Some(true));
            trigger.mock_single_click(PointerEventButton::Left);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
            assert_eq!(trigger.accessible_expanded(), Some(false));
        }
        assert!(chosen.borrow().is_empty(), "toggle must not select a run");
    }
    crate::i18n::apply_preference("en").unwrap();
    for selected in 0..3 {
        ElementHandle::find_by_accessible_label(&page, "Execution history")
            .next()
            .unwrap()
            .mock_single_click(PointerEventButton::Left);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        let rows = (0..3)
            .map(|index| {
                ElementHandle::find_by_accessible_label(
                    &page,
                    &format!("PR Review success 11:14:2{index}"),
                )
                .next()
                .expect("history option")
            })
            .collect::<Vec<_>>();
        for pair in rows.windows(2) {
            assert!(
                (pair[1].absolute_position().y - pair[0].absolute_position().y - 32.0).abs() < 1.0
            );
            assert!(
                pair[0].absolute_position().y + pair[0].size().height
                    <= pair[1].absolute_position().y + 0.5
            );
        }
        rows[selected].mock_single_click(PointerEventButton::Left);
        assert_eq!(
            chosen.borrow().last(),
            Some(&("history".to_string(), format!("run-{selected}")))
        );
    }
    state.set_history(ModelRc::new(VecModel::from(vec![HookChoice {
        value: "only-run".into(),
        label: "Only history record".into(),
    }])));
    state.set_summaries(ModelRc::new(VecModel::from(
        (0..10)
            .map(|index| HookSummary {
                id: format!("hook-{index}").into(),
                name: format!("Hook {index}").into(),
                description: "Review submitted".into(),
                enabled: true,
                category: "Pull request review".into(),
                category_index: 0,
                category_start: index == 0,
                category_end: index == 9,
                category_size: 10,
            })
            .collect::<Vec<_>>(),
    )));
    page.set_viewport_height(600.0);
    page.window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(600.0, 300.0),
            delta_x: 0.0,
            delta_y: -10000.0,
        });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
    let trigger = ElementHandle::find_by_accessible_label(&page, "Execution history")
        .next()
        .unwrap();
    trigger.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let menu = ElementHandle::find_by_element_id(&page, "GitHubActionMenu::menu-surface")
        .next()
        .expect("history popup surface");
    let row = ElementHandle::find_by_accessible_label(&page, "Only history record")
        .next()
        .unwrap();
    assert!(
        (menu.size().height - 40.0).abs() < 0.5,
        "single option must have only 4px padding per edge"
    );
    assert!((row.absolute_position().y - menu.absolute_position().y - 4.0).abs() < 0.5);
    assert!(
        menu.absolute_position().y + menu.size().height <= trigger.absolute_position().y - 3.5,
        "bottom-edge menu must open above its trigger: menu={:?}/{:?}, trigger={:?}/{:?}",
        menu.absolute_position(),
        menu.size(),
        trigger.absolute_position(),
        trigger.size()
    );
    row.mock_single_click(PointerEventButton::Left);
    page.window()
        .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(100.0, 300.0),
            delta_x: 0.0,
            delta_y: 10000.0,
        });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
    state.set_summaries(ModelRc::new(VecModel::from(vec![HookSummary {
        id: "hook-a".into(),
        name: "Independent Hook option".into(),
        description: "Review submitted".into(),
        enabled: true,
        category: "Pull request review".into(),
        category_index: 0,
        category_start: true,
        category_end: true,
        category_size: 1,
    }])));
    ElementHandle::find_by_accessible_label(&page, "Edit Hook: Independent Hook option")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert!(
        ElementHandle::find_by_accessible_label(&page, "Only history record")
            .next()
            .is_none()
    );
    assert_eq!(
        chosen.borrow().last(),
        Some(&("hook".to_string(), "hook-a".to_string()))
    );
    crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
}

#[test]
fn dialog_cancel_discards_edits_and_validation_keeps_the_editor_open() {
    use slint::ComponentHandle;
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_dir, service) = service();
    let saved = hook("printf saved");
    service.save(saved.clone()).unwrap();
    let window = crate::MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    bridge::bind_settings_with_service(&window, &page, service.clone());
    let g = page.global::<HookSettingsState>();
    assert_eq!(g.get_dialog_mode(), "");
    g.invoke_choose("hook".into(), saved.id.clone().into());
    assert_eq!(g.get_dialog_mode(), "edit");
    assert!(g.get_draft_script_loading());
    assert!(g.get_draft_script().is_empty());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    assert_eq!(g.get_draft_script(), saved.script);
    assert!(!g.get_draft_script_loading());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert!(g.get_code_highlight_ready());
    g.set_draft_name("Unsaved rename".into());
    g.set_draft_script("printf unsaved".into());
    g.invoke_draft_edited();
    g.invoke_select_language(1);
    g.invoke_cancel();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(g.get_dialog_mode(), "");
    assert!(g.get_draft_script().is_empty());
    assert!(g.get_shell_draft().script.is_empty());
    assert!(g.get_node_draft().script.is_empty());
    assert_eq!(g.get_code_highlight(), slint::StyledText::default());
    assert!(!g.get_code_highlight_ready());
    g.invoke_choose("hook".into(), saved.id.clone().into());
    g.invoke_cancel();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(125));
    assert!(
        g.get_draft_script().is_empty(),
        "cancel must stop delayed source hydration"
    );
    assert_eq!(g.get_code_highlight(), slint::StyledText::default());
    g.invoke_choose("hook".into(), saved.id.clone().into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    assert_eq!(g.get_draft_name(), saved.name);
    assert_eq!(g.get_draft_script(), saved.script);
    g.invoke_cancel();
    g.invoke_create();
    assert_eq!(g.get_dialog_mode(), "add");
    assert!(g.get_draft_script().is_empty());
    g.invoke_save();
    assert!(g.get_result_error());
    assert!(g.get_result().contains("empty"));
    assert_eq!(g.get_dialog_mode(), "add");
    g.invoke_test();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(g.get_test_status(), "failure");
    let button = i_slint_backend_testing::ElementHandle::find_by_element_id(
        &page,
        "GitHubHookTestFeedback::test-button",
    )
    .next()
    .unwrap();
    let error = i_slint_backend_testing::ElementHandle::find_by_element_id(
        &page,
        "GitHubHookTestFeedback::error-text",
    )
    .next()
    .unwrap();
    assert!(
        error.absolute_position().y >= button.absolute_position().y + button.size().height,
        "error y={} button bottom={}",
        error.absolute_position().y,
        button.absolute_position().y + button.size().height,
    );
    assert!(error.absolute_position().y + error.size().height <= page.get_window_height());
    g.set_busy(true);
    g.invoke_cancel();
    assert_eq!(g.get_dialog_mode(), "add");
    g.set_busy(false);
    g.invoke_cancel();
    g.invoke_request_delete(saved.id.clone().into());
    assert_eq!(g.get_dialog_mode(), "delete");
    g.invoke_cancel();
    assert_eq!(service.state.snapshot().state().hooks.len(), 1);
    assert_eq!(
        service.state.snapshot().state().hooks[0].script,
        saved.script
    );
    g.invoke_choose("hook".into(), saved.id.clone().into());
    let mut run = HookRun::queued(
        &saved,
        &HookTrigger::new(&saved.event, "test", serde_json::json!({})),
    );
    run.test = true;
    run.conclusion = Some(HookConclusion::Success);
    run.detail = "Execution completed successfully.".into();
    g.set_test_run(run.id.clone().into());
    let snapshot = HookState {
        ready: true,
        hooks: vec![saved.clone()].into(),
        runs: vec![run],
        ..Default::default()
    };
    bridge::apply_settings(&page, &snapshot);
    assert_eq!(g.get_test_status(), "success");
    assert!(
        g.get_result().is_empty(),
        "success is a compact status, not a detail banner"
    );
    g.set_draft_script("printf changed".into());
    g.invoke_draft_edited();
    bridge::apply_settings(&page, &snapshot);
    assert!(
        g.get_test_status().is_empty(),
        "old run updates must not validate an edited script"
    );
    assert!(g.get_test_run().is_empty());
    crate::app_runtime::shared_runtime().block_on(service.shutdown());
}

#[test]
fn dialog_switching_hooks_rebuilds_the_editor_session_and_refreshes_highlight() {
    use i_slint_backend_testing::ElementHandle;
    use slint::ComponentHandle;
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let (_dir, service) = service();
    let mut first = hook("import crypto from 'node:crypto'\nconsole.log('first')");
    first.name = "First".into();
    let mut second = hook("console.log(222)");
    second.name = "Second".into();
    service.save(first.clone()).unwrap();
    service.save(second.clone()).unwrap();
    let window = crate::MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    bridge::bind_settings_with_service(&window, &page, service);
    let g = page.global::<HookSettingsState>();

    g.invoke_choose("hook".into(), first.id.clone().into());
    assert_eq!(g.get_dialog_mode(), "edit");
    assert!(g.get_draft_script_loading());
    assert!(g.get_draft_script().is_empty());
    assert!(!g.get_code_highlight_ready());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    assert_eq!(g.get_draft_script(), first.script);
    assert!(!g.get_draft_script_loading());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(
        i_slint_core::styled_text::get_raw_text(&g.get_code_highlight()),
        first.script
    );
    let highlight = ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
        .next()
        .expect("one mounted rich-text node");
    assert!(g.get_code_highlight_ready());
    g.invoke_cancel();
    assert_eq!(g.get_code_highlight(), slint::StyledText::default());
    assert!(!g.get_code_highlight_ready());
    assert!(
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
            .next()
            .is_none()
    );
    assert!(!highlight.is_valid());

    g.invoke_choose("hook".into(), second.id.clone().into());
    assert!(g.get_draft_script_loading());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(
        i_slint_core::styled_text::get_raw_text(&g.get_code_highlight()),
        second.script
    );
    let second_highlight =
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
            .next()
            .expect("second editor session rich-text node");
    assert!(second_highlight.is_valid());
    g.invoke_cancel();
    assert!(
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
            .next()
            .is_none()
    );
    assert!(!second_highlight.is_valid());

    g.invoke_choose("hook".into(), first.id.into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(25));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    assert_eq!(
        i_slint_core::styled_text::get_raw_text(&g.get_code_highlight()),
        first.script
    );
    let third_highlight =
        ElementHandle::find_by_element_id(&page, "HookCodeEditor::script-highlight")
            .next()
            .expect("third editor session rich-text node");
    assert!(third_highlight.is_valid());
}

#[test]
fn hook_tab_precedes_diagnostics_and_new_button_stays_inside_pane() {
    use i_slint_backend_testing::ElementHandle;
    use slint::ComponentHandle;
    use slint::platform::PointerEventButton;
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let tab = ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|element| element.accessible_checked().is_some())
        .expect("Hooks tab");
    let diagnostics = ElementHandle::find_by_accessible_label(&page, "Diagnostics")
        .find(|element| element.accessible_checked().is_some())
        .expect("Diagnostics tab");
    assert!(tab.absolute_position().y < diagnostics.absolute_position().y);
    tab.mock_single_click(PointerEventButton::Left);
    page.global::<HookSettingsState>().set_ready(true);
    let button = ElementHandle::find_by_accessible_label(&page, "New Hook")
        .next()
        .expect("New Hook button");
    assert!(button.absolute_position().x + button.size().width <= 1200.0);
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = calls.clone();
    page.global::<HookSettingsState>()
        .on_create(move || observed.set(observed.get() + 1));
    button.mock_single_click(PointerEventButton::Left);
    assert_eq!(calls.get(), 1);
}

#[test]
fn runtime_radios_restore_only_the_selected_runtime_draft() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{ComponentHandle, platform::PointerEventButton};
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(1800.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|e| e.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    bridge::bind_language_selector(&page);
    bridge::set_draft(&page, &hook("echo shell-draft"));
    let g = page.global::<HookSettingsState>();
    g.set_dialog_mode("edit".into());
    g.set_ready(true);
    g.set_draft_interpreter("/bin/sh".into());
    bridge::update_hook_highlight(&page);
    assert!(g.get_code_highlight_ready());

    let select = |name: &str| {
        let tab = ElementHandle::find_by_accessible_label(&page, name)
            .find(|e| e.accessible_checked().is_some())
            .unwrap();
        tab.mock_single_click(PointerEventButton::Left);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    };
    select("JavaScript/TypeScript");
    assert!(g.get_draft_script().is_empty());
    assert!(g.get_draft_interpreter().is_empty());
    assert!(!g.get_code_highlight_ready());
    assert_eq!(g.get_code_highlight(), slint::StyledText::default());
    g.set_draft_script("const n: number = 42".into());
    g.set_draft_interpreter("/opt/node".into());
    bridge::update_hook_highlight(&page);

    for width in [1200.0, 720.0, 1200.0] {
        page.set_viewport_width(width);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
        for (index, name) in [(0, "Shell"), (1, "JavaScript/TypeScript")] {
            g.set_test_status("success".into());
            g.set_test_run("old-run".into());
            select(name);
            assert_eq!(g.get_draft_language(), index);
            assert!(
                !g.get_code_highlight_ready(),
                "old runtime highlight must disappear immediately"
            );
            assert_eq!(g.get_code_highlight(), slint::StyledText::default());
            let (script, interpreter) = match index {
                0 => ("echo shell-draft", "/bin/sh"),
                _ => ("const n: number = 42", "/opt/node"),
            };
            assert_eq!(g.get_draft_interpreter(), interpreter);
            assert_eq!(g.get_draft_script(), script);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
            assert!(g.get_code_highlight_ready());
            assert_eq!(
                i_slint_core::styled_text::get_raw_text(&g.get_code_highlight()),
                script
            );
            assert!(g.get_test_status().is_empty());
            assert!(g.get_test_run().is_empty());
            let input = ElementHandle::find_by_accessible_label(&page, "Interpreter executable")
                .find(|e| e.accessible_value().is_some())
                .unwrap();
            assert_eq!(input.accessible_value().as_deref(), Some(interpreter));
            for (tab_name, selected) in
                [("Shell", index == 0), ("JavaScript/TypeScript", index == 1)]
            {
                let tab = ElementHandle::find_by_accessible_label(&page, tab_name)
                    .find(|e| e.accessible_checked().is_some())
                    .unwrap();
                assert_eq!(tab.accessible_checked(), Some(selected));
            }
        }
    }
    g.set_busy(true);
    select("Shell");
    assert_eq!(g.get_draft_language(), 1);
    g.set_busy(false);
    bridge::set_draft(&page, &hook("echo another-hook"));
    select("JavaScript/TypeScript");
    assert!(g.get_draft_script().is_empty());
    assert!(g.get_draft_interpreter().is_empty());
    select("Shell");
    assert_eq!(g.get_draft_script(), "echo another-hook");
}

#[test]
fn global_enable_progress_does_not_dim_or_move_execution_history() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{ComponentHandle, ModelRc, VecModel, platform::PointerEventButton};
    use usher_ui_settings::{GitHubSettingsPage, HookChoice, HookSettingsState};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|element| element.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    let state = page.global::<HookSettingsState>();
    state.set_ready(true);
    state.set_result("Hook saved".into());
    state.set_history(ModelRc::new(VecModel::from(vec![HookChoice {
        value: "run-1".into(),
        label: "Run 1".into(),
    }])));
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let history = ElementHandle::find_by_accessible_label(&page, "Execution history")
        .find(|element| element.accessible_enabled().is_some())
        .unwrap();
    let before = history.absolute_position();
    assert_eq!(history.accessible_enabled(), Some(true));

    state.set_enable_busy(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let history = ElementHandle::find_by_accessible_label(&page, "Execution history")
        .find(|element| element.accessible_enabled().is_some())
        .unwrap();
    assert_eq!(history.accessible_enabled(), Some(true));
    assert_eq!(history.absolute_position(), before);
    assert_eq!(state.get_result(), "Hook saved");
}

#[test]
fn hook_rows_and_event_select_share_module_categories() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{
        ComponentHandle, Model,
        platform::{Key, PointerEventButton, WindowEvent},
    };
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(1100.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|element| element.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);

    let mut ai_hook = hook("echo ai");
    ai_hook.id = "ai-hook".into();
    ai_hook.name = "AI Hook".into();
    ai_hook.event = "ai.review.completed.v1".into();
    let mut pr_hook_one = hook("echo pr-one");
    pr_hook_one.id = "pr-hook-one".into();
    pr_hook_one.name = "PR Hook One".into();
    let mut pr_hook_two = hook("echo pr-two");
    pr_hook_two.id = "pr-hook-two".into();
    pr_hook_two.name = "PR Hook Two".into();
    let state = HookState {
        ready: true,
        enabled: true,
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
        hooks: vec![ai_hook, pr_hook_one, pr_hook_two].into(),
        ..Default::default()
    };
    bridge::apply_settings(&page, &state);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let global = page.global::<HookSettingsState>();
    let summaries = global.get_summaries();
    assert_eq!(summaries.row_count(), 3);
    assert_eq!(summaries.row_data(0).unwrap().id, "pr-hook-one");
    assert_eq!(summaries.row_data(1).unwrap().id, "pr-hook-two");
    assert_eq!(summaries.row_data(2).unwrap().id, "ai-hook");
    assert!(summaries.row_data(0).unwrap().category_start);
    assert!(summaries.row_data(1).unwrap().category_end);
    assert_eq!(summaries.row_data(0).unwrap().category_size, 2);
    assert_eq!(summaries.row_data(2).unwrap().category_index, 1);

    let first = ElementHandle::find_by_accessible_label(&page, "Edit Hook: PR Hook One")
        .next()
        .unwrap();
    let second = ElementHandle::find_by_accessible_label(&page, "Edit Hook: PR Hook Two")
        .next()
        .unwrap();
    let third = ElementHandle::find_by_accessible_label(&page, "Edit Hook: AI Hook")
        .next()
        .unwrap();
    assert!((second.absolute_position().y - first.absolute_position().y - 72.0).abs() < 0.5);
    assert!((third.absolute_position().y - second.absolute_position().y - 120.0).abs() < 0.5);
    assert!(
        ElementHandle::find_by_accessible_label(&page, "Pull request review. 2")
            .next()
            .is_some()
    );
    assert!(
        ElementHandle::find_by_accessible_label(&page, "AI code review. 1")
            .next()
            .is_some()
    );

    bridge::set_draft(&page, &HookDefinition::default());
    global.set_dialog_mode("add".into());
    let selected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = selected.clone();
    global.on_choose(move |kind, value| {
        observed
            .borrow_mut()
            .push((kind.to_string(), value.to_string()));
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Pull request approved")
        .find(|element| element.accessible_expandable().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let first_heading = ElementHandle::find_by_accessible_label(&page, "Pull request review")
        .next()
        .expect("first event group heading");
    let first_option = ElementHandle::find_by_accessible_label(&page, "Pull request approved")
        .find(|element| element.accessible_checked().is_some())
        .expect("first event option");
    let second_option = ElementHandle::find_by_accessible_label(&page, "Pull request merged")
        .next()
        .expect("second event option");
    let second_heading = ElementHandle::find_by_accessible_label(&page, "AI code review")
        .next()
        .expect("second event group heading");
    let third_option = ElementHandle::find_by_accessible_label(&page, "AI review completed")
        .next()
        .expect("third event option");
    assert!(
        (first_option.absolute_position().y - first_heading.absolute_position().y - 28.0).abs()
            < 0.5
    );
    assert!(
        (second_option.absolute_position().y - first_option.absolute_position().y - 32.0).abs()
            < 0.5
    );
    assert!(
        (second_heading.absolute_position().y - second_option.absolute_position().y - 32.0).abs()
            < 0.5
    );
    assert!(
        (third_option.absolute_position().y - second_heading.absolute_position().y - 28.0).abs()
            < 0.5
    );

    for key in [Key::DownArrow, Key::DownArrow, Key::Return] {
        page.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        page.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    }
    assert_eq!(
        selected.borrow().as_slice(),
        &[("event".to_string(), "ai.review.completed.v1".to_string())]
    );

    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        bridge::apply_settings(&page, &state);
        let label = crate::i18n::tr("hooks.event_pr_approved").into_owned();
        let trigger = ElementHandle::find_by_accessible_label(&page, &label)
            .find(|element| element.accessible_expandable().is_some())
            .expect("event trigger");
        for _ in 0..2 {
            trigger.mock_single_click(PointerEventButton::Left);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
            assert_eq!(trigger.accessible_expanded(), Some(true));
            trigger.mock_single_click(PointerEventButton::Left);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
            assert_eq!(trigger.accessible_expanded(), Some(false));
        }
        assert_eq!(global.get_draft_event(), PR_REVIEW_APPROVED);
        assert_eq!(
            selected.borrow().len(),
            1,
            "toggle must not commit a choice"
        );
    }

    crate::i18n::apply_preference("zh-CN").unwrap();
    bridge::apply_settings(&page, &state);
    let events = global.get_events();
    let summaries = global.get_summaries();
    assert_eq!(events.row_data(0).unwrap().label, "Pull Request 审核通过后");
    assert_eq!(events.row_data(1).unwrap().label, "Pull Request 合并完成后");
    assert_eq!(events.row_data(0).unwrap().group_label, "Pull Request 评审");
    assert_eq!(events.row_data(2).unwrap().label, "AI 评审完成后");
    assert_eq!(summaries.row_data(0).unwrap().category, "Pull Request 评审");
    assert_eq!(
        summaries.row_data(0).unwrap().description,
        "Pull Request 审核通过后 · Shell"
    );
    assert_eq!(global.get_draft_event_label(), "Pull Request 审核通过后");

    crate::i18n::apply_preference("en").unwrap();
    bridge::apply_settings(&page, &state);
    let events = global.get_events();
    let summaries = global.get_summaries();
    assert_eq!(events.row_data(0).unwrap().label, "Pull request approved");
    assert_eq!(events.row_data(1).unwrap().label, "Pull request merged");
    assert_eq!(
        events.row_data(0).unwrap().group_label,
        "Pull request review"
    );
    assert_eq!(events.row_data(2).unwrap().label, "AI review completed");
    assert_eq!(
        summaries.row_data(0).unwrap().category,
        "Pull request review"
    );
    assert_eq!(
        summaries.row_data(0).unwrap().description,
        "Pull request approved · Shell"
    );
    assert_eq!(global.get_draft_event_label(), "Pull request approved");
}

#[test]
fn row_switches_reflect_global_state_without_losing_configured_values() {
    use i_slint_backend_testing::ElementHandle;
    use slint::{ComponentHandle, Model, ModelRc, VecModel, platform::PointerEventButton};
    use usher_ui_settings::{GitHubSettingsPage, HookSettingsState, HookSummary};

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let page = GitHubSettingsPage::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    page.set_viewport_width(1200.0);
    page.set_viewport_height(900.0);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    ElementHandle::find_by_accessible_label(&page, "Hooks")
        .find(|element| element.accessible_checked().is_some())
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    let state = page.global::<HookSettingsState>();
    state.set_ready(true);
    state.set_summaries(ModelRc::new(VecModel::from(vec![
        HookSummary {
            id: "enabled-hook".into(),
            name: "Enabled Hook".into(),
            description: "Review submitted".into(),
            enabled: true,
            category: "Pull request review".into(),
            category_index: 0,
            category_start: true,
            category_end: false,
            category_size: 2,
        },
        HookSummary {
            id: "disabled-hook".into(),
            name: "Disabled Hook".into(),
            description: "Review submitted".into(),
            enabled: false,
            category: "Pull request review".into(),
            category_index: 0,
            category_start: false,
            category_end: true,
            category_size: 2,
        },
    ])));

    state.set_enabled(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let enabled = ElementHandle::find_by_accessible_label(&page, "Enable this Hook: Enabled Hook")
        .next()
        .expect("enabled Hook row Switch");
    let disabled =
        ElementHandle::find_by_accessible_label(&page, "Enable this Hook: Disabled Hook")
            .next()
            .expect("disabled Hook row Switch");
    assert_eq!(enabled.accessible_checked(), Some(true));
    assert_eq!(disabled.accessible_checked(), Some(false));
    assert_eq!(enabled.accessible_enabled(), Some(true));

    let toggles = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = toggles.clone();
    state.on_set_hook_enabled(move |id, value| {
        observed.borrow_mut().push((id.to_string(), value));
    });
    enabled.mock_single_click(PointerEventButton::Left);
    assert_eq!(
        toggles.borrow().as_slice(),
        &[("enabled-hook".to_string(), false)]
    );

    state.set_row_enable_busy_id("enabled-hook".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(enabled.accessible_checked(), Some(true));
    assert_eq!(disabled.accessible_checked(), Some(false));
    assert_eq!(enabled.accessible_enabled(), Some(false));
    assert_eq!(disabled.accessible_enabled(), Some(false));
    disabled.mock_single_click(PointerEventButton::Left);
    assert_eq!(
        toggles.borrow().as_slice(),
        &[("enabled-hook".to_string(), false)]
    );

    state.set_row_enable_busy_id("".into());
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(enabled.accessible_enabled(), Some(true));
    assert_eq!(disabled.accessible_enabled(), Some(true));

    state.set_enabled(false);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(enabled.accessible_checked(), Some(false));
    assert_eq!(disabled.accessible_checked(), Some(false));
    assert_eq!(enabled.accessible_enabled(), Some(false));

    state.set_enabled(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(enabled.accessible_checked(), Some(true));
    assert_eq!(disabled.accessible_checked(), Some(false));
    let summaries = state.get_summaries();
    assert!(summaries.row_data(0).unwrap().enabled);
    assert!(!summaries.row_data(1).unwrap().enabled);
}
