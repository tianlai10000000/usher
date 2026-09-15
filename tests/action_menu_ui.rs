#![cfg(feature = "ui-components")]

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, Route};

#[test]
fn showcase_action_menu_anchors_floats_and_toggles() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1000.0, 800.0),
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let find = |id| {
        ElementHandle::find_by_element_id(&window, id)
            .next()
            .unwrap_or_else(|| panic!("missing UI element {id}"))
    };
    let scroll = find("MainWindow::page-scroll-view");
    for _ in 0..20 {
        if ElementHandle::find_by_element_id(
            &window,
            "GitHubUiComponentsGallery::start-action-button",
        )
        .next()
        .is_some()
        {
            break;
        }
        scroll.scroll(0.0, -300.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    }
    let start = find("GitHubUiComponentsGallery::start-action-button");
    scroll.scroll(0.0, 280.0 - start.absolute_position().y);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));

    let section = find("GitHubUiComponentsGallery::action-menu-section");
    let section_position = section.absolute_position();
    let section_size = section.size();
    let start = find("GitHubUiComponentsGallery::start-action-button");
    start.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(start.accessible_checked(), Some(true));
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!((menu.absolute_position().x - start.absolute_position().x).abs() < 0.5);
    assert!(
        (menu.absolute_position().y - start.absolute_position().y - start.size().height - 4.0)
            .abs()
            < 0.5
    );
    assert_eq!(section.absolute_position(), section_position);
    assert_eq!(section.size(), section_size);

    start.mock_single_click(PointerEventButton::Left);
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsGallery::action-menu")
            .next()
            .is_none(),
        "a second click on the active trigger must close the menu"
    );

    let end = find("GitHubUiComponentsGallery::end-action-button");
    end.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!(
        (menu.absolute_position().x + menu.size().width
            - end.absolute_position().x
            - end.size().width)
            .abs()
            < 0.5
    );
    assert!(
        (menu.absolute_position().y - end.absolute_position().y - end.size().height - 4.0).abs()
            < 0.5
    );

    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(760.0, 700.0),
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    let end = find("GitHubUiComponentsGallery::end-action-button");
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!(
        (menu.absolute_position().x + menu.size().width
            - end.absolute_position().x
            - end.size().width)
            .abs()
            < 0.5,
        "menu must remain end-aligned after a window resize"
    );
    assert!(
        (menu.absolute_position().y - end.absolute_position().y - end.size().height - 4.0).abs()
            < 0.5,
        "menu must remain below its trigger after a window resize"
    );

    let resized_section_position = section.absolute_position();
    let resized_section_size = section.size();
    let above = find("GitHubUiComponentsGallery::above-action-button");
    above.mock_single_click(PointerEventButton::Left);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
    let menu = find("GitHubUiComponentsGallery::action-menu");
    assert!((menu.absolute_position().x - above.absolute_position().x).abs() < 0.5);
    assert!(
        (menu.absolute_position().y + menu.size().height - above.absolute_position().y + 4.0).abs()
            < 0.5
    );
    assert_eq!(section.absolute_position(), resized_section_position);
    assert_eq!(section.size(), resized_section_size);
}
