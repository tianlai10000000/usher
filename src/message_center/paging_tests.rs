use super::*;
use slint::platform::WindowEvent;
use std::time::Duration;
use usher_messages::{MessageDraft, NoopDesktopNotifier};
use usher_ui::pagination_links;

use i_slint_core::accessibility::AccessibleStringProperty;
use i_slint_core::item_tree::{ItemRc, ItemVisitorResult, TraversalOrder, visit_items};
use i_slint_core::items::{BasicBorderRectangle, Clip, FocusScope, Opacity};
use i_slint_core::lengths::LogicalPoint;
use i_slint_core::window::WindowInner;
use slint::platform::{Key, PointerEventButton};

#[test]
fn pagination_links_are_bounded_ordered_and_include_current_and_endpoints() {
    for total in 1..=50 {
        for current in 1..=total {
            let links = pagination_links(current, total);
            assert!(links.len() <= 9, "{current}/{total}: {links:?}");
            assert_eq!(links[0], 1);
            assert_eq!(links.last(), Some(&(total as i32)));
            assert!(links.contains(&(current as i32)));
            let pages: Vec<_> = links.iter().copied().filter(|page| *page != 0).collect();
            assert!(pages.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(!links.windows(2).any(|pair| pair == [0, 0]));
        }
    }
    assert_eq!(pagination_links(1, 50), [1, 2, 3, 4, 5, 6, 7, 0, 50]);
    assert_eq!(pagination_links(25, 50), [1, 0, 23, 24, 25, 26, 27, 0, 50]);
    assert_eq!(pagination_links(50, 50), [1, 0, 44, 45, 46, 47, 48, 49, 50]);
}

fn settle() {
    for _ in 0..4 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

fn assert_visible_rows(page: &GitHubMessagesPage, expected: usize) {
    assert_eq!(page.get_page_row_count() as usize, expected);
    assert_eq!(page.get_messages().row_count(), MESSAGE_PAGE_SIZE);
    for row in page.get_messages().iter().skip(expected) {
        assert!(message_row_equivalent(&row, &MessageListItem::default()));
    }
}

// Follow the mounted host tree, carrying its offset into the independently
// generated page. ItemRc::map_to_window alone stops at the factory root.
fn mounted_control(window: &MainWindow, label: &str) -> (ItemRc, slint::LogicalPosition) {
    mounted_control_with_role(window, label, None)
}

fn mounted_control_with_role(
    window: &MainWindow,
    label: &str,
    role: Option<i_slint_core::items::AccessibleRole>,
) -> (ItemRc, slint::LogicalPosition) {
    let mut found = None;
    visit_items(
        &WindowInner::from_pub(window.window()).component(),
        TraversalOrder::FrontToBack,
        |tree, _, index, (origin, opacity)| {
            let item = ItemRc::new(tree.clone(), index);
            let geometry = item.geometry();
            let origin = *origin + geometry.origin.to_vector();
            let opacity = *opacity
                * item
                    .downcast::<Opacity>()
                    .map_or(1.0, |item| item.as_pin_ref().opacity());
            if opacity <= 0.0
                || item.downcast::<Clip>().is_some_and(|clip| {
                    let clip = clip.as_pin_ref();
                    clip.is_visibility_clip() && clip.clip()
                })
            {
                return ItemVisitorResult::SkipChildren;
            }
            if item
                .accessible_string_property(AccessibleStringProperty::Label)
                .is_some_and(|name| name == label)
                && role.is_none_or(|role| item.accessible_role() == role)
            {
                found = Some((
                    item,
                    slint::LogicalPosition::new(
                        origin.x + geometry.size.width / 2.0,
                        origin.y + geometry.size.height / 2.0,
                    ),
                ));
                return ItemVisitorResult::Abort;
            }
            ItemVisitorResult::Continue((origin, opacity))
        },
        (LogicalPoint::default(), 1.0f32),
    );
    found.unwrap_or_else(|| panic!("mounted focus control {label}"))
}

fn click_control(window: &MainWindow, label: &str) -> ItemRc {
    let (item, position) = mounted_control(window, label);
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
    item
}

fn key(window: &MainWindow, text: impl Into<slint::SharedString>) {
    let text = text.into();
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn host_focus(window: &MainWindow) -> Option<ItemRc> {
    WindowInner::from_pub(window.window())
        .focus_item
        .borrow()
        .upgrade()
}

fn tab_until(window: &MainWindow, target: &ItemRc) {
    for _ in 0..160 {
        key(window, Key::Tab);
        if host_focus(window).as_ref() == Some(target) {
            return;
        }
    }
    panic!("target did not enter the mounted host Tab chain");
}

fn focus_outline_width(control: &ItemRc) -> f32 {
    control
        .visit_descendants(|item| {
            if let Some(chrome) = item.downcast::<BasicBorderRectangle>() {
                return std::ops::ControlFlow::Break(chrome.as_pin_ref().border_width().0);
            }
            std::ops::ControlFlow::Continue(())
        })
        .expect("pagination chrome")
}

#[test]
fn compiled_inbox_pointer_handoff_keyboard_paging_and_hidden_focus_follow_host() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    for locale in ["en", "zh-CN"] {
        let directory = tempfile::tempdir().unwrap();
        let center = MessageCenter::open_with(
            directory.path().join("messages.json"),
            Arc::new(NoopDesktopNotifier),
        )
        .unwrap();
        for index in 0..41 {
            center
                .publish(MessageDraft::new(format!("Message {index}"), "Fixture").desktop(false))
                .unwrap();
        }
        let window = MainWindow::new().unwrap();
        window.on_advance_sidebar_motion(crate::sidebar_motion::advance);
        crate::dialog::setup_dialog(&window);
        window.window().dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(1_000.0, 700.0),
        });
        window.set_platform_is_macos(false);
        window.set_show_navigation_drawer(true);
        window.invoke_restore_navigation_drawer_state(true);
        let bridge = MessagePageBridge::new(center.clone());
        bridge.install(&window);
        let weak_window = window.as_weak();
        window.on_navigate_from_nav_index(move |index| {
            if index == 0
                && let Some(window) = weak_window.upgrade()
            {
                window.set_current_route(crate::Route::Home);
            }
        });
        crate::i18n::apply_preference(locale).unwrap();
        bridge.sync_language(locale);
        window.set_current_route(crate::Route::Messages);
        settle();
        assert!(
            window.get_sidebar_motion_observed_progress() > 0.99,
            "fixture uses the production sidebar spring instead of a default callback"
        );
        let page = bridge
            .page
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap();
        let model = page.get_messages();
        // Take a real host-side focus before clicking the independently
        // generated page; calling page.invoke_change_page would hide UI-120.
        // The sidebar has a FocusScope without its own accessible label. Its
        // pointer handler focuses it using the real host's generated API.
        let (home, position) = mounted_control_with_role(
            &window,
            crate::i18n::tr("navigation.home").as_ref(),
            Some(i_slint_core::items::AccessibleRole::Tab),
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
        assert_eq!(
            home.accessible_role(),
            i_slint_core::items::AccessibleRole::Tab
        );
        assert_eq!(window.get_current_route(), crate::Route::Home);
        window.set_current_route(crate::Route::Messages);
        settle();
        key(&window, Key::Tab);
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Shift.into(),
        });
        key(&window, Key::Tab);
        window.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
        let previous_focus = host_focus(&window).expect("sidebar owns the host focus");
        let mut ancestor = Some(previous_focus.clone());
        while let Some(item) = ancestor {
            if item == home {
                break;
            }
            ancestor =
                item.parent_item(i_slint_core::item_tree::ParentItemTraversalMode::StopAtPopups);
            assert!(
                ancestor.is_some(),
                "keyboard focus belongs to the actual Home control"
            );
        }
        assert!(!i_slint_core::item_tree::ItemTreeRc::ptr_eq(
            previous_focus.item_tree(),
            &WindowInner::from_pub(page.window()).component()
        ));
        page.set_scroll_y(-10_000.0);
        page.invoke_clamp_scroll();
        settle();
        let page_two = click_control(&window, &format!("{} 2", crate::i18n::tr("common.page")));
        assert_eq!(page.get_current_page(), 2, "first pointer click activates");
        assert_eq!(
            page.get_scroll_y(),
            0.0,
            "pointer paging returns to the top"
        );
        assert!(
            focus::owns_page_focus(&page, &window),
            "neutral factory focus belongs to real host"
        );
        assert!(
            !previous_focus
                .downcast::<FocusScope>()
                .unwrap()
                .as_pin_ref()
                .has_focus()
        );
        assert!(
            !page_two
                .downcast::<FocusScope>()
                .unwrap()
                .as_pin_ref()
                .has_focus()
        );
        assert!(
            WindowInner::from_pub(page.window())
                .focus_item
                .borrow()
                .upgrade()
                .is_none(),
            "private adapter owns no duplicate focus"
        );
        key(&window, Key::Return);
        key(&window, " ");
        assert_eq!(
            page.get_current_page(),
            2,
            "neutral pointer focus never activates an unrelated action"
        );
        assert_eq!(window.get_current_route(), crate::Route::Messages);
        assert_eq!(center.snapshot().unread_count, 41);
        tab_until(&window, &page_two);
        key(&window, Key::Tab);
        let page_three =
            mounted_control(&window, &format!("{} 3", crate::i18n::tr("common.page"))).0;
        assert_eq!(host_focus(&window), Some(page_three.clone()));
        key(&window, Key::Return);
        assert_eq!(page.get_current_page(), 3);
        assert_eq!(host_focus(&window), Some(page_three.clone()));
        // Seed the exact generated .focus() adapter path explicitly. A prior
        // independently generated control can retain this private focus; its
        // presence must not classify a later real keyboard page activation as
        // a pointer activation. Keep the real host focused on Page 3.
        page.invoke_focus_page();
        assert!(
            WindowInner::from_pub(page.window())
                .focus_item
                .borrow()
                .upgrade()
                .is_some()
        );
        assert_eq!(host_focus(&window), Some(page_three));
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Shift.into(),
        });
        key(&window, Key::Tab);
        window.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
        assert_eq!(host_focus(&window), Some(page_two.clone()));
        key(&window, " ");
        assert_eq!(page.get_current_page(), 2);
        assert_eq!(host_focus(&window), Some(page_two.clone()));
        assert!(
            page.get_scroll_y() < 0.0,
            "keyboard paging keeps the footer visible even after a short last page"
        );
        let shortcuts = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = shortcuts.clone();
        window.on_app_shortcut_key_pressed(move |text, _, _, _, meta| {
            if meta && text == "k" {
                count.set(count.get() + 1);
                return true;
            }
            false
        });
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Meta.into(),
        });
        key(&window, "k");
        window.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Meta.into(),
        });
        assert_eq!(
            shortcuts.get(),
            1,
            "factory forwards an executed host shortcut once"
        );
        assert!(!page.invoke_host_key_pressed("x".into(), false, false, false, false));
        // Walk all the way out of the factory and back, with the inactive
        // opacity-zero sidebar preview still mounted alongside the docked one.
        for _ in 0..160 {
            key(&window, Key::Tab);
            if !focus::owns_page_focus(&page, &window) {
                break;
            }
        }
        assert!(
            !focus::owns_page_focus(&page, &window),
            "Tab exits the factory"
        );
        window.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Shift.into(),
        });
        key(&window, Key::Tab);
        window.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Shift.into(),
        });
        assert!(
            focus::owns_page_focus(&page, &window),
            "Shift+Tab returns from the host"
        );
        // Some backends normalize reverse Tab to its own key rather than
        // supplying a Shift modifier. This must cross the factory boundary as
        // well, instead of falling back to the private root's local loop.
        let mut backward_trace = Vec::new();
        for _ in 0..160 {
            key(&window, Key::Backtab);
            backward_trace.push((
                host_focus(&window).map(|item| {
                    (
                        item.index(),
                        item.accessible_string_property(AccessibleStringProperty::Label),
                        item.is_visible(),
                    )
                }),
                page.get_scroll_y(),
            ));
            if !focus::owns_page_focus(&page, &window) {
                break;
            }
        }
        assert!(
            !focus::owns_page_focus(&page, &window),
            "Backtab exits the factory in reverse: {:?}",
            &backward_trace[backward_trace.len().saturating_sub(30)..]
        );
        key(&window, Key::Tab);
        assert!(
            focus::owns_page_focus(&page, &window),
            "forward Tab returns after a reverse boundary crossing"
        );
        // A modal blocks dispatch into a page even if native focus transfer to
        // the dialog has not happened yet.
        window.invoke_show_update_dialog("fixture-2".into(), "fixture-1".into());
        settle();
        assert!(window.get_modal_overlay_visible());
        assert!(page.invoke_host_key_pressed(Key::Return.into(), false, false, false, false));
        crate::DialogManager::get(&window).invoke_close();
        WindowInner::from_pub(window.window()).set_focus_item(
            &page_two,
            true,
            i_slint_core::input::FocusReason::TabNavigation,
        );
        assert_eq!(page.get_messages(), model);
        window.set_current_route(crate::Route::Home);
        settle();
        assert!(
            host_focus(&window).is_none(),
            "hidden Inbox releases its host focus"
        );
        assert!(page.invoke_host_key_pressed(Key::Return.into(), false, false, false, false));
        key(&window, Key::Return);
        assert_eq!(
            page.get_current_page(),
            2,
            "hidden Inbox cannot receive activation"
        );
        window.set_current_route(crate::Route::Messages);
        settle();
        assert_eq!(page.get_current_page(), 2);
        assert_eq!(page.get_messages(), model);
        bridge.commands.shutdown().unwrap();
        center.shutdown();
    }
}

