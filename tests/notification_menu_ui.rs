#![cfg(feature = "ui-components")]

#[path = "support/ui_components.rs"]
mod playground;

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize, ModelRc, VecModel};
use usher_lib::{MainWindow, Notification, NotificationManager, Route};

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
fn notification_menu_escapes_showcase_sections_and_tracks_viewport() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    playground::install(&window);
    usher_lib::install_floating_overlay_positioning(&window);
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    window.window().set_size(LogicalSize::new(1000.0, 800.0));
    slint::select_bundled_translation("en").unwrap();
    settle();
    ElementHandle::find_by_element_type_name(&window, "GitHubLink")
        .find(|link| link.accessible_label().as_deref() == Some("Communication"))
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    let scroll = playground::document(&window);
    playground::reveal(
        &window,
        &scroll,
        "GitHubNotificationBanner::queue-menu-button",
    );

    for (locale, dismiss_label) in [("en", "Dismiss all"), ("zh-CN", "关闭全部通知")] {
        slint::select_bundled_translation(locale).unwrap();
        settle();
        let trigger = playground::reveal(
            &window,
            &scroll,
            "GitHubNotificationBanner::queue-menu-button",
        );
        scroll.scroll(0.0, 400.0 - trigger.absolute_position().y);
        settle();
        let trigger = find(&window, "GitHubNotificationBanner::queue-menu-button");
        trigger.mock_single_click(PointerEventButton::Left);
        settle();
        let menu = find(&window, "GitHubUiComponentsOverlay::notification-menu");
        assert_eq!(menu.size().height, 80.0);
        assert!(
            (menu.absolute_position().y
                - trigger.absolute_position().y
                - trigger.size().height
                - 6.0)
                .abs()
                < 0.5,
            "downward placement must use the footer button"
        );
        assert!(
            (menu.absolute_position().x + menu.size().width
                - trigger.absolute_position().x
                - trigger.size().width)
                .abs()
                < 0.5
        );
        let row = ElementHandle::find_by_accessible_label(&window, dismiss_label)
            .next()
            .unwrap();
        let next_header = find(&window, "GitHubShowcaseGroup::header-band");
        assert!(
            row.absolute_position().y + row.size().height > next_header.absolute_position().y,
            "the regression must exercise the menu over the following section"
        );
        let covered_position = LogicalPosition::new(
            row.absolute_position().x + row.size().width / 2.0,
            (next_header
                .absolute_position()
                .y
                .max(row.absolute_position().y)
                + row.absolute_position().y
                + row.size().height)
                / 2.0,
        );
        assert!(covered_position.y > next_header.absolute_position().y);
        window.window().dispatch_event(WindowEvent::PointerMoved {
            position: covered_position,
        });
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position: covered_position,
            button: PointerEventButton::Left,
        });
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: covered_position,
                button: PointerEventButton::Left,
            });
        settle();
        assert!(
            ElementHandle::find_by_element_id(
                &window,
                "GitHubUiComponentsOverlay::notification-menu"
            )
            .next()
            .is_none(),
            "the menu row must receive clicks above the next section"
        );

        trigger.mock_single_click(PointerEventButton::Left);
        settle();
        let before =
            find(&window, "GitHubUiComponentsOverlay::notification-menu").absolute_position();
        scroll.scroll(0.0, -30.0);
        settle();
        let after =
            find(&window, "GitHubUiComponentsOverlay::notification-menu").absolute_position();
        assert!((after.y - before.y + 30.0).abs() < 0.5);
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        settle();
        let trigger = find(&window, "GitHubNotificationBanner::queue-menu-button");
        scroll.scroll(0.0, 700.0 - trigger.absolute_position().y);
        settle();
        let trigger = find(&window, "GitHubNotificationBanner::queue-menu-button");
        trigger.mock_single_click(PointerEventButton::Left);
        settle();
        let menu = find(&window, "GitHubUiComponentsOverlay::notification-menu");
        assert!(
            (trigger.absolute_position().y - menu.absolute_position().y - menu.size().height - 6.0)
                .abs()
                < 0.5,
            "upward placement must stay next to the footer button, not above the card"
        );
        assert!(
            (menu.absolute_position().x + menu.size().width
                - trigger.absolute_position().x
                - trigger.size().width)
                .abs()
                < 0.5
        );
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Escape.into(),
        });
        settle();
    }

    let trigger = find(&window, "GitHubNotificationBanner::queue-menu-button");
    scroll.scroll(0.0, 700.0 - trigger.absolute_position().y);
    settle();
    find(&window, "GitHubNotificationBanner::queue-menu-button")
        .mock_single_click(PointerEventButton::Left);
    settle();
    let menu = find(&window, "GitHubUiComponentsOverlay::notification-menu");
    assert_eq!(menu.size().height, 80.0);
    assert!(menu.absolute_position().y + menu.size().height <= 800.0);
    window.window().set_size(LogicalSize::new(760.0, 760.0));
    settle();
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsOverlay::notification-menu")
            .next()
            .is_none(),
        "responsive reflow moved the old notification anchor outside the viewport"
    );
    let trigger = playground::reveal(
        &window,
        &scroll,
        "GitHubNotificationBanner::queue-menu-button",
    );
    scroll.scroll(0.0, 660.0 - trigger.absolute_position().y);
    settle();
    trigger.mock_single_click(PointerEventButton::Left);
    settle();
    let menu = find(&window, "GitHubUiComponentsOverlay::notification-menu");
    assert!(menu.absolute_position().x >= 0.0);
    assert!(menu.absolute_position().x + menu.size().width <= 760.0);
    assert!(menu.absolute_position().y + menu.size().height <= 760.0);
    let trigger = find(&window, "GitHubNotificationBanner::queue-menu-button");
    assert!(
        (trigger.absolute_position().y - menu.absolute_position().y - menu.size().height - 6.0)
            .abs()
            < 0.5,
        "resizing must preserve the button anchor"
    );
    scroll.scroll(0.0, -800.0);
    settle();
    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubUiComponentsOverlay::notification-menu")
            .next()
            .is_none()
    );

    // The application notification host keeps using the same menu content.
    window.set_current_route(Route::Home);
    window
        .global::<NotificationManager>()
        .set_queue(ModelRc::new(VecModel::from(vec![
            Notification {
                id: 1,
                title: "First".into(),
                ..Default::default()
            },
            Notification {
                id: 2,
                title: "Second".into(),
                ..Default::default()
            },
        ])));
    settle();
    find(&window, "GitHubNotificationBanner::queue-menu-button")
        .mock_single_click(PointerEventButton::Left);
    settle();
    let menu = find(&window, "GitHubNotificationBanner::queue-actions-menu");
    let trigger = find(&window, "GitHubNotificationBanner::queue-menu-button");
    assert_eq!(menu.size().height, 80.0);
    assert!(
        (menu.absolute_position().y - trigger.absolute_position().y - trigger.size().height - 6.0)
            .abs()
            < 0.5,
        "production notifications use the same button anchor"
    );
}
