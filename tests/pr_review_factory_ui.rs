#![cfg(all(not(feature = "live-preview"), not(feature = "dev-shell")))]

use i_slint_backend_testing::ElementHandle;
use i_slint_core::{
    accessibility::AccessibleStringProperty,
    input::FocusReason,
    item_tree::{
        ItemRc, ItemTreeRc, ItemVisitorResult, ParentItemTraversalMode, TraversalOrder, visit_items,
    },
    items::FocusScope,
    window::WindowInner,
};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize, Model, ModelRc, VecModel};
use std::{cell::Cell, rc::Rc, time::Duration};
use usher_lib::{
    MainWindow, PrDiffBridge, PrReviewBridge, PrReviewUiRoot, Route, WorkflowPrDiffLine,
    WorkflowPrDiffRowMetric, WorkflowPrDiffSelectionHit, WorkflowPrDiffVirtualRow,
    WorkflowPrDiffVirtualWindow, install_pr_review_fixture, pr_review_fixture_view,
};

fn window() -> MainWindow {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("shell");
    window.window().set_size(LogicalSize::new(1280.0, 900.0));
    window
}

fn flush(window: &MainWindow) {
    WindowInner::from_pub(window.window()).ensure_tree_instantiated();
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
}

fn key(window: &MainWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn focus_label(window: &MainWindow, label: &str) -> ItemRc {
    let host = WindowInner::from_pub(window.window());
    let mut target = None;
    visit_items(
        &host.component(),
        TraversalOrder::FrontToBack,
        |tree, _, index, ()| {
            let item = ItemRc::new(tree.clone(), index);
            if item.downcast::<FocusScope>().is_some()
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .is_some_and(|text| text == label)
            {
                target = Some(item);
                return ItemVisitorResult::Abort;
            }
            ItemVisitorResult::Continue(())
        },
        (),
    );
    let target = target.unwrap_or_else(|| panic!("mounted focus control {label}"));
    host.set_focus_item(&target, true, FocusReason::Programmatic);
    target
}

fn focused(window: &MainWindow) -> ItemRc {
    WindowInner::from_pub(window.window())
        .focus_item
        .borrow()
        .upgrade()
        .expect("mounted host focus")
}

fn top_tree(mut item: ItemRc) -> ItemTreeRc {
    while let Some(parent) = item.parent_item(ParentItemTraversalMode::StopAtPopups) {
        item = parent;
    }
    item.item_tree().clone()
}

fn macos_merged_review() -> (MainWindow, PrReviewUiRoot) {
    let window = window();
    usher_lib::install_transient_input_bridge(&window).expect("real window pointer forwarding");
    window.set_platform_is_macos(true);
    window.set_show_navigation_drawer(false);
    usher_lib::install_floating_overlay_positioning(&window);
    install_pr_review_fixture(&window);
    let page = pr_review_fixture_view(&window);
    let review = page.global::<PrReviewBridge>();
    review.set_workflow_pr_review_visible(true);
    review.set_workflow_pr_review_loading(false);
    review.set_workflow_pr_merge_readiness_kind("merged".into());
    review.set_workflow_pr_merge_readiness_label("Merged".into());
    review.set_workflow_pr_merge_readiness_detail(
        "This Pull Request has already been merged.".into(),
    );
    window.set_current_route(Route::PrReview);
    flush(&window);
    (window, page)
}

fn click_readiness(window: &MainWindow) {
    ElementHandle::find_by_element_id(window, "GitHubMergeReadinessBadge::badge-touch")
        .next()
        .expect("real title-bar readiness trigger")
        .mock_single_click(PointerEventButton::Left);
    flush(window);
}

#[test]
fn real_factory_preserves_shell_identity_and_actual_renderer_across_route_cycles() {
    let window = window();
    let host = WindowInner::from_pub(window.window());
    let shell = host.component();
    install_pr_review_fixture(&window);
    let page = pr_review_fixture_view(&window);
    assert!(Rc::ptr_eq(
        &host.window_adapter(),
        &WindowInner::from_pub(page.window()).window_adapter(),
    ));
    assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));

    let standalone = PrReviewUiRoot::new().expect("standalone negative control");
    assert!(!Rc::ptr_eq(
        &host.window_adapter(),
        &WindowInner::from_pub(standalone.window()).window_adapter(),
    ));
    drop(standalone);

    window.set_show_navigation_drawer(false);
    for cycle in 0..20 {
        let width = if cycle % 2 == 0 { 1280.0 } else { 800.0 };
        window.window().dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width, 900.0),
        });
        window.set_current_route(Route::PrReview);
        flush(&window);
        // Window-root metadata is outside ElementHandle's descendant query;
        // this full-size child is the actual embedded factory's render surface.
        let surface = ElementHandle::find_by_element_type_name(&window, "PrReviewSurface")
            .next()
            .expect("the actual factory's full-window surface");
        assert_eq!(
            surface.absolute_position(),
            slint::LogicalPosition::new(0.0, 0.0)
        );
        assert_eq!(surface.size(), LogicalSize::new(width, 900.0));
        let document =
            ElementHandle::find_by_element_type_name(&window, "GitHubPullRequestReviewPage")
                .next()
                .expect("the complete review document is mounted on entry");
        assert_eq!(document.absolute_position().x, 0.0);
        assert_eq!(document.size().width, width);
        let stats = ElementHandle::find_by_element_id(
            &window,
            "GitHubPullRequestReviewPage::aggregate-diffstat",
        )
        .next()
        .expect("the real document's trailing totals");
        assert!((stats.absolute_position().x + stats.size().width - (width - 16.0)).abs() < 0.5);
        window.set_current_route(Route::Home);
        flush(&window);
        assert!(
            ElementHandle::find_by_element_type_name(&page, "GitHubPullRequestReviewPage")
                .next()
                .is_none(),
            "route exit destroys the heavy document in the retained factory"
        );
        install_pr_review_fixture(&window);
        let retained = pr_review_fixture_view(&window);
        assert!(Rc::ptr_eq(
            &host.window_adapter(),
            &WindowInner::from_pub(retained.window()).window_adapter(),
        ));
        assert!(ItemTreeRc::ptr_eq(&shell, &host.component()));
    }
}