#[test]
fn compiled_inbox_narrow_hidden_slots_disabled_directions_and_single_page_are_inert() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(NoopDesktopNotifier),
    )
    .unwrap();
    for index in 0..1_000 {
        center
            .publish(MessageDraft::new(format!("Message {index}"), "Fixture").desktop(false))
            .unwrap();
    }
    let window = MainWindow::new().unwrap();
    window.on_advance_sidebar_motion(crate::sidebar_motion::advance);
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1_000.0, 700.0),
    });
    window.set_platform_is_macos(false);
    window.set_show_navigation_drawer(false);
    let bridge = MessagePageBridge::new(center.clone());
    bridge.install(&window);
    crate::i18n::apply_preference("en").unwrap();
    bridge.sync_language("en");
    window.set_current_route(crate::Route::Messages);
    settle();
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    let model = page.get_messages();
    assert_eq!(page.get_page_count(), 50);
    assert!(
        page.get_page_links().iter().any(|page| page == 0),
        "fixture includes noninteractive ellipsis"
    );
    page.invoke_change_page(49);
    page.invoke_reveal_pagination();
    settle();
    let final_direction = mounted_control(&window, "Next").0;
    WindowInner::from_pub(window.window()).set_focus_item(
        &final_direction,
        true,
        i_slint_core::input::FocusReason::TabNavigation,
    );
    assert_eq!(
        focus_outline_width(&final_direction),
        2.0,
        "enabled keyboard focus paints an outline"
    );
    key(&window, Key::Return);
    assert_eq!(page.get_current_page(), 50);
    assert_eq!(
        focus_outline_width(&final_direction),
        0.0,
        "a direction disabled by keyboard paging clears its outline"
    );
    key(&window, Key::Return);
    key(&window, " ");
    assert_eq!(page.get_current_page(), 50);
    assert_eq!(
        focus_outline_width(&final_direction),
        0.0,
        "disabled activation cannot restore the stale outline"
    );
    key(&window, Key::Tab);
    assert!(!focus::owns_page_focus(&page, &window));
    assert_eq!(
        focus_outline_width(&final_direction),
        0.0,
        "leaving the disabled direction never leaves a second outline"
    );
    page.invoke_change_page(50);
    page.invoke_reveal_pagination();
    settle();
    let next = click_control(&window, "Next");
    assert!(
        !next
            .downcast::<FocusScope>()
            .unwrap()
            .as_pin_ref()
            .enabled()
    );
    assert_eq!(page.get_current_page(), 50);
    let last = mounted_control(&window, "Page 50").0;
    WindowInner::from_pub(window.window()).set_focus_item(
        &last,
        true,
        i_slint_core::input::FocusReason::TabNavigation,
    );
    key(&window, Key::Tab);
    assert!(
        !focus::owns_page_focus(&page, &window),
        "Tab skips the disabled final direction and exits the factory"
    );
    // Resizing hides number links without replacing their slots. A hidden
    // focused page number must not activate itself or the old host control.
    WindowInner::from_pub(window.window()).set_focus_item(
        &last,
        true,
        i_slint_core::input::FocusReason::TabNavigation,
    );
    window.set_show_navigation_drawer(true);
    window.invoke_restore_navigation_drawer_state(true);
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(640.0, 700.0),
    });
    settle();
    assert!(
        !last
            .downcast::<FocusScope>()
            .unwrap()
            .as_pin_ref()
            .enabled()
    );
    key(&window, Key::Return);
    assert_eq!(page.get_current_page(), 50);
    assert_eq!(window.get_current_route(), crate::Route::Messages);
    assert_eq!(center.snapshot().unread_count, 1_000);
    page.invoke_reveal_pagination();
    click_control(&window, "Previous");
    assert_eq!(
        page.get_current_page(),
        49,
        "narrow direction-only controls still activate on the first click"
    );
    assert_eq!(page.get_scroll_y(), 0.0);
    assert!(focus::owns_page_focus(&page, &window));
    page.invoke_change_filter(2);
    assert_eq!(page.get_page_count(), 1);
    assert_eq!(page.get_page_row_count(), 0);
    key(&window, Key::Return);
    key(&window, " ");
    assert_eq!(page.get_selected_filter(), 2);
    assert_eq!(window.get_current_route(), crate::Route::Messages);
    let id = center.snapshot().messages[0].id.clone();
    center.set_read(&id, true).unwrap();
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_page_count(), 1);
    assert_eq!(page.get_page_row_count(), 1);
    key(&window, Key::Return);
    assert_eq!(center.snapshot().unread_count, 999);
    assert_eq!(
        page.get_messages(),
        model,
        "all focus transitions preserve the bounded row model"
    );
    bridge.commands.shutdown().unwrap();
    center.shutdown();
}

