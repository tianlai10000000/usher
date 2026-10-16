use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use usher_lib::{ChatMessage, MainWindow, MessageRole, MessageStatus, Route};

fn message() -> ChatMessage {
    ChatMessage {
        agent_label: "".into(),
        id: 31,
        role: MessageRole::User,
        status: MessageStatus::Normal,
        content: "ANCHOR-31".into(),
        rendered_content: "ANCHOR-31".into(),
        preview_available: false,
        preview_title: "".into(),
        timestamp: "10:31".into(),
        elapsed: "".into(),
    }
}

fn assert_close(actual: f32, expected: f32, context: &str) {
    assert!(
        (actual - expected).abs() < 0.5,
        "{context}: expected {expected}, got {actual}"
    );
}

#[test]
fn chat_history_control_keeps_message_geometry_across_states() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1_000.0, 720.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_current_route(Route::Chat);
    window.set_show_chat_page(true);
    window.set_chat_history_restoring(false);
    window.set_chat_can_load_more_history(true);
    window.set_chat_messages(ModelRc::new(Rc::new(VecModel::from(vec![message()]))));

    let activations = Rc::new(Cell::new(0));
    let observed_activations = activations.clone();
    window.on_chat_load_more_history(move || {
        observed_activations.set(observed_activations.get() + 1)
    });
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);

    let host = ElementHandle::find_by_element_type_name(&window, "GitHubChatHistoryControl")
        .next()
        .expect("history control host");
    let action =
        ElementHandle::find_by_element_id(&window, "GitHubChatHistoryControl::history-action")
            .next()
            .expect("history action");
    let message_list =
        ElementHandle::find_by_element_id(&window, "GitHubChatPage::mounted-message-list")
            .next()
            .expect("mounted message list");

    assert_eq!(action.accessible_role(), Some(AccessibleRole::Button));
    assert_eq!(action.accessible_enabled(), Some(true));
    let ready_label = action
        .accessible_label()
        .map(|label| label.to_string())
        .expect("ready accessible label");
    assert!(!ready_label.is_empty());
    assert_close(host.size().height, 64.0, "history slot height");
    assert_close(action.size().height, 32.0, "history action height");
    assert_close(
        action.absolute_position().y - host.absolute_position().y,
        12.0,
        "history action top inset",
    );
    assert_close(
        message_list.absolute_position().y - action.absolute_position().y - action.size().height,
        20.0,
        "distance from history action to first message",
    );

    let ready_position = action.absolute_position();
    let ready_size = action.size();
    let message_list_y = message_list.absolute_position().y;
    action.invoke_accessible_default_action();
    assert_eq!(activations.get(), 1);

    window.set_chat_loading_history(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(
        action.is_valid(),
        "loading must keep the action node mounted"
    );
    assert_eq!(action.accessible_enabled(), Some(false));
    let loading_label = action
        .accessible_label()
        .map(|label| label.to_string())
        .expect("loading accessible label");
    assert!(!loading_label.is_empty());
    assert_ne!(loading_label, ready_label);
    assert_close(
        action.absolute_position().x,
        ready_position.x,
        "loading action x",
    );
    assert_close(
        action.absolute_position().y,
        ready_position.y,
        "loading action y",
    );
    assert_close(
        action.size().width,
        ready_size.width,
        "loading action width",
    );
    assert_close(
        action.size().height,
        ready_size.height,
        "loading action height",
    );
    assert_close(
        message_list.absolute_position().y,
        message_list_y,
        "loading message position",
    );
    action.invoke_accessible_default_action();
    assert_eq!(
        activations.get(),
        1,
        "loading must reject duplicate activation"
    );

    window.set_chat_loading_history(false);
    window.set_chat_history_load_failed(true);
    i_slint_backend_testing::mock_elapsed_time(Duration::ZERO);
    assert!(action.is_valid(), "error must keep the action node mounted");
    assert_eq!(action.accessible_enabled(), Some(true));
    let error_label = action
        .accessible_label()
        .map(|label| label.to_string())
        .expect("error accessible label");
    assert!(!error_label.is_empty());
    assert_ne!(error_label, loading_label);
    assert_close(
        action.absolute_position().x,
        ready_position.x,
        "error action x",
    );
    assert_close(
        action.absolute_position().y,
        ready_position.y,
        "error action y",
    );
    assert_close(action.size().width, ready_size.width, "error action width");
    assert_close(
        action.size().height,
        ready_size.height,
        "error action height",
    );
    assert_close(
        message_list.absolute_position().y,
        message_list_y,
        "error message position",
    );
    action.invoke_accessible_default_action();
    assert_eq!(activations.get(), 2);
}
