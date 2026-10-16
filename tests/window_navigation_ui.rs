use std::{cell::Cell, rc::Rc, time::Duration};

use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};
use usher_lib::{MainWindow, Route, TooltipOverlayState};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}

fn find(window: &MainWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("missing UI element {id}"))
}

fn button(window: &MainWindow, action: &str) -> ElementHandle {
    find(window, &format!("GitHubWindowNavigation::{action}-button"))
}

fn move_pointer(window: &MainWindow, position: LogicalPosition) {
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    settle();
}

fn hover(window: &MainWindow, action: &str) {
    let element = button(window, action);
    let origin = element.absolute_position();
    let size = element.size();
    move_pointer(
        window,
        LogicalPosition::new(origin.x + size.width / 2.0, origin.y + size.height / 2.0),
    );
}

fn window() -> MainWindow {
    let window = MainWindow::new().expect("main window");
    window.set_platform_is_macos(true);
    window.set_native_window_fullscreen(true);
    window.set_show_navigation_drawer(true);
    window.invoke_restore_navigation_drawer_state(true);
    window.set_current_route(Route::Project);
    window.set_shortcut_navigate_back_label("⌘[".into());
    window.set_shortcut_navigate_forward_label("⌘]".into());
    window.set_shortcut_toggle_sidebar_label("⌘B".into());
    window.set_navigation_can_go_back(true);
    window.set_navigation_can_go_forward(true);
    window.show().unwrap();
    window.window().dispatch_event(WindowEvent::Resized {
        size: LogicalSize::new(1200.0, 800.0),
    });
    settle();
    window
}

fn assert_tooltip(window: &MainWindow, expected: &str) -> f32 {
    let state = window.global::<TooltipOverlayState>();
    assert!(state.get_open(), "tooltip must be open for {expected}");
    assert_eq!(state.get_text(), expected);
    let label = find(window, "GitHubTooltipSurface::tip-label");
    assert_eq!(label.accessible_label().as_deref(), Some(expected));
    let surface = ElementHandle::find_by_element_type_name(window, "GitHubTooltipSurface")
        .next()
        .expect("root tooltip surface");
    let origin = surface.absolute_position();
    let size = surface.size();
    assert!(origin.x >= 8.0, "tooltip clears the left window edge");
    assert!(origin.y >= 8.0, "tooltip clears the top window edge");
    assert!(origin.x + size.width <= window.window().size().width as f32 + 0.5);
    assert!(origin.y + size.height <= window.window().size().height as f32 + 0.5);
    // The query API skips invisible nodes, including the private measurement
    // Text. Inspect the painted label and compare short/long surface widths at
    // the call site instead of trying to query that hidden implementation node.
    let label_origin = label.absolute_position();
    assert!(
        label_origin.x >= origin.x && label_origin.y >= origin.y,
        "painted label starts inside the tooltip surface"
    );
    assert!(label_origin.x + label.size().width <= origin.x + size.width + 0.5);
    assert!(label_origin.y + label.size().height <= origin.y + size.height + 0.5);
    size.width
}

#[test]
fn fullscreen_actions_keep_their_order_first_click_and_history_eligibility() {
    i_slint_backend_testing::init_no_event_loop();
    let window = window();
    let back_count = Rc::new(Cell::new(0));
    let forward_count = Rc::new(Cell::new(0));
    let toggle_count = Rc::new(Cell::new(0));
    let count = back_count.clone();
    window.on_navigate_back(move || count.set(count.get() + 1));
    let count = forward_count.clone();
    window.on_navigate_forward(move || count.set(count.get() + 1));
    let count = toggle_count.clone();
    let weak = window.as_weak();
    window.on_toggle_navigation_drawer(move || {
        count.set(count.get() + 1);
        let window = weak.upgrade().unwrap();
        window.invoke_restore_navigation_drawer_state(!window.get_navigation_drawer_expanded());
    });

    for (action, x) in [("back", 8.0), ("forward", 40.0), ("sidebar", 72.0)] {
        let element = button(&window, action);
        assert_eq!(element.absolute_position(), LogicalPosition::new(x, 9.0));
        assert_eq!(element.size(), LogicalSize::new(28.0, 28.0));
        element.mock_single_click(PointerEventButton::Left);
        settle();
    }
    assert_eq!(
        back_count.get(),
        1,
        "first unfocused pointer click activates"
    );
    assert_eq!(forward_count.get(), 1);
    assert_eq!(toggle_count.get(), 1);
    assert!(!window.get_navigation_drawer_expanded());
    assert!(!window.global::<TooltipOverlayState>().get_open());

    for key in [Key::Return.into(), " ".into()] {
        window
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: key });
        settle();
    }
    assert_eq!(
        toggle_count.get(),
        3,
        "Enter and Space use the focused action"
    );

    window.set_navigation_can_go_back(false);
    window.set_navigation_can_go_forward(false);
    settle();
    for action in ["back", "forward"] {
        let element = button(&window, action);
        assert_eq!(element.accessible_enabled(), Some(false));
        element.mock_single_click(PointerEventButton::Left);
    }
    assert_eq!(back_count.get(), 1);
    assert_eq!(forward_count.get(), 1);

    window.set_current_route(Route::Settings);
    window.set_settings_hook_dialog_open(true);
    settle();
    assert!(window.get_modal_overlay_visible());
    for action in ["back", "forward", "sidebar"] {
        assert_eq!(button(&window, action).accessible_enabled(), Some(false));
        hover(&window, action);
        button(&window, action).mock_single_click(PointerEventButton::Left);
        assert!(!window.global::<TooltipOverlayState>().get_open());
    }
    assert_eq!(toggle_count.get(), 3, "modal prevents sidebar activation");
    window.set_settings_hook_dialog_open(false);
    settle();
    button(&window, "sidebar").mock_single_click(PointerEventButton::Left);
    assert_eq!(
        toggle_count.get(),
        4,
        "closing modal restores the same control"
    );
}

