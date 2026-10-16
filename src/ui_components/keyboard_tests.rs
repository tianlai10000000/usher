use super::*;
use crate::ui_components::install;
use i_slint_core::accessibility::AccessibleStringProperty;
use i_slint_core::input::FocusReason;
use i_slint_core::item_tree::{
    ItemRc, ItemTreeRc, ItemVisitorResult, ParentItemTraversalMode, TraversalOrder, visit_items,
};
use i_slint_core::items::FocusScope;
use i_slint_core::window::WindowInner;
use slint::platform::WindowEvent;
use std::{cell::Cell, rc::Rc, time::Duration};

fn key(window: &MainWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn top_tree(mut item: ItemRc) -> ItemTreeRc {
    while let Some(parent) = item.parent_item(ParentItemTraversalMode::StopAtPopups) {
        item = parent;
    }
    item.item_tree().clone()
}

#[track_caller]
fn focus(window: &MainWindow) -> ItemRc {
    WindowInner::from_pub(window.window())
        .focus_item
        .borrow()
        .upgrade()
        .expect("host focus")
}

#[test]
fn focused_factory_dispatches_shortcut_once_and_tabs_through_the_host() {
    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.on_advance_sidebar_motion(crate::sidebar_motion::advance);
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1100.0, 700.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(true);
    window.invoke_restore_navigation_drawer_state(true);
    crate::i18n::apply_preference("en").unwrap();
    install(&window);
    window.set_current_route(crate::Route::UiComponents);
    for _ in 0..4 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
    let host = WindowInner::from_pub(window.window());
    host.ensure_tree_instantiated();
    let mut directory_link = None;
    visit_items(
        &host.component(),
        TraversalOrder::FrontToBack,
        |tree, _, index, ()| {
            let item = ItemRc::new(tree.clone(), index);
            if item.downcast::<FocusScope>().is_some()
                && item
                    .accessible_string_property(AccessibleStringProperty::Label)
                    .is_some_and(|label| label == "Foundations")
            {
                directory_link = Some(item);
                return ItemVisitorResult::Abort;
            }
            ItemVisitorResult::Continue(())
        },
        (),
    );
    let directory_link = directory_link.expect("real compiled gallery directory link");
    let factory_tree = top_tree(directory_link.clone());
    assert!(!ItemTreeRc::ptr_eq(&factory_tree, &host.component()));
    host.set_focus_item(&directory_link, true, FocusReason::Programmatic);
    assert_eq!(focus(&window), directory_link);
    let calls = Rc::new(Cell::new(0));
    let tab_calls = Rc::new(Cell::new(0));
    let captured = calls.clone();
    let tabs = tab_calls.clone();
    window.on_app_shortcut_key_pressed(move |text, _, _, _, meta| {
        if text == slint::SharedString::from(Key::Tab)
            || text == slint::SharedString::from(Key::Backtab)
        {
            tabs.set(tabs.get() + 1);
        }
        if meta && text == "k" {
            captured.set(captured.get() + 1);
            return true;
        }
        false
    });
    // Dispatch actual host key events while a factory control owns focus.
    // Calling the new callback directly would not prove the capture boundary.
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Meta.into(),
    });
    key(&window, "k");
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Meta.into(),
    });
    assert_eq!(
        calls.get(),
        1,
        "one executed host shortcut across the factory boundary"
    );
    // The directory itself is a programmatically focused scope used by
    // Back to top and precedes its first Link in the real Tab chain.
    // Follow that mounted boundary instead of assuming the first label is
    // also the first focusable item. A private-root loop must still fail.
    let mut boundary = None;
    for _ in 0..4 {
        let before = focus(&window);
        let forwarded = tab_calls.get();
        key(&window, Key::Backtab);
        assert_eq!(
            tab_calls.get(),
            forwarded + 1,
            "Tab bubbles through the factory bridge exactly once"
        );
        assert_ne!(focus(&window), before, "reverse Tab must advance focus");
        if !ItemTreeRc::ptr_eq(&top_tree(focus(&window)), &factory_tree) {
            boundary = Some(before);
            break;
        }
    }
    let boundary = boundary.expect("reverse Tab crosses from the directory to shell controls");
    key(&window, Key::Tab);
    assert_eq!(
        focus(&window),
        boundary,
        "forward Tab reenters the same factory boundary"
    );
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Shift.into(),
    });
    key(&window, Key::Tab);
    window.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Shift.into(),
    });
    assert!(!ItemTreeRc::ptr_eq(
        &top_tree(focus(&window)),
        &factory_tree
    ));
}

#[test]
fn select_tab_continues_from_its_opener_instead_of_the_directory() {
    use i_slint_backend_testing::{AccessibleRole, ElementHandle};
    use slint::platform::PointerEventButton;

    let _locale = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    crate::i18n::apply_preference("en").unwrap();
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1100.0, 700.0),
    });
    crate::install_floating_overlay_positioning(&window);
    install(&window);
    window.set_current_route(crate::Route::UiComponents);
    let settle = || i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(100));
    settle();
    ElementHandle::find_by_accessible_label(&window, "Selection")
        .find(|item| item.accessible_role() == Some(AccessibleRole::Button))
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    settle();
    let document = ElementHandle::find_by_element_id(&window, "GitHubUiComponentsWindow::document")
        .next()
        .unwrap();
    let trigger = || {
        ElementHandle::find_by_element_id(
            &window,
            "GitHubUiComponentsGallery::grouped-select-trigger",
        )
        .next()
    };
    for _ in 0..20 {
        if trigger().is_some() {
            break;
        }
        document.scroll(0.0, -200.0);
        settle();
    }
    let trigger = trigger().expect("visible grouped Select");
    document.scroll(0.0, 180.0 - trigger.absolute_position().y);
    settle();
    trigger.mock_single_click(PointerEventButton::Left);
    settle();
    assert_eq!(trigger.accessible_expanded(), Some(true));
    key(&window, Key::Escape);
    settle();
    let opener = focus(&window);
    let host = WindowInner::from_pub(window.window());
    for locale in ["en", "zh-CN"] {
        crate::i18n::apply_preference(locale).unwrap();
        settle();
        for (text, shift) in [(Key::Tab, false), (Key::Tab, true), (Key::Backtab, false)] {
            host.set_focus_item(&opener, true, FocusReason::Programmatic);
            if shift {
                window.window().dispatch_event(WindowEvent::KeyPressed {
                    text: Key::Shift.into(),
                });
            }
            key(&window, text);
            let expected = focus(&window);
            if shift {
                window.window().dispatch_event(WindowEvent::KeyReleased {
                    text: Key::Shift.into(),
                });
            }
            host.set_focus_item(&opener, true, FocusReason::Programmatic);
            key(&window, Key::Return);
            settle();
            assert_eq!(trigger.accessible_expanded(), Some(true));
            if shift {
                window.window().dispatch_event(WindowEvent::KeyPressed {
                    text: Key::Shift.into(),
                });
            }
            key(&window, text);
            if shift {
                window.window().dispatch_event(WindowEvent::KeyReleased {
                    text: Key::Shift.into(),
                });
            }
            assert_eq!(
                trigger.accessible_expanded(),
                Some(false),
                "{locale}: {text:?}, shift={shift}"
            );
            assert_eq!(
                focus(&window),
                expected,
                "{locale}: {text:?}, shift={shift} continues from opener"
            );
            assert!(
                trigger.absolute_position().y.abs() < 700.0,
                "Select must not jump to the directory"
            );
        }
    }
}

#[path = "host_select_tests.rs"]
mod host_select_tests;
