#![cfg(feature = "ui-components")]

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize};
use std::time::Duration;
use usher_lib::{MainWindow, Route};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
}

#[track_caller]
fn find(window: &MainWindow, id: &str) -> ElementHandle {
    match ElementHandle::find_by_element_id(window, id).next() {
        Some(element) => element,
        None => panic!("missing {id}"),
    }
}

#[test]
fn compact_pr_menu_escapes_sections_and_closes_when_anchor_leaves_view() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    usher_lib::install_floating_overlay_positioning(&window);
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(640.0, 800.0),
    });

    for (locale, directory) in [("en", "Selection"), ("zh-CN", "选择")] {
        window.set_current_route(Route::Home);
        window.set_current_route(Route::UiComponents);
        slint::select_bundled_translation(locale).unwrap();
        settle();
        ElementHandle::find_by_element_type_name(&window, "GitHubLink")
            .find(|link| link.accessible_label().as_deref() == Some(directory))
            .unwrap()
            .mock_single_click(PointerEventButton::Left);
        settle();
        let scroll = find(&window, "MainWindow::page-scroll-view");
        for _ in 0..30 {
            if ElementHandle::find_by_element_id(&window, "GitHubPullRequestFlow::more-btn")
                .next()
                .is_some()
            {
                break;
            }
            scroll.scroll(0.0, -150.0);
            settle();
        }
        let trigger = find(&window, "GitHubPullRequestFlow::more-btn");
        scroll.scroll(0.0, 160.0 - trigger.absolute_position().y);
        settle();
        trigger.mock_single_click(PointerEventButton::Left);
        settle();
        let menu = find(&window, "GitHubPullRequestMenuOverlay::filter-menu");
        assert_eq!(menu.size().height, 390.0);
        let original = menu.absolute_position();
        scroll.scroll(0.0, -20.0);
        settle();
        let menu = find(&window, "GitHubPullRequestMenuOverlay::filter-menu");
        assert!((menu.absolute_position().y - original.y + 20.0).abs() < 0.5);
        let flow = find(&window, "GitHubPrMenuShowcase::flow");
        let last = find(&window, "PullRequestSortOptions::sort-newest");
        assert!(
            last.absolute_position().y + last.size().height
                > flow.absolute_position().y + flow.size().height,
            "exercise the popup beyond its original list host"
        );
        last.mock_single_click(PointerEventButton::Left);
        settle();
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
                .next()
                .is_none(),
            "the row outside the list must receive the click"
        );
        trigger.mock_single_click(PointerEventButton::Left);
        settle();
        scroll.scroll(0.0, -250.0);
        settle();
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
                .next()
                .is_none(),
            "an offscreen trigger must not leave a menu pinned over later chapters"
        );
        scroll.scroll(0.0, 250.0);
        settle();
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
                .next()
                .is_none(),
            "scrolling back must not reopen the dismissed menu"
        );
        trigger.mock_single_click(PointerEventButton::Left);
        settle();
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
                .next()
                .is_some()
        );
        window.set_current_route(Route::Home);
        settle();
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubPullRequestMenuOverlay::filter-menu")
                .next()
                .is_none()
        );
    }
}
