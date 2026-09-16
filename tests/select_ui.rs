#![cfg(feature = "ui-components")]

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, Route};

#[test]
fn showcase_select_layout_and_popup_stay_inside_their_surfaces() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    slint::select_bundled_translation("en").unwrap();
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    for (width, height) in [
        (480.0, 600.0),
        (600.0, 600.0),
        (800.0, 600.0),
        (1280.0, 900.0),
    ] {
        window.window().dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width, height),
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
                "GitHubUiComponentsGallery::select-trigger",
            )
            .next()
            .is_some()
            {
                break;
            }
            scroll.scroll(0.0, -300.0);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        }
        let trigger = find("GitHubUiComponentsGallery::select-trigger");
        scroll.scroll(0.0, 150.0 - trigger.absolute_position().y);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        let trigger = find("GitHubUiComponentsGallery::select-trigger");
        let grouped = find("GitHubUiComponentsGallery::grouped-select-trigger");
        let disabled = find("GitHubUiComponentsGallery::select-disabled");
        let field = find("GitHubUiComponentsGallery::select-name");
        let section = find("GitHubUiComponentsGallery::select-section");
        let controls = [&trigger, &grouped, &disabled, &field];
        for (index, control) in controls.iter().enumerate() {
            let position = control.absolute_position();
            let size = control.size();
            assert!(position.x >= section.absolute_position().x);
            assert!(
                position.x + size.width
                    <= section.absolute_position().x + section.size().width + 0.5
            );
            assert!(
                position.y + size.height
                    <= section.absolute_position().y + section.size().height + 0.5
            );
            assert!((size.width - trigger.size().width).abs() < 0.5);
            for other in &controls[index + 1..] {
                let other_position = other.absolute_position();
                let other_size = other.size();
                assert!(
                    position.x + size.width <= other_position.x + 0.5
                        || other_position.x + other_size.width <= position.x + 0.5
                        || position.y + size.height <= other_position.y + 0.5
                        || other_position.y + other_size.height <= position.y + 0.5,
                    "showcase controls overlap at window width {width}"
                );
            }
        }
        if section.size().width < 544.0 {
            assert_eq!(trigger.absolute_position().x, grouped.absolute_position().x);
            assert!(
                trigger.absolute_position().y + trigger.size().height
                    < grouped.absolute_position().y
            );
            assert!(
                field.absolute_position().y + field.size().height < disabled.absolute_position().y
            );
        } else {
            assert_eq!(trigger.absolute_position().y, grouped.absolute_position().y);
            assert_eq!(
                grouped.absolute_position().x,
                disabled.absolute_position().x
            );
            assert!(
                (disabled.absolute_position().y - field.absolute_position().y - 26.0).abs() < 0.5
            );
        }
        assert_eq!(trigger.absolute_position().x, field.absolute_position().x);
        assert!(
            trigger.absolute_position().y + trigger.size().height < field.absolute_position().y
        );

        grouped.mock_single_click(PointerEventButton::Left);
        assert_eq!(
            grouped.accessible_expanded(),
            Some(true),
            "grouped opens before observer"
        );
        let anchor = grouped.absolute_position();
        window.invoke_post_dispatch_pointer_released(
            anchor.x + grouped.size().width / 2.0,
            anchor.y + grouped.size().height / 2.0,
        );
        for _ in 0..3 {
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
            assert_eq!(
                grouped.accessible_expanded(),
                Some(true),
                "grouped Select must remain open after pointer release and layout frames at width {width}"
            );
        }
        let grouped_menu = find("GitHubSelectPopup::menu");
        assert!((grouped_menu.absolute_position().x - grouped.absolute_position().x).abs() < 0.5);
        assert!(
            (grouped_menu.absolute_position().y
                - grouped.absolute_position().y
                - grouped.size().height
                - 4.0)
                .abs()
                < 0.5
        );
        for label in [
            "Pull request review submitted",
            "AI review completed",
            "Tag created",
            "Deployment completed",
        ] {
            assert!(
                ElementHandle::find_by_accessible_label(&window, label)
                    .any(|row| row.accessible_checked().is_some()),
                "missing grouped option {label}"
            );
        }
        grouped.mock_single_click(PointerEventButton::Left);
        assert_eq!(grouped.accessible_expanded(), Some(false));
        // In the compact column the plain popup covers the grouped trigger.
        if section.size().width >= 544.0 {
            trigger.mock_single_click(PointerEventButton::Left);
            assert_eq!(trigger.accessible_expanded(), Some(true));
        }
        grouped.mock_single_click(PointerEventButton::Left);
        assert_eq!(grouped.accessible_expanded(), Some(true));
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        assert_eq!(trigger.accessible_expanded(), Some(false));
        assert_eq!(grouped.accessible_expanded(), Some(true));
        let first_group_row =
            ElementHandle::find_by_accessible_label(&window, "AI review completed")
                .find(|row| row.accessible_checked().is_some())
                .unwrap();
        first_group_row.mock_single_click(PointerEventButton::Left);
        assert_eq!(
            grouped.accessible_label().as_deref(),
            Some("AI review completed")
        );
        grouped.mock_single_click(PointerEventButton::Left);
        let row = ElementHandle::find_by_accessible_label(&window, "Tag created")
            .find(|row| row.accessible_checked().is_some())
            .unwrap();
        row.mock_single_click(PointerEventButton::Left);
        assert_eq!(grouped.accessible_expanded(), Some(false));
        assert_eq!(grouped.accessible_label().as_deref(), Some("Tag created"));
        grouped.mock_single_click(PointerEventButton::Left);
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::End.into(),
        });
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Return.into(),
        });
        assert_eq!(grouped.accessible_expanded(), Some(false));
        assert_eq!(
            grouped.accessible_label().as_deref(),
            Some("Deployment completed")
        );
        grouped.mock_single_click(PointerEventButton::Left);
        trigger.mock_single_click(PointerEventButton::Left);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        assert_eq!(grouped.accessible_expanded(), Some(false));
        assert_eq!(trigger.accessible_expanded(), Some(true));
        trigger.mock_single_click(PointerEventButton::Left);
        grouped.mock_single_click(PointerEventButton::Left);
        scroll.scroll(0.0, -40.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        assert_eq!(grouped.accessible_expanded(), Some(false));

        scroll.scroll(0.0, height - 80.0 - trigger.absolute_position().y);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        let trigger = find("GitHubUiComponentsGallery::select-trigger");
        trigger.mock_single_click(PointerEventButton::Left);
        let menu = find("GitHubSelectPopup::menu");
        let position = menu.absolute_position();
        let size = menu.size();
        assert!(position.x >= 8.0 && position.y >= 8.0);
        assert!(position.x + size.width <= width - 7.5);
        assert!(position.y + size.height <= height - 7.5);
        assert!(
            position.y + size.height <= trigger.absolute_position().y - 3.5,
            "menu near the bottom must flip above its trigger"
        );
        for label in ["System default", "English", "简体中文"] {
            let row = ElementHandle::find_by_accessible_label(&window, label)
                .find(|row| row.accessible_checked().is_some())
                .unwrap();
            assert!((row.absolute_position().x - position.x - 4.0).abs() < 0.5);
            assert!((row.size().width - size.width + 8.0).abs() < 0.5);
        }
        window.invoke_post_dispatch_pointer_released(1.0, height - 1.0);
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubSelectPopup::menu")
                .next()
                .is_none()
        );
        trigger.mock_single_click(PointerEventButton::Left);
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubSelectPopup::menu")
                .next()
                .is_some()
        );
        scroll.scroll(0.0, -40.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
        assert!(
            ElementHandle::find_by_element_id(&window, "GitHubSelectPopup::menu")
                .next()
                .is_none(),
            "moving the trigger must dismiss the menu at its old anchor"
        );
    }
}
