use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};
use usher_lib::{MainWindow, Route};

fn click(window: &slint::Window, position: LogicalPosition) {
    window.dispatch_event(WindowEvent::PointerMoved { position });
    window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

#[test]
fn pr_review_keeps_the_status_bar_interactive_until_a_dialog_opens() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(800.0, 600.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_current_route(Route::PrReview);

    let navigations = Rc::new(Cell::new(0));
    let observed = navigations.clone();
    window.on_navigate(move |_| observed.set(observed.get() + 1));

    // The notification item is the second 28px target in the fixed 200px
    // cluster, which is inset 8px from the right edge of the 800px window.
    let notification = LogicalPosition::new(638.0, 586.0);
    click(window.window(), notification);
    assert_eq!(navigations.get(), 1, "PR Review must expose the status bar");
    assert!(
        ElementHandle::find_by_element_id(&window, "MainWindow::pr-review-status-bar-backdrop")
            .next()
            .is_none(),
        "the status-bar backdrop must stay unmounted without a dialog"
    );

    window.set_workflow_pr_review_dialog_open(true);
    let status_bar_backdrop =
        ElementHandle::find_by_element_id(&window, "MainWindow::pr-review-status-bar-backdrop")
            .next()
            .expect("dialog backdrop over the status bar");
    assert_eq!(status_bar_backdrop.absolute_position().x, 0.0);
    assert_eq!(status_bar_backdrop.absolute_position().y, 572.0);
    assert_eq!(status_bar_backdrop.size().width, 800.0);
    assert_eq!(status_bar_backdrop.size().height, 28.0);

    let dialog_surface = ElementHandle::find_by_element_id(&window, "GitHubDialog::dialog-surface")
        .next()
        .expect("visible PR dialog surface");
    let dialog_bottom = dialog_surface.absolute_position().y + dialog_surface.size().height;
    assert!(
        dialog_bottom <= 556.5,
        "the dialog must retain a 16px bottom safety margin above the 572px status-bar edge; bottom={dialog_bottom}"
    );

    click(window.window(), notification);
    assert_eq!(
        navigations.get(),
        1,
        "the PR Dialog mask must own input over the status bar"
    );
}

#[test]
fn status_bar_popup_stays_above_pr_review_and_owns_its_surface() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    slint::select_bundled_translation("en").expect("English test translations");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(800.0, 600.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_current_route(Route::PrReview);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let network = ElementHandle::find_by_accessible_label(&window, "Network connected")
        .next()
        .expect("network status button");
    let network_position = network.absolute_position();
    let network_size = network.size();
    click(
        window.window(),
        LogicalPosition::new(
            network_position.x + network_size.width / 2.0,
            network_position.y + network_size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let popup = ElementHandle::find_by_element_id(&window, "GitHubActionMenu::menu-surface")
        .next()
        .expect("visible status-bar ActionMenu");
    let position = popup.absolute_position();
    let size = popup.size();
    click(
        window.window(),
        LogicalPosition::new(
            position.x + size.width / 2.0,
            position.y + size.height / 2.0,
        ),
    );
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    assert!(
        ElementHandle::find_by_element_id(&window, "GitHubActionMenu::menu-surface")
            .next()
            .is_some(),
        "the popup surface must receive the click instead of the PR Review layer underneath"
    );
}
