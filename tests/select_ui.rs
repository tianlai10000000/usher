#![cfg(feature = "ui-components")]

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, Route};

#[test]
fn showcase_select_layout_and_popup_stay_inside_their_surfaces() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    for (width, height) in [(800.0, 600.0), (1280.0, 900.0)] {
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
        let disabled = find("GitHubUiComponentsGallery::select-disabled");
        let field = find("GitHubUiComponentsGallery::select-name");
        assert!(
            trigger.absolute_position().x + trigger.size().width <= disabled.absolute_position().x
        );
        assert_eq!(
            trigger.absolute_position().y,
            disabled.absolute_position().y
        );
        assert_eq!(trigger.absolute_position().x, field.absolute_position().x);
        assert!(
            trigger.absolute_position().y + trigger.size().height < field.absolute_position().y
        );

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
        for label in ["System default", "English", "Chinese (Simplified)"] {
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
