use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use i_slint_backend_testing::testing_backend::TestingWindow;
use i_slint_core::cursor::MouseCursorInner;
use i_slint_core::items::BuiltInMouseCursor;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};
use usher_lib::{MainWindow, Route};

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    import { GitHubResizeHandle } from "ui/components/github_resize_handle.slint";
    import { AppState } from "ui/state/app_state.slint";
    export { AppState }

    export component ResizeHandleHarness inherits Window {
        width: 400px;
        height: 400px;
        in property <bool> horizontal: false;
        in property <bool> leading-edge: false;
        in property <length> line-inset: 0px;
        in property <bool> resize-enabled: true;
        in-out property <length> boundary: 150px;
        private property <length> original-boundary;
        in-out property <int> starts: 0;
        in-out property <int> finishes: 0;
        in-out property <bool> cancelled: false;
        out property <bool> dragging: handle.dragging;
        out property <bool> hovered: handle.hovered;

        // A neighboring surface must not mask either side of the resize strip.
        TouchArea { mouse-cursor: default; }
        handle := GitHubResizeHandle {
            x: root.horizontal ? 30px : root.boundary - 4px;
            y: root.horizontal ? root.boundary - 4px : 30px;
            width: root.horizontal ? 180px : 9px;
            height: root.horizontal ? 9px : 180px;
            z: 10;
            horizontal: root.horizontal;
            leading-edge: root.leading-edge;
            line-inset: root.line-inset;
            resize-enabled: root.resize-enabled;
            drag-started => {
                root.starts += 1;
                root.original-boundary = root.boundary;
            }
            drag-delta(delta) => {
                // Production splitters move with the pointer; apply only the
                // unconsumed local delta, rather than a press-time total.
                root.boundary += delta;
            }
            drag-finished(cancelled) => {
                root.finishes += 1;
                root.cancelled = cancelled;
                if cancelled { root.boundary = root.original-boundary; }
            }
        }
    }
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

fn cursor(window: &impl ComponentHandle) -> MouseCursorInner {
    let adapter = i_slint_core::window::WindowInner::from_pub(window.window()).window_adapter();
    let testing = adapter
        .internal(i_slint_core::InternalToken)
        .and_then(|adapter| (adapter as &dyn std::any::Any).downcast_ref::<TestingWindow>())
        .expect("testing window adapter");
    testing.mouse_cursor()
}

fn point(window: &ResizeHandleHarness, offset: f32) -> LogicalPosition {
    if window.get_horizontal() {
        LogicalPosition::new(100.0, window.get_boundary() + offset)
    } else {
        LogicalPosition::new(window.get_boundary() + offset, 100.0)
    }
}

fn move_pointer(window: &impl ComponentHandle, position: LogicalPosition) {
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    settle();
}

fn expected_cursor(horizontal: bool) -> MouseCursorInner {
    MouseCursorInner::BuiltIn(if horizontal {
        BuiltInMouseCursor::NsResize
    } else {
        BuiltInMouseCursor::EwResize
    })
}

#[test]
fn both_axes_show_the_resize_cursor_across_the_complete_hit_strip() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResizeHandleHarness::new().expect("resize handle harness");
    window.show().unwrap();
    for horizontal in [false, true] {
        window.set_horizontal(horizontal);
        for locale in ["en", "zh-CN"] {
            slint::select_bundled_translation(locale).unwrap();
            for dark in [false, true] {
                window.global::<AppState>().set_effective_dark_mode(dark);
                for offset in [-3.0, 0.0, 3.0] {
                    move_pointer(&window, point(&window, offset));
                    assert!(
                        window.get_hovered(),
                        "hover strip: horizontal={horizontal}, locale={locale}, dark={dark}, offset={offset}"
                    );
                    assert_eq!(cursor(&window), expected_cursor(horizontal));
                    assert!(!window.get_dragging());
                    move_pointer(&window, LogicalPosition::new(300.0, 300.0));
                    assert!(!window.get_hovered());
                    assert_eq!(
                        cursor(&window),
                        MouseCursorInner::BuiltIn(BuiltInMouseCursor::Default)
                    );
                }
            }
        }
    }
    assert_eq!(window.get_starts(), 0);
    assert_eq!(window.get_finishes(), 0);
}

