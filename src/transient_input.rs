#[cfg(not(target_os = "macos"))]
use anyhow::{Context, Result};
use i_slint_core::context::set_window_event_hook;
use i_slint_core::input::FocusReason;
use i_slint_core::item_tree::{
    ItemRc, ItemTreeRc, ItemVisitorResult, ParentItemTraversalMode, TraversalOrder, visit_items,
};
use i_slint_core::items::TextInput;
use i_slint_core::lengths::LogicalPoint;
use i_slint_core::window::{PopupWindowLocation, WindowInner};
use slint::ComponentHandle;
use slint::platform::{PointerEventButton, WindowEvent};

use crate::MainWindow;

/// Restore native desktop blur behavior when a scroll viewport consumes a
/// pointer press before a parent FocusScope can take focus.
pub(crate) fn clear_text_input_focus_outside(
    window: &slint::Window,
    position: slint::LogicalPosition,
) {
    let window = WindowInner::from_pub(window);
    let Some(focused) = window.focus_item.borrow().upgrade() else {
        return;
    };
    if focused.downcast::<TextInput>().is_none() {
        return;
    }

    // Shared fields clip their TextInput inside the painted border. Include
    // that immediate clipped parent so clicking field padding preserves focus.
    let field = focused
        .parent_item(ParentItemTraversalMode::StopAtPopups)
        .filter(|parent| parent.borrow().as_ref().clips_children())
        .unwrap_or_else(|| focused.clone());

    let position = LogicalPoint::new(position.x, position.y);
    if field_contains_pointer(&window.component(), &field, position)
        || window.active_popups().iter().any(|popup| {
            let PopupWindowLocation::ChildWindow(origin) = popup.location else {
                return false;
            };
            field_contains_pointer(&popup.component, &field, position - origin.to_vector())
        })
    {
        return;
    }

    window.set_focus_item(&focused, false, FocusReason::PointerClick);
}

/// Return whether the pointer press landed inside the currently focused native
/// text field. This is evaluated after Slint dispatch so a newly focused input
/// is included without every reusable field wiring its own popover callbacks.
pub(crate) fn focused_text_input_contains_pointer(
    window: &slint::Window,
    position: slint::LogicalPosition,
) -> bool {
    let window = WindowInner::from_pub(window);
    let Some(focused) = window.focus_item.borrow().upgrade() else {
        return false;
    };
    if focused.downcast::<TextInput>().is_none() {
        return false;
    }

    let field = focused
        .parent_item(ParentItemTraversalMode::StopAtPopups)
        .filter(|parent| parent.borrow().as_ref().clips_children())
        .unwrap_or_else(|| focused.clone());
    let position = LogicalPoint::new(position.x, position.y);
    field_contains_pointer(&window.component(), &field, position)
        || window.active_popups().iter().any(|popup| {
            let PopupWindowLocation::ChildWindow(origin) = popup.location else {
                return false;
            };
            field_contains_pointer(&popup.component, &field, position - origin.to_vector())
        })
}

fn field_contains_pointer(tree: &ItemTreeRc, field: &ItemRc, position: LogicalPoint) -> bool {
    let mut contains = false;
    // Walk the mounted tree: factory products can have no parent link, so
    // mapping upward from their TextInput omits the ComponentContainer offset.
    // Carry local coordinates through each viewport and skip clipped branches.
    visit_items(
        tree,
        TraversalOrder::FrontToBack,
        |tree, item, index, position| {
            let current = ItemRc::new(tree.clone(), index);
            let geometry = current.geometry();
            if current == *field {
                contains = geometry.contains(*position);
                return ItemVisitorResult::Abort;
            }
            if item.as_ref().clips_children() && !geometry.contains(*position) {
                return ItemVisitorResult::SkipChildren;
            }
            let mut position = *position - geometry.origin.to_vector();
            if let Some(transform) = current.inverse_children_transform() {
                position = transform.transform_point(position.cast()).cast();
            }
            ItemVisitorResult::Continue(position)
        },
        position,
    );
    contains
}

