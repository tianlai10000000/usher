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
    }

    fn input(app: &TextFocusTestWindow, id: &str) -> ElementHandle {
        ElementHandle::find_by_element_id(app, id)
            .next()
            .expect("text input")
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