#[test]
fn first_press_captures_one_bidirectional_drag_while_the_strip_moves() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResizeHandleHarness::new().unwrap();
    window.show().unwrap();
    for horizontal in [false, true] {
        window.set_horizontal(horizontal);
        for offset in [-3.0, 0.0, 3.0] {
            window.set_boundary(150.0);
            let initial_starts = window.get_starts();
            let initial_finishes = window.get_finishes();
            let start = point(&window, offset);
            move_pointer(&window, start);
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position: start,
                button: PointerEventButton::Left,
            });
            assert!(window.get_dragging());
            assert_eq!(window.get_starts(), initial_starts + 1);
            let mut position = start;
            for delta in [20.0, 50.0, 30.0, -10.0] {
                position = LogicalPosition::new(
                    start.x + if horizontal { 0.0 } else { delta },
                    start.y + if horizontal { delta } else { 0.0 },
                );
                move_pointer(&window, position);
                assert!((window.get_boundary() - 150.0 - delta).abs() < 0.5);
                assert!(window.get_dragging());
                assert_eq!(cursor(&window), expected_cursor(horizontal));
            }
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
            assert!(!window.get_dragging());
            assert_eq!(window.get_finishes(), initial_finishes + 1);
            assert!(!window.get_cancelled());
            assert!((window.get_boundary() - 140.0).abs() < 0.5);
        }
    }
}

#[test]
fn pointer_cancel_restores_the_consumers_size_once() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResizeHandleHarness::new().unwrap();
    window.show().unwrap();
    for horizontal in [false, true] {
        window.set_horizontal(horizontal);
        window.set_boundary(150.0);
        let initial_finishes = window.get_finishes();
        let start = point(&window, 0.0);
        move_pointer(&window, start);
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position: start,
            button: PointerEventButton::Left,
        });
        let end = LogicalPosition::new(
            start.x + if horizontal { 0.0 } else { 30.0 },
            start.y + if horizontal { 30.0 } else { 0.0 },
        );
        move_pointer(&window, end);
        assert!((window.get_boundary() - 180.0).abs() < 0.5);
        window.window().dispatch_event(WindowEvent::PointerExited);
        assert!(!window.get_dragging());
        assert!(window.get_cancelled());
        assert_eq!(window.get_finishes(), initial_finishes + 1);
        assert!((window.get_boundary() - 150.0).abs() < 0.5);
        window.window().dispatch_event(WindowEvent::PointerExited);
        assert_eq!(window.get_finishes(), initial_finishes + 1);
    }
}

#[test]
fn disabled_and_non_primary_presses_never_start_resizing() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResizeHandleHarness::new().unwrap();
    window.show().unwrap();
    for horizontal in [false, true] {
        window.set_horizontal(horizontal);
        window.set_resize_enabled(false);
        let position = point(&window, 0.0);
        move_pointer(&window, position);
        assert!(!window.get_hovered());
        assert_eq!(
            cursor(&window),
            MouseCursorInner::BuiltIn(BuiltInMouseCursor::Default)
        );
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        window.set_resize_enabled(true);
        move_pointer(&window, position);
        assert!(window.get_hovered());
        for button in [PointerEventButton::Right, PointerEventButton::Middle] {
            window
                .window()
                .dispatch_event(WindowEvent::PointerPressed { position, button });
            assert!(!window.get_dragging());
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased { position, button });
        }
    }
    assert_eq!(window.get_starts(), 0);
    assert_eq!(window.get_finishes(), 0);
    assert_eq!(window.get_boundary(), 150.0);
}

#[test]
fn disabling_an_active_handle_cancels_before_it_can_change_more_geometry() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResizeHandleHarness::new().unwrap();
    window.show().unwrap();
    for horizontal in [false, true] {
        window.set_horizontal(horizontal);
        window.set_resize_enabled(true);
        let initial_finishes = window.get_finishes();
        let start = point(&window, 0.0);
        move_pointer(&window, start);
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position: start,
            button: PointerEventButton::Left,
        });
        let end = LogicalPosition::new(
            start.x + if horizontal { 0.0 } else { 30.0 },
            start.y + if horizontal { 30.0 } else { 0.0 },
        );
        move_pointer(&window, end);
        assert!((window.get_boundary() - 180.0).abs() < 0.5);
        window.set_resize_enabled(false);
        settle();
        assert!(!window.get_dragging());
        assert!(!window.get_hovered());
        assert!(window.get_cancelled());
        assert_eq!(window.get_finishes(), initial_finishes + 1);
        assert_eq!(window.get_boundary(), 150.0);
        move_pointer(&window, end);
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: start,
                button: PointerEventButton::Left,
            });
        assert_eq!(window.get_finishes(), initial_finishes + 1);
        assert_eq!(window.get_boundary(), 150.0);
    }
}