#[test]
fn fullscreen_breadcrumb_reserves_navigation_with_sidebar_open_or_closed() {
    i_slint_backend_testing::init_no_event_loop();
    let window = window();
    // Retain the mounted element handle: visible-tree queries intentionally
    // omit this control while native window chrome owns the actions.
    let sidebar_button = button(&window, "sidebar");
    for width in [800.0, 1600.0, 800.0] {
        window.window().dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width, 800.0),
        });
        for (fullscreen, expanded, expected_x) in [
            (true, true, 240.0),
            (true, false, 112.0),
            (false, false, 184.0),
            (false, true, 240.0),
        ] {
            window.set_native_window_fullscreen(fullscreen);
            window.invoke_restore_navigation_drawer_state(expanded);
            settle();
            let breadcrumb = find(&window, "MainWindow::titlebar-breadcrumb-host");
            assert!((breadcrumb.absolute_position().x - expected_x).abs() < 0.5);
            assert!((breadcrumb.absolute_position().y).abs() < 0.5);
            assert_eq!(breadcrumb.size().height, 46.0);
            assert_eq!(sidebar_button.accessible_enabled(), Some(fullscreen));
            assert!(sidebar_button.is_valid(), "the control stays mounted");
        }
    }
    window.set_native_window_fullscreen(true);
    window.set_platform_is_macos(false);
    settle();
    assert_eq!(sidebar_button.accessible_enabled(), Some(false));
}

#[test]
fn fullscreen_tooltips_follow_locale_action_bounds_and_repeated_reentry() {
    i_slint_backend_testing::init_no_event_loop();
    let window = window();
    for locale in ["en", "zh-CN", "en"] {
        slint::select_bundled_translation(locale).unwrap();
        for width in [800.0, 1600.0, 800.0] {
            window.window().dispatch_event(WindowEvent::Resized {
                size: LogicalSize::new(width, 800.0),
            });
            for expanded in [true, false, true] {
                window.invoke_restore_navigation_drawer_state(expanded);
                let mut back_width: Option<f32> = None;
                for (action, key, shortcut) in [
                    ("back", "common.back", "⌘["),
                    ("forward", "tooltip.forward", "⌘]"),
                    (
                        "sidebar",
                        if expanded {
                            "tooltip.hide_sidebar"
                        } else {
                            "tooltip.show_sidebar"
                        },
                        "⌘B",
                    ),
                    ("back", "common.back", "⌘["),
                ] {
                    hover(&window, action);
                    let expected =
                        format!("{} ({shortcut})", usher_i18n::translate_for(locale, key));
                    assert_eq!(
                        button(&window, action).accessible_label().as_deref(),
                        Some(expected.as_str())
                    );
                    let width = assert_tooltip(&window, &expected);
                    if action == "back" {
                        if let Some(previous_width) = back_width {
                            assert!(
                                (width - previous_width).abs() < 0.5,
                                "short tooltip returns to its original width after longer neighbors"
                            );
                        } else {
                            back_width = Some(width);
                        }
                    } else if action == "sidebar" {
                        assert!(
                            width > back_width.unwrap(),
                            "the longer sidebar label must expand the tooltip surface"
                        );
                    }
                }
            }
        }
    }

    // Change locale while the same anchor remains hovered and its portal open.
    hover(&window, "sidebar");
    for locale in ["zh-CN", "en"] {
        slint::select_bundled_translation(locale).unwrap();
        settle();
        assert_tooltip(
            &window,
            &format!(
                "{} (⌘B)",
                usher_i18n::translate_for(locale, "tooltip.hide_sidebar")
            ),
        );
    }
    for _ in 0..20 {
        hover(&window, "back");
        assert!(window.global::<TooltipOverlayState>().get_open());
        window.set_native_window_fullscreen(false);
        settle();
        assert!(!window.global::<TooltipOverlayState>().get_open());
        move_pointer(&window, LogicalPosition::new(600.0, 500.0));
        window.set_native_window_fullscreen(true);
        settle();
        assert!(!window.global::<TooltipOverlayState>().get_open());
    }
}