#[test]
fn retained_factory_has_one_model_and_command_owner_without_cross_window_aliasing() {
    let first = window();
    let second = MainWindow::new().expect("second shell");
    install_pr_review_fixture(&first);
    install_pr_review_fixture(&second);
    let first_page = pr_review_fixture_view(&first);
    let second_page = pr_review_fixture_view(&second);
    let rows = Rc::new(VecModel::from(vec![WorkflowPrDiffLine {
        content: "first snapshot".into(),
        ..Default::default()
    }]));
    let identity: ModelRc<WorkflowPrDiffLine> = rows.clone().into();
    first_page
        .global::<PrReviewBridge>()
        .set_workflow_pr_review_lines(identity.clone());
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    first_page
        .global::<PrReviewBridge>()
        .on_workflow_pr_review_select_file(move |_| observed.set(observed.get() + 1));

    for _ in 0..3 {
        first.set_current_route(Route::PrReview);
        flush(&first);
        let retained = pr_review_fixture_view(&first);
        let state = retained.global::<PrReviewBridge>();
        assert_eq!(state.get_workflow_pr_review_lines(), identity);
        state.invoke_workflow_pr_review_select_file(0);
        first.set_current_route(Route::Home);
        flush(&first);
    }
    assert_eq!(calls.get(), 3);
    rows.set_row_data(
        0,
        WorkflowPrDiffLine {
            content: "latest snapshot".into(),
            ..Default::default()
        },
    );
    assert_eq!(
        first_page
            .global::<PrReviewBridge>()
            .get_workflow_pr_review_lines()
            .row_data(0)
            .unwrap()
            .content,
        "latest snapshot"
    );
    assert_eq!(
        second_page
            .global::<PrReviewBridge>()
            .get_workflow_pr_review_lines()
            .row_count(),
        0
    );
}

