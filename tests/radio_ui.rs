#![cfg(feature = "ui-components")]

use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize};
use usher_lib::{MainWindow, Route};

fn find(window: &MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing UI element {id}"))
}

#[test]
fn radio_optional_description_preserves_single_and_caption_geometry() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    slint::select_bundled_translation("en").unwrap();
    window.set_show_navigation_drawer(false);
    window.set_platform_is_macos(false);
    window.set_current_route(Route::UiComponents);
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1_000.0, 900.0),
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));

    let scroll = find(&window, "MainWindow::page-scroll-view");
    for _ in 0..24 {
        if ElementHandle::find_by_element_id(&window, "GitHubUiComponentsGallery::radio-single")
            .next()
            .is_some()
        {
            break;
        }
        scroll.scroll(0.0, -300.0);
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    }

    let single = find(&window, "GitHubUiComponentsGallery::radio-single");
    let caption = find(&window, "GitHubUiComponentsGallery::radio-description");

    assert!((single.size().height - 32.0).abs() < 0.5);
    assert!(
        single
            .query_descendants()
            .match_id("GitHubRadio::description-label")
            .find_first()
            .is_none(),
        "single-line Radio must not mount a visible description"
    );

    let caption_y = caption.absolute_position().y;
    let title = caption
        .query_descendants()
        .match_id("GitHubRadio::label")
        .find_first()
        .expect("caption Radio title");
    let description = caption
        .query_descendants()
        .match_id("GitHubRadio::description-label")
        .find_first()
        .expect("caption Radio description");

    assert!((caption.size().height - 52.0).abs() < 0.5);
    assert!((title.absolute_position().y - caption_y).abs() < 0.5);
    assert!((title.size().height - 32.0).abs() < 0.5);
    assert!((description.absolute_position().y - caption_y - 32.0).abs() < 0.5);
    assert!((description.size().height - 20.0).abs() < 0.5);
    assert!(
        title.absolute_position().y + title.size().height
            <= description.absolute_position().y + 0.5
    );
}