/// Observe already-dispatched main-window pointer events without knowing which
/// reusable surface owns the coordinate. The composition root routes the click
/// after the target has handled links and controls.
/// Production must call this after `Window::show()` has created its adapter.
#[cfg(not(target_os = "macos"))]
pub(crate) fn install(app: &MainWindow) -> Result<()> {
    install_for_window(app).context("Failed to install transient input bridge")
}

#[doc(hidden)]
pub fn install_for_window(app: &MainWindow) -> Result<(), slint::PlatformError> {
    let weak = app.as_weak();
    let previous = set_window_event_hook(None)?;

    set_window_event_hook(Some(Box::new(move |adapter, event, result| {
        if let Some(previous) = previous.as_ref() {
            previous(adapter, event, result);
        }

        let (position, released) = match event {
            WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            } => (position, false),
            WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            } => (position, true),
            _ => return,
        };
        let Some(app) = weak.upgrade() else {
            return;
        };

        // The hook is process-global. Restrict it to the MainWindow adapter;
        // detached Preview windows have their own transient interaction scope.
        if adapter.size() != app.window().size()
            || adapter.position().unwrap_or_default() != app.window().position()
        {
            return;
        }
        if !released {
            clear_text_input_focus_outside(app.window(), *position);
            if focused_text_input_contains_pointer(app.window(), *position) {
                app.invoke_dismiss_transient_popovers();
            }
        }
        if released {
            app.invoke_post_dispatch_pointer_released(position.x, position.y);
        } else {
            app.invoke_post_dispatch_pointer_pressed(position.x, position.y);
        }
    })))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_backend_testing::ElementHandle;

    slint::slint! {
        export component EmbeddedTextFocusTestWindow inherits Window {
            width: 640px;
            height: 480px;
            in property <component-factory> page;
            in property <length> scroll-offset: 0px;
            Rectangle {
                x: 240px;
                y: 46px;
                width: 360px;
                height: 240px;
                clip: true;
                ComponentContainer {
                    y: root.scroll-offset;
                    width: 360px;
                    height: 240px;
                    component-factory: root.page;
                }
            }
        }

        export component TextFocusTestWindow inherits Window {
            width: 360px;
            height: 240px;
            out property <bool> first-focused: first.has-focus;
            out property <bool> second-focused: second.has-focus;

            first-field := Rectangle {
                x: 20px;
                y: 20px;
                width: 180px;
                height: 32px;
                clip: true;

                first := TextInput {
                    x: 12px;
                    y: 4px;
                    width: 156px;
                    height: 24px;
                    text: "first";
                    single-line: true;
                }
            }
            second := TextInput {
                x: 20px;
                y: 84px;
                width: 180px;
                height: 32px;
                text: "second";
                single-line: true;
            }
        }

        export component NativeTextEscapeTestWindow inherits Window {
            width: 360px;
            height: 240px;
            out property <int> edit-count: 0;
            out property <int> context-escape-count: 0;

            FocusScope {
                width: 100%;
                height: 100%;
                key-pressed(event) => {
                    if event.text == Key.Escape {
                        root.context-escape-count += 1;
                        return accept;
                    }
                    return reject;
                }

                single-line-input := TextInput {
                    x: 20px;
                    y: 20px;
                    width: 280px;
                    height: 32px;
                    text: "single draft";
                    single-line: true;
                    edited => { root.edit-count += 1; }
                }

                multiline-input := TextInput {
                    x: 20px;
                    y: 70px;
                    width: 280px;
                    height: 72px;
                    text: "first line\nsecond line";
                    single-line: false;
                    wrap: word-wrap;
                    line-height-factor: 1;
                    edited => { root.edit-count += 1; }
                }

                password-input := TextInput {
                    x: 20px;
                    y: 164px;
                    width: 280px;
                    height: 32px;
                    text: "secret value";
                    single-line: true;
                    input-type: InputType.password;
                    edited => { root.edit-count += 1; }
                }
            }
        }
    }

    fn input(app: &TextFocusTestWindow, id: &str) -> ElementHandle {
        ElementHandle::find_by_element_id(app, id)
            .next()
            .expect("text input")
    }

    fn press_key(window: &slint::Window, text: impl Into<slint::SharedString>) {
        let text = text.into();
        window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        window.dispatch_event(WindowEvent::KeyReleased { text });
    }

    fn assert_escape_preserves_native_input(
        app: &NativeTextEscapeTestWindow,
        window: &slint::Window,
        focused: &ItemRc,
    ) {
        let input = focused
            .downcast::<TextInput>()
            .expect("native TextInput focus");
        let input = input.as_pin_ref();
        // Keep a real selection so Escape must preserve both its anchor and
        // caret, rather than merely leaving the visible text unchanged.
        let text = input.text();
        let selection_end = text.as_str().find('\n').map_or(5, |newline| newline + 3);
        input.anchor_position_byte_offset.set(0);
        input.cursor_position_byte_offset.set(selection_end as i32);
        let edits = app.get_edit_count();
        let context_escapes = app.get_context_escape_count();

        press_key(window, slint::platform::Key::Escape);

        assert_eq!(input.text(), text, "Escape preserves the current draft");
        assert!(input.has_focus(), "Escape leaves the text input focused");
        assert_eq!(input.anchor_position_byte_offset(), 0);
        assert_eq!(input.cursor_position_byte_offset(), selection_end as i32);
        assert_eq!(app.get_edit_count(), edits, "Escape is not an edit");
        assert_eq!(
            app.get_context_escape_count(),
            context_escapes + 1,
            "the surrounding context still receives Escape"
        );
        assert_eq!(
            WindowInner::from_pub(window)
                .focus_item
                .borrow()
                .upgrade()
                .expect("focus survives Escape"),
            *focused
        );

        press_key(window, "Z");
        assert_eq!(
            input.text().as_str(),
            format!("Z{}", &text.as_str()[selection_end..])
        );
        assert_eq!(app.get_edit_count(), edits + 1);
        assert!(input.has_focus(), "typing continues in the same field");
    }

    fn assert_native_field_escape(id: &str) {
        i_slint_backend_testing::init_no_event_loop();
        let app = NativeTextEscapeTestWindow::new().expect("test window");
        ElementHandle::find_by_element_id(&app, id)
            .next()
            .expect("text input")
            .mock_single_click(PointerEventButton::Left);
        let focused = WindowInner::from_pub(app.window())
            .focus_item
            .borrow()
            .upgrade()
            .expect("focused native input");
        assert_escape_preserves_native_input(&app, app.window(), &focused);
    }

    #[test]
    fn escape_preserves_single_line_input_and_reaches_its_context() {
        assert_native_field_escape("NativeTextEscapeTestWindow::single-line-input");
    }

    #[test]
    fn escape_preserves_multiline_input_and_reaches_its_context() {
        assert_native_field_escape("NativeTextEscapeTestWindow::multiline-input");
    }

    #[test]
    fn escape_preserves_password_input_and_reaches_its_context() {
        assert_native_field_escape("NativeTextEscapeTestWindow::password-input");
    }

    #[test]
    fn escape_preserves_input_in_an_independent_factory_product() {
        i_slint_backend_testing::init_no_event_loop();
        let app = EmbeddedTextFocusTestWindow::new().expect("host window");
        let page = NativeTextEscapeTestWindow::new().expect("independent factory product");
        let page_weak = page.as_weak();
        app.set_page(slint::ComponentFactory::new(move |_| page_weak.upgrade()));
        let window = WindowInner::from_pub(app.window());
        i_slint_core::item_tree::ensure_item_tree_instantiated(&window.component());

        ElementHandle::find_by_element_id(&page, "NativeTextEscapeTestWindow::single-line-input")
            .next()
            .expect("embedded text input")
            .mock_single_click(PointerEventButton::Left);
        let focused = WindowInner::from_pub(page.window())
            .focus_item
            .borrow()
            .upgrade()
            .expect("focused embedded input");
        window.set_focus_item(&focused, true, FocusReason::PointerClick);

        assert_escape_preserves_native_input(&page, app.window(), &focused);
    }

    #[test]
    fn outside_click_clears_text_input_focus() {
        i_slint_backend_testing::init_no_event_loop();
        let app = TextFocusTestWindow::new().expect("test window");

        input(&app, "TextFocusTestWindow::first").mock_single_click(PointerEventButton::Left);
        assert!(app.get_first_focused());

        clear_text_input_focus_outside(app.window(), slint::LogicalPosition::new(300.0, 180.0));
        assert!(!app.get_first_focused());
        assert!(!app.get_second_focused());
    }

    #[test]
    fn clicking_another_text_input_keeps_the_new_focus() {
        i_slint_backend_testing::init_no_event_loop();
        let app = TextFocusTestWindow::new().expect("test window");

        input(&app, "TextFocusTestWindow::first").mock_single_click(PointerEventButton::Left);
        assert!(app.get_first_focused());

        let second = input(&app, "TextFocusTestWindow::second");
        second.mock_single_click(PointerEventButton::Left);
        let second_position = second.absolute_position();
        clear_text_input_focus_outside(
            app.window(),
            slint::LogicalPosition::new(second_position.x + 1.0, second_position.y + 1.0),
        );
        assert!(!app.get_first_focused());
        assert!(app.get_second_focused());
    }

    #[test]
    fn clicking_clipped_field_padding_preserves_focus() {
        i_slint_backend_testing::init_no_event_loop();
        let app = TextFocusTestWindow::new().expect("test window");

        input(&app, "TextFocusTestWindow::first").mock_single_click(PointerEventButton::Left);
        assert!(app.get_first_focused());

        clear_text_input_focus_outside(app.window(), slint::LogicalPosition::new(21.0, 21.0));
        assert!(app.get_first_focused());
    }

    #[test]
    fn focused_input_hit_is_distinguished_from_outside_content() {
        i_slint_backend_testing::init_no_event_loop();
        let app = TextFocusTestWindow::new().expect("test window");

        input(&app, "TextFocusTestWindow::first").mock_single_click(PointerEventButton::Left);
        assert!(focused_text_input_contains_pointer(
            app.window(),
            slint::LogicalPosition::new(40.0, 36.0),
        ));
        assert!(!focused_text_input_contains_pointer(
            app.window(),
            slint::LogicalPosition::new(300.0, 180.0),
        ));
    }

    #[test]
    fn embedded_field_uses_host_offset_and_viewport_clip() {
        i_slint_backend_testing::init_no_event_loop();
        let app = EmbeddedTextFocusTestWindow::new().expect("host window");
        let page = TextFocusTestWindow::new().expect("independent factory product");
        let page_weak = page.as_weak();
        app.set_page(slint::ComponentFactory::new(move |_| page_weak.upgrade()));
        let window = WindowInner::from_pub(app.window());
        i_slint_core::item_tree::ensure_item_tree_instantiated(&window.component());

        input(&page, "TextFocusTestWindow::first").mock_single_click(PointerEventButton::Left);
        let focused = WindowInner::from_pub(page.window())
            .focus_item
            .borrow()
            .upgrade()
            .expect("focused embedded input");
        window.set_focus_item(&focused, true, FocusReason::PointerClick);

        clear_text_input_focus_outside(app.window(), slint::LogicalPosition::new(261.0, 67.0));
        assert!(
            page.get_first_focused(),
            "host-offset field padding keeps focus"
        );

        app.set_scroll_offset(-30.0);
        clear_text_input_focus_outside(app.window(), slint::LogicalPosition::new(270.0, 50.0));
        assert!(
            page.get_first_focused(),
            "visible scrolled field keeps focus"
        );

        clear_text_input_focus_outside(app.window(), slint::LogicalPosition::new(270.0, 40.0));
        assert!(
            !page.get_first_focused(),
            "clipped part above the viewport loses focus"
        );
    }
}