fn find(window: &MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing production UI element {id}"))
}

fn exercise_horizontal_boundary(
    window: &MainWindow,
    handle_id: &str,
    surface_id: &str,
    x_offset: f32,
    terminal: bool,
) {
    let handle = find(window, handle_id);
    let surface = find(window, surface_id);
    let initial_height = surface.size().height;
    let initial_bottom = surface.absolute_position().y + initial_height;
    let initial_terminal_height = window.get_shell_drawer_height();
    let start = LogicalPosition::new(
        handle.absolute_position().x + x_offset,
        handle.absolute_position().y + handle.size().height / 2.0,
    );
    move_pointer(window, start);
    assert_eq!(
        cursor(window),
        expected_cursor(true),
        "{handle_id}, x={x_offset}"
    );
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button: PointerEventButton::Left,
    });
    for delta in [-20.0, 8.0, -8.0, 0.0] {
        move_pointer(window, LogicalPosition::new(start.x, start.y + delta));
        assert!(
            (surface.size().height - initial_height + delta).abs() < 0.5,
            "first held drag must resize {surface_id} at x={x_offset}: initial={initial_height}, delta={delta}, actual={}",
            surface.size().height,
        );
        assert!(
            (surface.absolute_position().y + surface.size().height - initial_bottom).abs() < 0.5,
            "{surface_id} must remain bottom-pinned"
        );
        assert_eq!(cursor(window), expected_cursor(true));
        if terminal {
            assert!(
                window.get_shell_drawer_visible(),
                "drag must not close Terminal"
            );
            assert!(
                !window.get_shell_drawer_fullscreen(),
                "drag must not maximize Terminal"
            );
        }
    }
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: start,
            button: PointerEventButton::Left,
        });
    settle();
    assert!((surface.size().height - initial_height).abs() < 0.5);

    // A later cancelled gesture must restore the committed consumer size.
    move_pointer(window, start);
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: start,
        button: PointerEventButton::Left,
    });
    move_pointer(window, LogicalPosition::new(start.x, start.y - 24.0));
    assert!((surface.size().height - initial_height - 24.0).abs() < 0.5);
    window.window().dispatch_event(WindowEvent::PointerExited);
    settle();
    assert!((surface.size().height - initial_height).abs() < 0.5);
    if terminal {
        assert_eq!(window.get_shell_drawer_height(), initial_terminal_height);
        assert!(window.get_shell_drawer_visible());
        assert!(!window.get_shell_drawer_fullscreen());
    }
}

#[test]
fn chat_composer_full_width_hover_and_first_drag_follow_the_shared_contract() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_current_route(Route::Chat);
    window.set_show_chat_page(true);
    window.show().unwrap();
    for width in [800.0, 1280.0] {
        window.window().dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width, 800.0),
        });
        for locale in ["en", "zh-CN"] {
            slint::select_bundled_translation(locale).unwrap();
            for dark in [false, true] {
                window
                    .global::<usher_lib::AppState>()
                    .set_effective_dark_mode(dark);
                settle();
                let handle = find(&window, "GitHubChatPage::resize-handle");
                let composer = find(&window, "GitHubChatPage::composer");
                assert!((handle.size().width - composer.size().width).abs() < 0.5);
                assert!((handle.size().height - 9.0).abs() < 0.5);
                for fraction in [0.1, 0.5, 0.9] {
                    exercise_horizontal_boundary(
                        &window,
                        "GitHubChatPage::resize-handle",
                        "GitHubChatPage::composer",
                        handle.size().width * fraction,
                        false,
                    );
                }
            }
        }
    }
}

