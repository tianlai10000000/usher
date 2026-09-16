#![cfg(feature = "ui-components")]

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
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
fn bordered_select_menu_floats_tracks_and_avoids_edges() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    usher_lib::install_floating_overlay_positioning(&window);
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1000.0, 800.0),
    });
    slint::select_bundled_translation("en").unwrap();
    settle();
    ElementHandle::find_by_element_type_name(&window, "GitHubLink")
        .find(|link| link.accessible_label().as_deref() == Some("Selection"))
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    let scroll = find(&window, "MainWindow::page-scroll-view");
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
        scroll.scroll(0.0, -200.0);
        settle();
    }
    let trigger = find(&window, "GitHubUiComponentsGallery::status-filter-button");
    scroll.scroll(0.0, 300.0 - trigger.absolute_position().y);
    settle();
    let section = find(&window, "GitHubUiComponentsGallery::filter-menu-section");
    let section_size = section.size();
    let sample = find(&window, "GitHubPrMenuShowcase::short-rules-button");
    let sample_position = sample.absolute_position();
    trigger.mock_single_click(PointerEventButton::Left);
    settle();
    let menu = find(&window, "GitHubUiComponentsOverlay::filter-menu");
    assert_eq!(menu.size().height, 160.0);
    assert!(
        (menu.absolute_position().x - trigger.absolute_position().x).abs() < 0.5,
        "Primer default aligns the menu with the trigger's start edge"
    );
    assert!(
        (menu.absolute_position().y - trigger.absolute_position().y - trigger.size().height - 4.0)
            .abs()
            < 0.5
    );
    assert_eq!(
        section.size(),
        section_size,
        "opening a menu must not grow the section"
    );
    assert_eq!(
        sample.absolute_position(),
        sample_position,
        "opening must not push the borderless sample"
    );
    assert!(menu.absolute_position().y + menu.size().height > sample_position.y);
    for (locale, label) in [("en", "Archived"), ("zh-CN", "已归档")] {
        slint::select_bundled_translation(locale).unwrap();
        settle();
        assert!(
            ElementHandle::find_by_accessible_label(&window, label)
                .next()
                .is_some()
        );
        assert_eq!(section.size(), section_size);
    }
    ElementHandle::find_by_accessible_label(&window, "已归档")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    settle();
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsOverlay::filter-menu")
            .next()
            .is_none()
    );
    trigger.mock_single_click(PointerEventButton::Left);
    settle();
    let before = find(&window, "GitHubUiComponentsOverlay::filter-menu").absolute_position();
    scroll.scroll(0.0, -30.0);
    settle();
    let after = find(&window, "GitHubUiComponentsOverlay::filter-menu").absolute_position();
    assert!((after.y - before.y + 30.0).abs() < 0.5);
    find(&window, "GitHubUiComponentsGallery::owner-filter-button")
        .mock_single_click(PointerEventButton::Left);
    settle();
    assert_eq!(
        find(&window, "GitHubUiComponentsOverlay::filter-menu")
            .size()
            .height,
        128.0
    );
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Escape.into(),
    });
    settle();
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsOverlay::filter-menu")
            .next()
            .is_none()
    );
    scroll.scroll(0.0, 710.0 - trigger.absolute_position().y);
    settle();
    trigger.mock_single_click(PointerEventButton::Left);
    settle();
    let menu = find(&window, "GitHubUiComponentsOverlay::filter-menu");
    assert!(menu.absolute_position().y + menu.size().height <= trigger.absolute_position().y - 4.0);
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(760.0, 760.0),
    });
    settle();
    let menu = find(&window, "GitHubUiComponentsOverlay::filter-menu");
    assert!(menu.absolute_position().x >= 0.0);
    assert!(menu.absolute_position().x + menu.size().width <= 760.0);
    assert!(menu.absolute_position().y >= 0.0);
    assert!(menu.absolute_position().y + menu.size().height <= 760.0);
    scroll.scroll(0.0, -900.0);
    settle();
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsOverlay::filter-menu")
            .next()
            .is_none()
    );
    // Crossing the form's compact breakpoint moves the trigger. Reopen only
    // after bringing that new trigger geometry back into the visible viewport.
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(600.0, 760.0),
    });
    settle();
    scroll.scroll(0.0, 300.0 - trigger.absolute_position().y);
    settle();
    trigger.mock_single_click(PointerEventButton::Left);
    settle();
    let menu = find(&window, "GitHubUiComponentsOverlay::filter-menu");
    assert!((menu.absolute_position().x - trigger.absolute_position().x).abs() < 0.5);
    assert!(menu.absolute_position().x + menu.size().width <= 600.0);
    assert!(menu.absolute_position().y + menu.size().height <= 760.0);
}
