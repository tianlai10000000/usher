use super::*;
use i_slint_backend_testing::ElementHandle;
use i_slint_core::accessibility::AccessibleStringProperty;
use i_slint_core::input::FocusReason;
use i_slint_core::item_tree::{ItemRc, ItemVisitorResult, TraversalOrder, visit_items};
use i_slint_core::items::{AccessibleRole, FocusScope};
use i_slint_core::window::WindowInner;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[test]
fn factory_claim_closes_host_peers_without_echoing_to_its_opener() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = crate::MainWindow::new().unwrap();
    let page = super::super::create_page(&window).unwrap();
    window.set_current_route(crate::Route::UiComponents);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(window.global::<crate::UiComponentsBridge>().get_active());
    bind(&window, &page);
    let dismissals = Rc::new(RefCell::new(Vec::new()));
    let captured = dismissals.clone();
    window
        .global::<crate::UiComponentsBridge>()
        .on_dismiss(move |all| captured.borrow_mut().push(all));
    let host = window.global::<crate::TransientOverlayState>();
    let before = host.get_exclusive_generation();
    page.invoke_claim_host_exclusive();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert_eq!(host.get_exclusive_generation(), before + 1);
    assert!(
        dismissals.borrow().is_empty(),
        "do not dismiss the local opener"
    );

    host.invoke_claim_exclusive();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert_eq!(
        *dismissals.borrow(),
        vec![false],
        "external claims close only popovers"
    );
    page.invoke_claim_host_exclusive();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert_eq!(host.get_exclusive_generation(), before + 3);
    assert_eq!(
        *dismissals.borrow(),
        vec![false],
        "a new local claim must not echo"
    );
}

#[test]
fn inactive_or_destroyed_host_ignores_factory_claims() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = crate::MainWindow::new().unwrap();
    let page = super::super::create_page(&window).unwrap();
    bind(&window, &page);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    let before = window
        .global::<crate::TransientOverlayState>()
        .get_exclusive_generation();
    page.invoke_claim_host_exclusive();
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert_eq!(
        window
            .global::<crate::TransientOverlayState>()
            .get_exclusive_generation(),
        before
    );
    drop(window);
    page.invoke_claim_host_exclusive();
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
}

fn find(window: &crate::MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing mounted control {id}"))
}

fn filter_open(window: &crate::MainWindow) -> bool {
    ElementHandle::find_by_element_id(window, "GitHubUiComponentsOverlay::filter-menu")
        .next()
        .is_some()
}

fn menu_count(window: &crate::MainWindow) -> usize {
    ElementHandle::find_by_element_id(window, "GitHubActionMenu::menu-surface").count()
}

fn focus_filter(window: &crate::MainWindow) {
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
                    .is_some_and(|label| label == "Status")
            {
                target = Some(item);
                return ItemVisitorResult::Abort;
            }
            ItemVisitorResult::Continue(())
        },
        (),
    );
    host.set_focus_item(
        &target.expect("mounted factory Status filter"),
        true,
        FocusReason::Programmatic,
    );
}

#[test]
fn actual_factory_filter_and_initially_open_version_menu_claim_peer_exclusivity() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = crate::MainWindow::new().unwrap();
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1000.0, 800.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window
        .global::<crate::AppState>()
        .set_version("peer-fixture".into());
    crate::i18n::apply_preference("en").unwrap();
    crate::install_floating_overlay_positioning(&window);
    super::super::install(&window);
    window.set_current_route(crate::Route::UiComponents);
    settle();
    ElementHandle::find_by_accessible_label(&window, "Selection")
        .find(|element| element.accessible_role() == Some(AccessibleRole::Button))
        .expect("real gallery directory link")
        .mock_single_click(PointerEventButton::Left);
    settle();
    let document = find(&window, "GitHubUiComponentsWindow::document");
    // Selection begins with Checkbox/Radio/Select sections. The testing query
    // excludes clipped controls, so scroll the real viewport before finding
    // the later Status filter; it is not visible at the directory jump target.
    for _ in 0..20 {
        if ElementHandle::find_by_element_id(
            &window,
            "GitHubUiComponentsGallery::status-filter-button",
        )
        .next()
        .is_some()
        {
            break;
        }
        document.scroll(0.0, -200.0);
        settle();
    }
    let filter = find(&window, "GitHubUiComponentsGallery::status-filter-button");
    document.scroll(0.0, 260.0 - filter.absolute_position().y);
    settle();
    let before = window
        .global::<crate::TransientOverlayState>()
        .get_exclusive_generation();
    find(&window, "GitHubUiComponentsGallery::status-filter-button")
        .mock_single_click(PointerEventButton::Left);
    settle();
    assert!(filter_open(&window));
    assert_eq!(menu_count(&window), 1);
    let host = window.global::<crate::TransientOverlayState>();
    assert_eq!(host.get_exclusive_generation(), before + 1);

    for cycle in 0..3 {
        let before = host.get_exclusive_generation();
        // This real pointer click conditionally creates ActionMenu { open:true }.
        // Before the fix, only the existing factory menu had claimed a generation.
        find(&window, "GitHubStatusBar::version-item").mock_single_click(PointerEventButton::Left);
        settle();
        assert!(
            !filter_open(&window),
            "cycle {cycle}: Version closes factory filter"
        );
        assert_eq!(menu_count(&window), 1, "only the Version menu remains");
        assert_eq!(
            host.get_exclusive_generation(),
            before + 1,
            "claim once on init"
        );
        assert_eq!(
            WindowInner::from_pub(window.window())
                .focus_item
                .borrow()
                .upgrade()
                .unwrap()
                .accessible_role(),
            AccessibleRole::List,
            "initial open takes focus through the same path as a live toggle"
        );
        settle();
        assert_eq!(
            host.get_exclusive_generation(),
            before + 1,
            "no duplicate claim"
        );

        // The status outside-click surface intentionally owns a pointer click.
        // Activate the real factory filter by keyboard to test the reverse peer
        // claim without changing that existing click-dismiss behavior.
        focus_filter(&window);
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Return.into(),
        });
        window.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Return.into(),
        });
        settle();
        assert!(
            filter_open(&window),
            "cycle {cycle}: factory filter remains open"
        );
        assert_eq!(
            menu_count(&window),
            1,
            "factory claim closes the Version peer"
        );
        assert_eq!(
            host.get_exclusive_generation(),
            before + 2,
            "claim once on reopen"
        );
    }
}