#[test]
fn terminal_full_width_resize_strip_wins_over_the_header_action_buttons() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_show_breadcrumb(false);
    window.set_current_route(Route::Project);
    window.set_shell_drawer_visible(true);
    window.set_shell_drawer_height(280.0);
    window.show().unwrap();
    for width in [800.0, 1280.0] {
        window.window().dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width, 800.0),
        });
        for locale in ["en", "zh-CN"] {
            slint::select_bundled_translation(locale).unwrap();
            for dark in [false, true] {
                window
                    .global::<usher_lib::AppState>()
                    .set_effective_dark_mode(dark);
                settle();
                let handle = find(&window, "GitHubTerminalDrawer::resize-handle");
                let drawer = find(&window, "GitHubTerminalDrawer::drawer");
                assert!((handle.size().width - drawer.size().width).abs() < 0.5);
                assert!((handle.size().height - 9.0).abs() < 0.5);
                for x in [
                    handle.size().width * 0.1,
                    handle.size().width * 0.5,
                    handle.size().width * 0.9,
                    handle.size().width - 56.0,
                    handle.size().width - 20.0,
                ] {
                    exercise_horizontal_boundary(
                        &window,
                        "GitHubTerminalDrawer::resize-handle",
                        "GitHubTerminalDrawer::drawer",
                        x,
                        true,
                    );
                }
            }
        }
    }
}

#[test]
fn one_sided_hosts_align_the_painted_divider_without_narrowing_the_hit_band() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ResizeHandleHarness::new().unwrap();
    window.show().unwrap();
    for horizontal in [false, true] {
        window.set_horizontal(horizontal);
        for leading_edge in [false, true] {
            window.set_leading_edge(leading_edge);
            for inset in [0.0, 6.0] {
                window.set_line_inset(inset);
                move_pointer(&window, LogicalPosition::new(300.0, 300.0));
                let line =
                    ElementHandle::find_by_element_id(&window, "GitHubResizeHandle::divider-line")
                        .next()
                        .expect("painted divider");
                let handle =
                    ElementHandle::find_by_element_id(&window, "ResizeHandleHarness::handle")
                        .next()
                        .expect("mounted hit band");
                let position = line.absolute_position();
                let origin = handle.absolute_position();
                let offset = if horizontal {
                    position.y - origin.y
                } else {
                    position.x - origin.x
                };
                assert!((offset - if leading_edge { 0.0 } else { 4.0 }).abs() < 0.5);
                let thickness = || {
                    if horizontal {
                        line.size().height
                    } else {
                        line.size().width
                    }
                };
                let band = || {
                    if horizontal {
                        handle.size().height
                    } else {
                        handle.size().width
                    }
                };
                let painted_span = if horizontal {
                    line.size().width
                } else {
                    line.size().height
                };
                let paint_start = if horizontal {
                    position.x - origin.x
                } else {
                    position.y - origin.y
                };
                assert!(
                    (thickness() - 1.0).abs() < 0.5,
                    "idle divider remains a hairline"
                );
                assert!((band() - 9.0).abs() < 0.5);
                assert!((paint_start - inset).abs() < 0.5);
                assert!((painted_span - (180.0 - 2.0 * inset)).abs() < 0.5);
                for offset in [-3.0, 0.0, 3.0] {
                    move_pointer(&window, point(&window, offset));
                    assert!(window.get_hovered());
                    assert_eq!(cursor(&window), expected_cursor(horizontal));
                    assert!((thickness() - 2.0).abs() < 0.5, "hover stroke is 2px");
                }
                // Rounded editors may shorten paint only: their unpainted corner
                // spans still need the full hover and first-press interaction.
                for long_offset in [1.0, 179.0] {
                    let position = LogicalPosition::new(
                        if horizontal {
                            origin.x + long_offset
                        } else {
                            origin.x + 4.5
                        },
                        if horizontal {
                            origin.y + 4.5
                        } else {
                            origin.y + long_offset
                        },
                    );
                    move_pointer(&window, position);
                    assert!(window.get_hovered());
                    assert_eq!(cursor(&window), expected_cursor(horizontal));
                    window.window().dispatch_event(WindowEvent::PointerPressed {
                        position,
                        button: PointerEventButton::Left,
                    });
                    assert!(window.get_dragging());
                    let outside = LogicalPosition::new(
                        if horizontal { 250.0 } else { position.x },
                        if horizontal { position.y } else { 250.0 },
                    );
                    move_pointer(&window, outside);
                    assert!(window.get_dragging());
                    assert!((thickness() - 2.0).abs() < 0.5, "held drag stroke is 2px");
                    assert!((band() - 9.0).abs() < 0.5);
                    window.window().dispatch_event(WindowEvent::PointerExited);
                    settle();
                    assert!((thickness() - 1.0).abs() < 0.5);
                }
            }
        }
    }
}
