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

mod observers;
pub(crate) use observers::register_pointer_root;

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
#[cfg(any(test, feature = "ui-components"))]
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
pub(crate) fn install(app: &MainWindow) -> Result<()> {
    install_with_pointer_forwarding(app, !cfg!(target_os = "macos"))
        .context("Failed to install transient input bridge")
}

#[doc(hidden)]
pub fn install_for_window(app: &MainWindow) -> Result<(), slint::PlatformError> {
    install_with_pointer_forwarding(app, true)
}

fn install_with_pointer_forwarding(
    app: &MainWindow,
    forward_pointer: bool,
) -> Result<(), slint::PlatformError> {
    let weak = app.as_weak();
    let previous = set_window_event_hook(None)?;

    set_window_event_hook(Some(Box::new(move |adapter, event, result| {
        if let Some(previous) = previous.as_ref() {
            previous(adapter, event, result);
        }

        let Some(app) = weak.upgrade() else {
            return;
        };

        // The hook is process-global. Restrict it to the MainWindow adapter;
        // detached Preview windows have their own transient interaction scope.
        if !std::rc::Rc::ptr_eq(
            adapter,
            &WindowInner::from_pub(app.window()).window_adapter(),
        ) {
            return;
        }

        let (position, released) = match event {
            WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            } => {
                clear_text_input_focus_outside(app.window(), *position);
                // Every real left press is published once after target dispatch,
                // even when a Flickable delays its child press. Shared popovers
                // classify bounds without depending on TextInput focus timing.
                observers::publish_pointer_press(&app, position.x, position.y);
                (position, false)
            }
            WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            } => (position, true),
            _ => return,
        };
        // AppKit owns native chrome and release forwarding on macOS. Input
        // blur and popover press observation belong to this hook on all platforms.
        if forward_pointer {
            if released {
                app.invoke_post_dispatch_pointer_released(position.x, position.y);
            } else {
                app.invoke_post_dispatch_pointer_pressed(position.x, position.y);
            }
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
            in property <bool> delay-press: false;
            out property <bool> first-focused: first.has-focus;
            out property <bool> second-focused: second.has-focus;

            Flickable {
                width: 100%; height: 100%;
                content-width: self.width;
                content-height: root.delay-press ? self.height * 2 : self.height;
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

    struct HookFixture {
        host: MainWindow,
        page: TextFocusTestWindow,
        samples: std::rc::Rc<std::cell::Cell<usize>>,
        presses: std::rc::Rc<std::cell::Cell<usize>>,
        releases: std::rc::Rc<std::cell::Cell<usize>>,
    }

    fn hook_fixture(forward_pointer: bool) -> HookFixture {
        i_slint_backend_testing::init_no_event_loop();
        let host = MainWindow::new().unwrap();
        host.set_platform_is_macos(false);
        host.set_show_navigation_drawer(false);
        host.set_show_breadcrumb(false);
        host.window()
            .set_size(slint::LogicalSize::new(640.0, 480.0));
        let page = TextFocusTestWindow::new().unwrap();
        let weak = page.as_weak();
        host.set_settings_page_factory(slint::ComponentFactory::new(move |_| weak.upgrade()));
        host.set_current_route(crate::Route::Settings);
        WindowInner::from_pub(host.window()).ensure_tree_instantiated();
        let samples = std::rc::Rc::new(std::cell::Cell::new(0));
        let presses = std::rc::Rc::new(std::cell::Cell::new(0));
        let releases = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = samples.clone();
        host.global::<crate::TransientOverlayState>()
            .on_pointer_pressed(move |_, _| count.set(count.get() + 1));
        let count = presses.clone();
        host.on_post_dispatch_pointer_pressed(move |_, _| count.set(count.get() + 1));
        let count = releases.clone();
        host.on_post_dispatch_pointer_released(move |_, _| count.set(count.get() + 1));
        install_with_pointer_forwarding(&host, forward_pointer).unwrap();
        HookFixture {
            host,
            page,
            samples,
            presses,
            releases,
        }
    }

    fn pointer(
        window: &slint::Window,
        position: slint::LogicalPosition,
        button: PointerEventButton,
        pressed: bool,
    ) {
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(if pressed {
            WindowEvent::PointerPressed { position, button }
        } else {
            WindowEvent::PointerReleased { position, button }
        });
    }

    #[test]
    fn post_dispatch_hook_ignores_a_detached_window_with_identical_geometry() {
        let fixture = hook_fixture(true);
        let inside = slint::LogicalPosition::new(40.0, 36.0);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            true,
        );
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            false,
        );
        assert!(fixture.page.get_first_focused());
        assert_eq!(
            fixture.samples.get(),
            1,
            "real host press is the positive control"
        );
        let detached = TextFocusTestWindow::new().unwrap();
        detached.window().set_size(fixture.host.window().size());
        detached
            .window()
            .set_position(fixture.host.window().position());
        assert_eq!(detached.window().size(), fixture.host.window().size());
        assert_eq!(
            detached.window().position(),
            fixture.host.window().position()
        );
        let outside = slint::LogicalPosition::new(300.0, 180.0);
        pointer(detached.window(), outside, PointerEventButton::Left, true);
        pointer(detached.window(), outside, PointerEventButton::Left, false);
        assert!(
            fixture.page.get_first_focused(),
            "another adapter must not blur the host"
        );
        assert_eq!(fixture.samples.get(), 1);
        assert_eq!(fixture.presses.get(), 1);
        assert_eq!(fixture.releases.get(), 1);
    }

    #[test]
    fn post_dispatch_hook_publishes_the_press_without_reclassifying_an_outside_release() {
        let fixture = hook_fixture(true);
        pointer(
            fixture.host.window(),
            slint::LogicalPosition::new(40.0, 36.0),
            PointerEventButton::Left,
            true,
        );
        assert!(fixture.page.get_first_focused());
        assert_eq!(fixture.samples.get(), 1, "the real press is published once");
        pointer(
            fixture.host.window(),
            slint::LogicalPosition::new(300.0, 180.0),
            PointerEventButton::Left,
            false,
        );
        assert!(
            fixture.page.get_first_focused(),
            "selection dragged outside keeps input focus"
        );
        assert_eq!(fixture.samples.get(), 1);
        assert_eq!(fixture.presses.get(), 1);
        assert_eq!(fixture.releases.get(), 1);
    }

    #[test]
    fn post_dispatch_hook_ignores_right_and_middle_button_phases() {
        let fixture = hook_fixture(true);
        pointer(
            fixture.host.window(),
            slint::LogicalPosition::new(40.0, 36.0),
            PointerEventButton::Left,
            true,
        );
        pointer(
            fixture.host.window(),
            slint::LogicalPosition::new(40.0, 36.0),
            PointerEventButton::Left,
            false,
        );
        assert!(fixture.page.get_first_focused());
        assert_eq!(
            fixture.samples.get(),
            1,
            "left press is the positive control"
        );
        assert_eq!(fixture.presses.get(), 1);
        assert_eq!(fixture.releases.get(), 1);
        fixture.samples.set(0);
        fixture.presses.set(0);
        fixture.releases.set(0);
        for button in [PointerEventButton::Right, PointerEventButton::Middle] {
            let outside = slint::LogicalPosition::new(300.0, 180.0);
            pointer(fixture.host.window(), outside, button, true);
            pointer(fixture.host.window(), outside, button, false);
        }
        assert_eq!(fixture.samples.get(), 0);
        assert_eq!(fixture.presses.get(), 0);
        assert_eq!(fixture.releases.get(), 0);
    }

    #[test]
    fn native_forwarding_mode_publishes_press_without_duplicate_pointer_phases() {
        let fixture = hook_fixture(false);
        let inside = slint::LogicalPosition::new(40.0, 36.0);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            true,
        );
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            false,
        );
        assert!(fixture.page.get_first_focused());
        assert_eq!(fixture.samples.get(), 1);
        assert_eq!(fixture.presses.get(), 0);
        assert_eq!(
            fixture.releases.get(),
            0,
            "AppKit owns the only production release delivery"
        );
    }

    #[test]
    fn flickable_short_click_publishes_before_its_delayed_child_press() {
        let fixture = hook_fixture(true);
        fixture.page.set_delay_press(true);
        let inside = slint::LogicalPosition::new(40.0, 36.0);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            true,
        );
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
        assert!(
            !fixture.page.get_first_focused(),
            "Flickable still holds the child press"
        );
        assert_eq!(fixture.samples.get(), 1);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            false,
        );
        assert!(fixture.page.get_first_focused());
        assert_eq!(
            fixture.samples.get(),
            1,
            "delayed child dispatch does not publish the press again"
        );
        assert_eq!(fixture.presses.get(), 1);
        assert_eq!(fixture.releases.get(), 1);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            false,
        );
        assert_eq!(
            fixture.samples.get(),
            1,
            "an unmatched release cannot publish another press"
        );
    }

    #[test]
    fn flickable_held_press_publishes_once_and_preserves_outside_selection_drag() {
        let fixture = hook_fixture(false);
        fixture.page.set_delay_press(true);
        let inside = slint::LogicalPosition::new(40.0, 36.0);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            true,
        );
        assert!(!fixture.page.get_first_focused());
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        assert!(
            fixture.page.get_first_focused(),
            "Flickable's timer delivered the child press"
        );
        assert_eq!(fixture.samples.get(), 1);
        let focused = WindowInner::from_pub(fixture.host.window())
            .focus_item
            .borrow()
            .upgrade()
            .expect("held native text input");
        let input = focused.downcast::<TextInput>().expect("native TextInput");
        let input = input.as_pin_ref();
        // Drag horizontally so this vertical Flickable keeps forwarding the
        // gesture to the editor after its delayed press has been delivered.
        let outside = slint::LogicalPosition::new(300.0, inside.y);
        fixture
            .host
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position: outside });
        let selection = (
            input.anchor_position_byte_offset(),
            input.cursor_position_byte_offset(),
        );
        assert_ne!(
            selection.0, selection.1,
            "the real drag must create a nonempty selection"
        );
        assert!(!focused_text_input_contains_pointer(
            fixture.host.window(),
            outside
        ));
        pointer(
            fixture.host.window(),
            outside,
            PointerEventButton::Left,
            false,
        );
        assert!(
            input.has_focus(),
            "release outside the field preserves editor focus"
        );
        assert_eq!(
            (
                input.anchor_position_byte_offset(),
                input.cursor_position_byte_offset()
            ),
            selection
        );
        assert_eq!(fixture.samples.get(), 1);
        assert_eq!(fixture.presses.get(), 0);
        assert_eq!(fixture.releases.get(), 0);
    }

    fn assert_release_does_not_publish_after(event: WindowEvent) {
        let fixture = hook_fixture(false);
        fixture.page.set_delay_press(true);
        let inside = slint::LogicalPosition::new(40.0, 36.0);
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            true,
        );
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(150));
        let window = WindowInner::from_pub(fixture.host.window());
        let focused = window
            .focus_item
            .borrow()
            .upgrade()
            .expect("delayed input focus");
        fixture.host.window().dispatch_event(event);
        fixture
            .host
            .window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
        // A later programmatic focus must not create a second press sample.
        window.set_focus_item(&focused, false, FocusReason::Programmatic);
        window.set_focus_item(&focused, true, FocusReason::Programmatic);
        assert!(fixture.page.get_first_focused());
        pointer(
            fixture.host.window(),
            inside,
            PointerEventButton::Left,
            false,
        );
        assert_eq!(
            fixture.samples.get(),
            1,
            "release cannot dismiss a later surface"
        );
    }

    #[test]
    fn pointer_exit_then_release_does_not_publish_another_press() {
        assert_release_does_not_publish_after(WindowEvent::PointerExited);
    }

    #[test]
    fn window_deactivation_then_release_does_not_publish_another_press() {
        assert_release_does_not_publish_after(WindowEvent::WindowActiveChanged(false));
    }
}