#[test]
fn mounted_review_tabs_refresh_source_and_packaged_locale_without_replacing_models() {
    let window = window();
    install_pr_review_fixture(&window);
    let page = pr_review_fixture_view(&window);
    let rows: ModelRc<WorkflowPrDiffLine> = Rc::new(VecModel::from(Vec::new())).into();
    page.global::<PrReviewBridge>()
        .set_workflow_pr_review_lines(rows.clone());
    window.set_current_route(Route::PrReview);
    flush(&window);

    for locale in ["en", "zh-CN", "en"] {
        slint::select_bundled_translation(locale).expect("packaged locale");
        i_slint_core::translations::mark_all_translations_dirty();
        flush(&window);
        let expected = usher_i18n::translate_for(locale, "pr_review.sections");
        assert!(
            ElementHandle::find_by_element_type_name(&page, "GitHubUnderlineNav")
                .any(|element| element.accessible_label().as_deref() == Some(expected.as_ref())),
            "mounted PR tabs must use the selected locale"
        );
        assert_eq!(
            page.global::<PrReviewBridge>()
                .get_workflow_pr_review_lines(),
            rows
        );
    }
}

#[test]
fn dropping_the_host_releases_the_retained_factory_and_its_models() {
    let window = window();
    install_pr_review_fixture(&window);
    let page = pr_review_fixture_view(&window);
    let weak = page.as_weak();
    window.set_current_route(Route::PrReview);
    flush(&window);
    window.set_current_route(Route::Home);
    flush(&window);
    drop(page);
    assert!(
        weak.upgrade().is_some(),
        "the host retains the small factory"
    );
    drop(window);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        weak.upgrade().is_none(),
        "the registry must not retain the UI"
    );
}

#[test]
fn an_old_fixed_tooltip_claim_cannot_hide_or_renew_a_new_action() {
    use usher_lib::{PrReviewTooltipKind, PrReviewTooltipState};
    use usher_ui_pr_review::{GitHubTooltipAlignment, GitHubTooltipPlacement};

    let window = window();
    install_pr_review_fixture(&window);
    window.set_current_route(Route::PrReview);
    flush(&window);
    let page = pr_review_fixture_view(&window);
    let state = page.global::<PrReviewTooltipState>();
    assert!(state.get_enabled());
    let old = state.invoke_show(
        PrReviewTooltipKind::CopyFilePath,
        100.0,
        100.0,
        32.0,
        32.0,
        GitHubTooltipPlacement::Top,
        GitHubTooltipAlignment::Center,
    );
    let current = state.invoke_show(
        PrReviewTooltipKind::ConflictsRefresh,
        300.0,
        150.0,
        32.0,
        32.0,
        GitHubTooltipPlacement::Top,
        GitHubTooltipAlignment::Center,
    );
    assert_ne!(old, current);
    state.invoke_hide(old);
    state.invoke_renew(old, 1.0, 1.0, 1.0, 1.0);
    assert_eq!(state.get_kind(), PrReviewTooltipKind::ConflictsRefresh);
    assert_eq!(state.get_anchor_x(), 300.0);
    state.invoke_hide(current);
    assert_eq!(state.get_kind(), PrReviewTooltipKind::None);
    window.set_current_route(Route::Home);
    flush(&window);
    assert!(!state.get_enabled());
}

