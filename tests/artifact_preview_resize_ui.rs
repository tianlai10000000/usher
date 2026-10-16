use std::time::Duration;

use i_slint_backend_testing::ElementHandle;
use i_slint_backend_testing::testing_backend::TestingWindow;
use i_slint_core::cursor::MouseCursorInner;
use i_slint_core::items::BuiltInMouseCursor;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, Global, LogicalPosition, LogicalSize};
use usher_lib::{AppState, ArtifactPreviewKind, MainWindow, Route};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(350));
}

fn move_pointer(window: &MainWindow, x: f32, y: f32) {
    window.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(x, y),
    });
}

fn cursor(window: &MainWindow) -> MouseCursorInner {
    let adapter = i_slint_core::window::WindowInner::from_pub(window.window()).window_adapter();
    let testing = adapter
        .internal(i_slint_core::InternalToken)
        .and_then(|adapter| (adapter as &dyn std::any::Any).downcast_ref::<TestingWindow>())
        .expect("testing window adapter");
    testing.mouse_cursor()
}

#[test]
fn docked_preview_captures_first_drag_across_the_complete_divider() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().expect("main window");
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(true);
    window.set_navigation_drawer_expanded(true);
    window.set_show_breadcrumb(false);
    window.set_artifact_preview_kind(ArtifactPreviewKind::PlainText);
    window.set_artifact_preview_plain_text("Preview content\n".repeat(100).into());
    window.set_artifact_preview_visible(true);

    for mode in [0, 2] {
        AppState::get(&window).set_artifact_preview_presentation_mode(mode);
        for width in [1280.0, 800.0] {
            window.window().dispatch_event(WindowEvent::Resized {
                size: LogicalSize::new(width, 800.0),
            });
            for route in [Route::Chat, Route::Home, Route::Project] {
                window.set_current_route(route);
                window.set_show_chat_page(route == Route::Chat);
                for locale in ["en", "zh-CN"] {
                    slint::select_bundled_translation(locale).unwrap();
                    window.set_artifact_preview_visible(false);
                    settle();
                    window.set_artifact_preview_visible(true);
                    settle();
                    let content =
                        ElementHandle::find_by_element_id(&window, "MainWindow::main-content-host")
                            .next()
                            .unwrap();
                    let preview = ElementHandle::find_by_element_id(
                        &window,
                        "MainWindow::artifact-preview-panel-host",
                    )
                    .next()
                    .unwrap();
                    let content_width = content.size().width;
                    let content_right = content.absolute_position().x + content_width;
                    let divider = window.get_artifact_preview_divider_x();
                    if mode == 0 {
                        assert!(
                            (content_right - divider).abs() < 1.0,
                            "split content must end at the divider"
                        );
                    } else {
                        assert!(
                            (content_right - width).abs() < 1.0,
                            "floating preview must not reserve content width"
                        );
                    }
                    for offset in [-3.0, 0.0, 3.0] {
                        let start = window.get_artifact_preview_divider_x();
                        let x = start + offset;
                        let y = 300.0;
                        move_pointer(&window, x, y);
                        assert_eq!(
                            cursor(&window),
                            MouseCursorInner::BuiltIn(BuiltInMouseCursor::EwResize),
                            "divider hover: mode={mode}, width={width}, route={route:?}, locale={locale}, offset={offset}"
                        );
                        window.window().dispatch_event(WindowEvent::PointerPressed {
                            position: LogicalPosition::new(x, y),
                            button: PointerEventButton::Left,
                        });
                        assert!(
                            window.get_artifact_preview_divider_resizing(),
                            "first press must capture: width={width}, route={route:?}, locale={locale}, offset={offset}"
                        );
                        let preview_width = preview.size().width;
                        let direction = if mode == 2 && preview_width <= 320.0 {
                            -1.0
                        } else {
                            1.0
                        };
                        for delta in [8.0, 16.0, 8.0, 0.0].map(|delta| delta * direction) {
                            move_pointer(&window, x + delta, y);
                            let actual = window.get_artifact_preview_divider_x();
                            assert!(
                                (actual - start - delta).abs() < 1.0,
                                "divider must follow one held drag: expected {}, got {actual}",
                                start + delta
                            );
                            let expected_content =
                                content_width + if mode == 0 { delta } else { 0.0 };
                            assert!((content.size().width - expected_content).abs() < 1.0);
                            assert!((preview.size().width - preview_width + delta).abs() < 1.0);
                        }
                        window
                            .window()
                            .dispatch_event(WindowEvent::PointerReleased {
                                position: LogicalPosition::new(x, y),
                                button: PointerEventButton::Left,
                            });
                        assert!(!window.get_artifact_preview_divider_resizing());
                    }
                }
            }
        }
    }

    window
        .global::<usher_lib::ProjectBridge>()
        .set_show_add_modal(true);
    settle();
    let start = window.get_artifact_preview_divider_x();
    for offset in [-3.0, 0.0, 3.0] {
        let position = LogicalPosition::new(start + offset, 300.0);
        move_pointer(&window, position.x, position.y);
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        assert!(!window.get_artifact_preview_divider_resizing());
        move_pointer(&window, position.x + 40.0, position.y);
        assert_eq!(window.get_artifact_preview_divider_x(), start);
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
    }
}

#[test]
#[cfg(feature = "ai-code-review")]
fn pr_review_divider_keeps_first_drag_capture_with_either_trailing_surface() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1280.0, 800.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.set_current_route(Route::PrReview);
    window.set_ai_review_panel_visible(true);
    window.set_artifact_preview_pr_side_context(true);
    window.set_artifact_preview_kind(ArtifactPreviewKind::PlainText);
    window.set_artifact_preview_plain_text("Preview content\n".repeat(100).into());
    for preview in [false, true] {
        window.set_artifact_preview_visible(preview);
        settle();
        for offset in [-3.0, 0.0, 3.0] {
            let width = window.get_pr_review_trailing_surface_width();
            let x = 1280.0 - width + offset;
            move_pointer(&window, x, 300.0);
            assert_eq!(
                cursor(&window),
                MouseCursorInner::BuiltIn(BuiltInMouseCursor::EwResize),
                "PR trailing divider hover: preview={preview}, offset={offset}"
            );
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position: LogicalPosition::new(x, 300.0),
                button: PointerEventButton::Left,
            });
            for delta in [-40.0, 0.0] {
                move_pointer(&window, x + delta, 300.0);
                assert!(
                    (window.get_pr_review_trailing_surface_width() - width + delta).abs() < 1.0
                );
            }
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position: LogicalPosition::new(x, 300.0),
                    button: PointerEventButton::Left,
                });
        }
    }
}