#[test]
fn compiled_inbox_pages_filters_and_reentry_keep_only_twenty_rows() {
    let _locale_guard = crate::i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let directory = tempfile::tempdir().unwrap();
    let center = MessageCenter::open_with(
        directory.path().join("messages.json"),
        Arc::new(NoopDesktopNotifier),
    )
    .unwrap();
    for index in 0..41 {
        center
            .publish(MessageDraft::new(format!("Message {index}"), "Fixture").desktop(false))
            .unwrap();
    }
    let window = MainWindow::new().unwrap();
    window.window().dispatch_event(WindowEvent::Resized {
        size: slint::LogicalSize::new(1_000.0, 700.0),
    });
    let bridge = MessagePageBridge::new(center.clone());
    bridge.install(&window);
    window.set_current_route(crate::Route::Messages);
    settle();
    let page = bridge
        .page
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap();
    assert_eq!(page.get_total_count(), 41);
    assert_eq!(page.get_unread_count(), 41);
    assert_eq!(page.get_page_count(), 3);
    assert_eq!(page.get_messages().row_count(), 20);

    let first_ids: Vec<_> = page.get_messages().iter().map(|row| row.id).collect();
    let model = page.get_messages();
    page.invoke_set_read(first_ids[0].clone(), true);
    bridge.commands.barrier();
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_unread_count(), 40);
    assert_eq!(
        page.get_messages(),
        model,
        "read changes retain model identity"
    );
    assert_eq!(
        page.get_messages()
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        first_ids
    );

    page.set_scroll_y(-48.0);
    for filter in [1, 0, 1, 0] {
        page.invoke_change_filter(filter);
        settle();
        assert_eq!(page.get_scroll_y(), -48.0, "filter tabs preserve viewport");
        assert_eq!(page.get_current_page(), 1);
    }

    page.set_scroll_y(-500.0);
    page.invoke_change_page(3);
    assert_eq!(page.get_current_page(), 3);
    assert_eq!(page.get_scroll_y(), 0.0);
    assert_visible_rows(&page, 1);
    assert_eq!(
        page.get_messages(),
        model,
        "page changes preserve delegate slots"
    );
    assert_eq!(
        page.get_messages().row_data(0).unwrap().id.as_str(),
        center.snapshot().messages[40].id
    );
    let last_id = page.get_messages().row_data(0).unwrap().id;
    page.invoke_delete_message(last_id);
    bridge.commands.barrier();
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_current_page(), 2);
    assert_eq!(page.get_page_count(), 2);
    assert_eq!(page.get_messages().row_count(), 20);

    page.set_scroll_y(-500.0);
    page.invoke_change_filter(2);
    settle();
    assert_eq!(
        page.get_scroll_y(),
        0.0,
        "short results clamp an out-of-range viewport"
    );
    assert_eq!(page.get_selected_filter(), 2);
    assert_eq!(page.get_current_page(), 1);
    assert_eq!(page.get_page_count(), 1);
    assert_visible_rows(&page, 1);
    assert!(page.get_messages().row_data(0).unwrap().read);
    page.invoke_change_filter(1);
    assert_eq!(page.get_unread_count(), 39);
    assert_eq!(page.get_messages().row_count(), 20);
    assert!(page.get_messages().iter().all(|row| !row.read));

    bridge.set_active(false);
    assert_visible_rows(&page, 0);
    assert_eq!(page.get_messages(), model);
    bridge.sync_language("en");
    assert_visible_rows(&page, 0);
    center
        .publish(MessageDraft::new("Off-page update", "Fixture").desktop(false))
        .unwrap();
    bridge.set_active(true);
    assert_eq!(page.get_messages(), model);
    assert_eq!(page.get_messages().row_count(), 20);
    assert_eq!(page.get_unread_count(), 40);
    assert_eq!(
        page.get_messages().row_data(0).unwrap().title,
        "Off-page update"
    );

    page.invoke_mark_all_read();
    bridge.commands.barrier();
    apply_page_snapshot(&page, center.snapshot());
    assert_visible_rows(&page, 0);
    assert_eq!(page.get_page_count(), 1);
    assert_eq!(page.get_unread_count(), 0);
    page.invoke_change_filter(0);
    page.invoke_clear_messages();
    bridge.commands.barrier();
    apply_page_snapshot(&page, center.snapshot());
    assert_eq!(page.get_total_count(), 0);
    assert_visible_rows(&page, 0);
    center
        .publish(MessageDraft::new("After clear", "Fixture").desktop(false))
        .unwrap();
    apply_page_snapshot(&page, center.snapshot());
    assert_visible_rows(&page, 1);
    assert_eq!(page.get_messages(), model);
    assert_eq!(
        page.get_messages().row_data(0).unwrap().title,
        "After clear"
    );
    bridge.commands.shutdown().unwrap();
    center.shutdown();
}