#[test]
fn route_exit_revokes_metadata_tooltips_and_transients_without_clearing_models() {
    use usher_ui_pr_review::{
        GitHubTooltipAlignment, GitHubTooltipPlacement, TooltipOverlayState, TransientOverlayState,
    };

    let window = window();
    install_pr_review_fixture(&window);
    window.set_current_route(Route::PrReview);
    flush(&window);
    let page = pr_review_fixture_view(&window);
    let rows: ModelRc<WorkflowPrDiffLine> = Rc::new(VecModel::from(Vec::new())).into();
    page.global::<PrReviewBridge>()
        .set_workflow_pr_review_lines(rows.clone());
    let tooltip = page.global::<TooltipOverlayState>();
    tooltip.invoke_show(
        "route metadata fixture".into(),
        "src/review.rs".into(),
        100.0,
        100.0,
        32.0,
        32.0,
        GitHubTooltipPlacement::Top,
        GitHubTooltipAlignment::Center,
    );
    let claim = tooltip.get_token();
    let transient = page.global::<TransientOverlayState>();
    let revision = transient.get_dismiss_revision();
    let shell_revision = window
        .global::<usher_lib::TransientOverlayState>()
        .get_dismiss_revision();

    // Active geometry updates must not close the current metadata interaction.
    window.window().set_size(LogicalSize::new(1281.0, 900.0));
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(tooltip.get_open());
    assert_eq!(transient.get_dismiss_revision(), revision);

    window.set_current_route(Route::Home);
    // No 48ms lease-watcher grace period is allowed after route exit.
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(!tooltip.get_open());
    assert_eq!(tooltip.get_text(), "");
    assert_eq!(transient.get_dismiss_revision(), revision + 1);
    assert_eq!(
        window
            .global::<usher_lib::TransientOverlayState>()
            .get_dismiss_revision(),
        shell_revision,
        "route teardown does not dismiss unrelated shell interactions"
    );
    tooltip.invoke_renew(
        claim,
        "stale metadata".into(),
        100.0,
        100.0,
        32.0,
        32.0,
        GitHubTooltipPlacement::Top,
        GitHubTooltipAlignment::Center,
    );
    assert!(!tooltip.get_open(), "the detached publisher lost its claim");
    window.set_current_route(Route::PrReview);
    flush(&window);
    assert_eq!(
        page.global::<PrReviewBridge>()
            .get_workflow_pr_review_lines(),
        rows,
        "visual cleanup must preserve the review session's actual model"
    );
    assert!(!tooltip.get_open());
}

#[test]
fn real_pr_popover_and_shell_version_menu_claim_exclusivity_in_both_directions() {
    let window = window();
    usher_lib::install_transient_input_bridge(&window).unwrap();
    usher_lib::install_floating_overlay_positioning(&window);
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window
        .global::<usher_lib::AppState>()
        .set_version("pr-peer-fixture".into());
    slint::select_bundled_translation("en").unwrap();
    install_pr_review_fixture(&window);
    window.set_current_route(Route::PrReview);
    flush(&window);
    let page = pr_review_fixture_view(&window);
    let open = || {
        ElementHandle::find_by_element_type_name(&page, "GitHubDiffSettingsPopover")
            .next()
            .is_some()
    };
    let menus =
        || ElementHandle::find_by_element_id(&window, "GitHubActionMenu::menu-surface").count();
    assert!(!open());
    let host = window.global::<usher_lib::TransientOverlayState>();
    let before = host.get_exclusive_generation();
    ElementHandle::find_by_element_id(&page, "GitHubPullRequestReviewPage::diff-settings-button")
        .next()
        .expect("real PR Diff settings trigger")
        .mock_single_click(PointerEventButton::Left);
    flush(&window);
    assert!(open());
    assert_eq!(host.get_exclusive_generation(), before + 1);

    ElementHandle::find_by_element_id(&window, "GitHubStatusBar::version-item")
        .next()
        .expect("real shell Version trigger")
        .mock_single_click(PointerEventButton::Left);
    flush(&window);
    assert!(!open(), "a shell peer claim closes the PR popover");
    assert_eq!(menus(), 1);
    assert_eq!(host.get_exclusive_generation(), before + 2);

    // The shared outside observer preserves this first factory control click.
    ElementHandle::find_by_element_id(&page, "GitHubPullRequestReviewPage::diff-settings-button")
        .next()
        .expect("real PR Diff settings trigger")
        .mock_single_click(PointerEventButton::Left);
    flush(&window);
    assert!(open(), "the new PR opener must not receive its own echo");
    assert_eq!(menus(), 0, "the PR claim closes the shell Version peer");
    assert_eq!(host.get_exclusive_generation(), before + 3);
}

