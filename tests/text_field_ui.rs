use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    #[library_path(material) = "material-1.18.0/material.slint"]
    import { GitHubTextField }
        from "ui/components/settings/github_settings_controls.slint";

    export component TextFieldHarness inherits Window {
        width: 220px;
        height: 100px;
        in-out property <string> text;

        GitHubTextField {
            x: 20px;
            y: 10px;
            width: 180px;
            label: "Repository";
            text <=> root.text;
        }
    }
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

#[test]
fn selection_drag_beyond_the_field_edge_keeps_the_endpoint_visible() {
    i_slint_backend_testing::init_no_event_loop();
    let window = TextFieldHarness::new().expect("text field harness");
    window
        .set_text("owner/a-very-long-repository-name-that-extends-beyond-the-visible-input".into());
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(220.0, 100.0),
    });
    window.show().expect("show text field harness");
    settle();

    let field = ElementHandle::find_by_element_id(&window, "GitHubTextField::field")
        .next()
        .expect("production text field");
    let input = ElementHandle::find_by_element_id(&window, "GitHubTextField::field-input")
        .next()
        .expect("production native input");
    let field_position = field.absolute_position();
    let field_right = field_position.x + field.size().width;
    let start = LogicalPosition::new(field_position.x + 14.0, field_position.y + 16.0);

    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: start });
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button: PointerEventButton::Left,
    });
    settle();
    let initial_x = input.absolute_position().x;

    for step in 1..=16 {
        let position = LogicalPosition::new(field_right + step as f32 * 14.0, start.y);
        window
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        settle();
    }

    let scrolled_x = input.absolute_position().x;
    assert!(
        scrolled_x < initial_x - 20.0,
        "dragging beyond the right edge must horizontally scroll the input: initial={initial_x}, scrolled={scrolled_x}"
    );
    assert!(
        input.size().width > field.size().width,
        "the native editor must retain its full content width while the field clips it"
    );

    for step in 1..=16 {
        let position = LogicalPosition::new(field_position.x - step as f32 * 14.0, start.y);
        window
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        settle();
    }
    let restored_x = input.absolute_position().x;
    assert!(
        restored_x > scrolled_x + 20.0,
        "dragging back beyond the left edge must reverse the horizontal scroll: right={scrolled_x}, left={restored_x}"
    );

    let end = LogicalPosition::new(field_position.x - 224.0, start.y);
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: end,
            button: PointerEventButton::Left,
        });
}