#[test]
fn macos_readiness_closes_on_factory_blank_click_without_a_root_pointer_shield() {
    let (window, page) = macos_merged_review();
    let review = page.global::<PrReviewBridge>();

    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).expect("packaged locale");
        i_slint_core::translations::mark_all_translations_dirty();
        review.set_workflow_pr_merge_readiness_label(
            usher_i18n::translate_for(locale, "pr_review.readiness.merged")
                .as_ref()
                .into(),
        );
        review.set_workflow_pr_merge_readiness_detail(
            usher_i18n::translate_for(locale, "pr_review.readiness_detail.merged")
                .as_ref()
                .into(),
        );
        flush(&window);
        click_readiness(&window);
        assert!(window.get_workflow_pr_readiness_popup_open());

        ElementHandle::find_by_element_id(&window, "GitHubActionMenu::menu-surface")
            .next()
            .expect("root-mounted readiness surface")
            .mock_single_click(PointerEventButton::Left);
        flush(&window);
        assert!(
            window.get_workflow_pr_readiness_popup_open(),
            "clicking inside the status detail keeps it open"
        );

        ElementHandle::find_by_element_id(
            &page,
            "GitHubPullRequestReviewPage::pr-header-bottom-rule",
        )
        .next()
        .expect("non-interactive factory header divider")
        .mock_single_click(PointerEventButton::Left);
        flush(&window);
        assert!(
            !window.get_workflow_pr_readiness_popup_open(),
            "{locale}: factory blank clicks must dismiss the root readiness popup"
        );

        click_readiness(&window);
        assert!(window.get_workflow_pr_readiness_popup_open());
        click_readiness(&window);
        assert!(
            !window.get_workflow_pr_readiness_popup_open(),
            "the same trigger still closes the status detail"
        );
    }
}

#[test]
fn macos_readiness_and_factory_diff_settings_switch_on_the_first_click() {
    let (window, page) = macos_merged_review();
    let diff_open = || {
        ElementHandle::find_by_element_type_name(&page, "GitHubDiffSettingsPopover")
            .next()
            .is_some()
    };
    click_readiness(&window);
    assert!(window.get_workflow_pr_readiness_popup_open());
    ElementHandle::find_by_element_id(&page, "GitHubPullRequestReviewPage::diff-settings-button")
        .next()
        .expect("real factory Diff view trigger")
        .mock_single_click(PointerEventButton::Left);
    flush(&window);
    assert!(!window.get_workflow_pr_readiness_popup_open());
    assert!(diff_open(), "the switching click opens Diff view settings");

    click_readiness(&window);
    assert!(window.get_workflow_pr_readiness_popup_open());
    assert!(!diff_open(), "the reverse click opens only readiness");

    page.global::<usher_ui_pr_review::TransientOverlayState>()
        .invoke_dismiss_all();
    flush(&window);
    assert!(
        window.get_workflow_pr_readiness_popup_open(),
        "local cleanup is not a user outside-click and must not echo to the shell"
    );
    window.invoke_dismiss_transient_popovers();
    flush(&window);
    assert!(!window.get_workflow_pr_readiness_popup_open());
}

#[test]
fn macos_readiness_closes_when_a_factory_tab_consumes_the_same_click() {
    let (window, page) = macos_merged_review();
    let review = page.global::<PrReviewBridge>();
    let selected = Rc::new(Cell::new(-1));
    let observed = selected.clone();
    review.on_workflow_pr_review_select_tab(move |tab| observed.set(tab));

    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).expect("packaged locale");
        i_slint_core::translations::mark_all_translations_dirty();
        review.set_workflow_pr_review_tab(2);
        selected.set(-1);
        flush(&window);
        click_readiness(&window);
        assert!(window.get_workflow_pr_readiness_popup_open());
        let conversation = format!(
            "{}, 0",
            usher_i18n::translate_for(locale, "pr_review.conversation")
        );
        ElementHandle::find_by_accessible_label(&page, conversation.as_str())
            .next()
            .expect("mounted Conversation tab")
            .mock_single_click(PointerEventButton::Left);
        flush(&window);
        assert_eq!(selected.get(), 0, "the tab receives the switching click");
        assert_eq!(review.get_workflow_pr_review_tab(), 0);
        assert!(
            !window.get_workflow_pr_readiness_popup_open(),
            "{locale}: an interactive page target also dismisses readiness"
        );
    }
}

#[test]
fn macos_readiness_closes_on_selectable_factory_diff_code_without_losing_the_press() {
    let (window, page) = macos_merged_review();
    let review = page.global::<PrReviewBridge>();
    review.set_workflow_pr_review_tab(2);
    review.set_workflow_pr_review_selected_file_index(0);
    review.set_workflow_pr_review_selected_file_path("src/lib.rs".into());
    review.set_workflow_pr_review_code_loading(false);
    let line = WorkflowPrDiffLine {
        old_line: 1,
        new_line: 1,
        kind: "context".into(),
        content: "let value = 1;".into(),
        source_index: 0,
        ..Default::default()
    };
    review.set_workflow_pr_review_lines(ModelRc::new(VecModel::from(vec![line.clone()])));
    review.set_workflow_pr_review_line_metrics(ModelRc::new(VecModel::from(vec![
        WorkflowPrDiffRowMetric {
            kind: "context".into(),
            columns: 14,
            double_hunk: false,
        },
    ])));
    let diff = page.global::<PrDiffBridge>();
    diff.on_workflow_pr_review_layout_diff_virtual_window(move |_, _, _, _, _, _, _, _| {
        WorkflowPrDiffVirtualWindow {
            rows: ModelRc::new(VecModel::from(vec![WorkflowPrDiffVirtualRow {
                index: 0,
                start: 0.0,
                size: 22.0,
                line: line.clone(),
            }])),
            scroll_offset: 0.0,
            content_height: 22.0,
        }
    });
    let selections = Rc::new(Cell::new(0));
    let observed = selections.clone();
    diff.on_workflow_pr_review_resolve_diff_selection(move |_, _, _, _, _, _, _, _, _, _| {
        observed.set(observed.get() + 1);
        WorkflowPrDiffSelectionHit { line: 0, column: 0 }
    });
    review.set_workflow_pr_review_line_content_revision(1);
    flush(&window);
    click_readiness(&window);
    assert!(window.get_workflow_pr_readiness_popup_open());
    ElementHandle::find_by_element_id(&page, "GitHubPullRequestReviewPage::unified-code")
        .next()
        .expect("real factory unified diff code")
        .mock_single_click(PointerEventButton::Left);
    flush(&window);
    assert!(
        selections.get() > 0,
        "code selection receives the same press"
    );
    assert!(
        !window.get_workflow_pr_readiness_popup_open(),
        "selectable diff code dismisses the shell status detail"
    );
}

#[test]
fn focused_pr_factory_forwards_shortcuts_once_and_tabs_across_the_shell_boundary() {
    let window = window();
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    slint::select_bundled_translation("en").unwrap();
    install_pr_review_fixture(&window);
    window.set_current_route(Route::PrReview);
    flush(&window);
    let initial = focus_label(&window, "Conversation, 0");
    let factory = top_tree(initial);
    assert!(!ItemTreeRc::ptr_eq(
        &factory,
        &WindowInner::from_pub(window.window()).component()
    ));
    let shortcuts = Rc::new(Cell::new(0));
    let tabs = Rc::new(Cell::new(0));
    let observed = shortcuts.clone();
    let forwarded = tabs.clone();
    window.on_app_shortcut_key_pressed(move |text, _, _, _, meta| {
        if text == slint::SharedString::from(Key::Tab)
            || text == slint::SharedString::from(Key::Backtab)
        {
            forwarded.set(forwarded.get() + 1);
        }
        if meta && text == "k" {
            observed.set(observed.get() + 1);
            return true;
        }
        false
    });
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Meta.into(),
    });
    key(&window, "k");
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Meta.into(),
    });
    assert_eq!(shortcuts.get(), 1);

    let mut boundary = None;
    for _ in 0..64 {
        let before = focused(&window);
        let count = tabs.get();
        key(&window, Key::Backtab);
        assert_eq!(tabs.get(), count + 1, "Tab crosses the adapter once");
        assert_ne!(focused(&window), before, "reverse Tab advances focus");
        if !ItemTreeRc::ptr_eq(&top_tree(focused(&window)), &factory) {
            boundary = Some(before);
            break;
        }
    }
    let boundary = boundary.expect("reverse Tab must escape the private PR root");
    key(&window, Key::Tab);
    assert_eq!(
        focused(&window),
        boundary,
        "forward Tab reenters the same edge"
    );
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Shift.into(),
    });
    key(&window, Key::Tab);
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Shift.into(),
    });
    assert!(!ItemTreeRc::ptr_eq(&top_tree(focused(&window)), &factory));
}
