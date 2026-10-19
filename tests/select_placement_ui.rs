//! Context-free geometry and interaction of the production shared Select.

use i_slint_backend_testing::ElementHandle;
use slint::{
    ComponentHandle, Global, LogicalSize,
    platform::{Key, PointerEventButton, WindowEvent},
};
use std::time::Duration;

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    #[library_path(material) = "material-1.18.1/material.slint"]
    import { GitHubSelectPopup, GitHubSelectOption, GitHubSelectState } from "ui/components/input/github_select.slint";
    import { GitHubSettingsSectionMenuButton } from "ui/components/settings/github_settings_controls.slint";
    import { GitHubScrollView } from "ui/components/github_scroll_view.slint";
    import { TransientOverlayState } from "ui/state/transient_overlay_state.slint";
    import { I18n } from "@i18n";
    export { GitHubSelectState, TransientOverlayState }
    export component SelectPlacementHarness inherits Window {
        width: 900px; height: 700px;
        in property <length> host-x: 0px;
        in property <length> host-y: 0px;
        in property <length> viewport-x: 0px;
        in property <length> viewport-y: 0px;
        in property <length> viewport-width: 800px;
        in property <length> viewport-height: 600px;
        in property <length> trigger-left: 24px;
        in property <length> trigger-top: 64px;
        in property <int> option-count: 3;
        in-out property <int> selection: 0;
        trigger := GitHubSettingsSectionMenuButton {
            x: root.host-x + root.viewport-x + root.trigger-left;
            y: root.host-y + root.viewport-y + root.trigger-top;
            width: 220px; text: I18n.common-save;
            open: GitHubSelectState.open;
            restore-focus-on-close: GitHubSelectState.restore-focus;
            clicked => { GitHubSelectState.toggle("geometry-fixture", "", root.selection,
                self.absolute-position.x, self.absolute-position.y, self.width, self.height); }
        }
        Rectangle {
            x: root.host-x; y: root.host-y;
            width: root.width - self.x; height: root.height - self.y;
            popup := GitHubSelectPopup {
                owner: "geometry-fixture";
                viewport-x: root.viewport-x; viewport-y: root.viewport-y;
                viewport-width: root.viewport-width; viewport-height: root.viewport-height;
                option-count: root.option-count;
                selected(index) => { root.selection = index; GitHubSelectState.close(true); }
                for index in root.option-count: GitHubSelectOption {
                    x: 0px; y: index * 32px; width: popup.option-width;
                    text: "" + index;
                    active: GitHubSelectState.open && GitHubSelectState.owner == popup.owner
                        && GitHubSelectState.active-index == index;
                    selected: root.selection == index;
                    activated => { popup.selected(index); }
                }
            }
        }
    }

    // No Dialog or feature state: a normal scrolling document dismisses a
    // stale anchored menu whenever its viewport moves.
    export component ClippedSelectHarness inherits Window {
        width: 800px; height: 600px;
        in-out property <length> scroll-offset: 0px;
        in-out property <int> selection: 0;
        out property <int> click-count: 0;
        callback reset-pointer-focus();
        reset-pointer-focus => { neutral-focus.focus(); }
        neutral-focus := FocusScope {
            x: 0px; y: 0px; width: 1px; height: 1px;
            focus-on-click: false; focus-on-tab-navigation: false;
        }
        body := GitHubScrollView {
            x: 80px; y: 100px; width: 560px; height: 240px;
            viewport_y <=> root.scroll-offset;
            changed viewport_y => { GitHubSelectState.close(false); }
            Rectangle {
                width: body.visible_width; height: 800px;
                trigger-row := Rectangle {
                    x: 24px; y: 300px; width: 440px; height: 40px;
                    trigger := GitHubSettingsSectionMenuButton {
                        x: 0px; y: 0px; width: parent.width;
                        text: I18n.common-save;
                        open: GitHubSelectState.open && GitHubSelectState.owner == "clipped-fixture";
                        restore-focus-on-close: GitHubSelectState.restore-focus;
                        clicked => {
                            root.click-count += 1;
                            GitHubSelectState.toggle("clipped-fixture", "", root.selection,
                                trigger-row.absolute-position.x, trigger-row.absolute-position.y,
                                trigger-row.width, trigger-row.height);
                        }
                    }
                }
            }
        }
        popup := GitHubSelectPopup {
            owner: "clipped-fixture"; option-count: 3;
            selected(index) => { root.selection = index; GitHubSelectState.close(true); }
            for index in 3: GitHubSelectOption {
                x: 0px; y: index * 32px; width: popup.option-width;
                text: "" + index;
                active: GitHubSelectState.open && GitHubSelectState.owner == popup.owner
                    && GitHubSelectState.active-index == index;
                selected: root.selection == index;
                activated => { popup.selected(index); }
            }
        }
    }
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
}
fn find(window: &impl ComponentHandle, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("visible production element {id}"))
}
fn click_trigger(window: &SelectPlacementHarness) {
    find(window, "SelectPlacementHarness::trigger").mock_single_click(PointerEventButton::Left);
    settle();
}
fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.6,
        "actual {actual}, expected {expected}"
    );
}

#[test]
fn shared_select_uses_the_side_with_more_visible_space_for_short_and_tall_menus() {
    i_slint_backend_testing::init_no_event_loop();
    let window = SelectPlacementHarness::new().unwrap();
    window.window().set_size(LogicalSize::new(900.0, 700.0));
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        for (host_x, host_y, viewport_x, viewport_y, width, height, left, top) in [
            (0.0, 0.0, 0.0, 0.0, 800.0, 600.0, 24.0, 64.0),
            // A short menu fits below, but there is more visible space above.
            (0.0, 0.0, 0.0, 0.0, 800.0, 600.0, 24.0, 350.0),
            (0.0, 0.0, 0.0, 0.0, 800.0, 600.0, 24.0, 280.0),
            (47.0, 31.0, 17.0, 29.0, 720.0, 520.0, 24.0, 320.0),
            (47.0, 31.0, 17.0, 29.0, 240.0, 180.0, 180.0, 70.0),
            (47.0, 31.0, 17.0, 29.0, 720.0, 520.0, 24.0, 0.0),
            (47.0, 31.0, 17.0, 29.0, 720.0, 520.0, 24.0, 480.0),
        ] {
            window.set_host_x(host_x);
            window.set_host_y(host_y);
            window.set_viewport_x(viewport_x);
            window.set_viewport_y(viewport_y);
            window.set_viewport_width(width);
            window.set_viewport_height(height);
            window.set_trigger_left(left);
            window.set_trigger_top(top);
            for count in [1, 3, 50] {
                window.set_option_count(count);
                window.set_selection(0);
                settle();
                click_trigger(&window);
                let trigger = find(&window, "SelectPlacementHarness::trigger");
                let menu = find(&window, "GitHubSelectPopup::menu");
                let t = trigger.absolute_position();
                let p = menu.absolute_position();
                let s = menu.size();
                let above = (top - 12.0_f32).max(0.0);
                let below = (height - top - 40.0 - 12.0_f32).max(0.0);
                let expected_height = 264.0_f32
                    .min(above.max(below))
                    .min(count as f32 * 32.0 + 8.0);
                near(s.height, expected_height);
                near(
                    p.y,
                    if above > below {
                        t.y - expected_height - 4.0
                    } else {
                        t.y + 44.0
                    },
                );
                assert!(p.x >= host_x + viewport_x + 7.5 && p.y >= host_y + viewport_y + 7.5);
                assert!(p.x + s.width <= host_x + viewport_x + width - 7.5);
                assert!(p.y + s.height <= host_y + viewport_y + height - 7.5);
                let state = GitHubSelectState::get(&window);
                let surface = state.get_surface();
                near(surface.x, p.x);
                near(surface.y, p.y);
                near(surface.width, s.width);
                near(surface.height, s.height);
                click_trigger(&window);
                assert!(!state.get_open());
            }
        }
    }
}

fn press(window: &impl ComponentHandle, key: Key) {
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.into() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text: key.into() });
    settle();
}

#[test]
fn shared_select_keeps_internal_scrolling_keyboard_pointer_and_dismissal() {
    i_slint_backend_testing::init_no_event_loop();
    let window = SelectPlacementHarness::new().unwrap();
    window.set_trigger_top(350.0);
    window.set_option_count(50);
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        click_trigger(&window);
        press(&window, Key::End);
        let menu = find(&window, "GitHubSelectPopup::menu");
        let last = ElementHandle::find_by_accessible_label(&window, "49")
            .next()
            .unwrap();
        assert!(last.absolute_position().y >= menu.absolute_position().y + 3.5);
        assert!(
            last.absolute_position().y + last.size().height
                <= menu.absolute_position().y + menu.size().height - 3.5
        );
        press(&window, Key::Return);
        assert_eq!(window.get_selection(), 49);
        assert!(!GitHubSelectState::get(&window).get_open());
        click_trigger(&window);
        find(&window, "GitHubActionMenu::menu-scroll").scroll(0.0, 96.0);
        settle();
        assert!(
            GitHubSelectState::get(&window).get_open(),
            "scrolling menu options keeps it open"
        );
        press(&window, Key::Home);
        assert_eq!(
            GitHubSelectState::get(&window).get_active_index(),
            0,
            "Home must reach the menu after scrolling its focused option out of view"
        );
        press(&window, Key::Return);
        assert_eq!(window.get_selection(), 0);
        click_trigger(&window);
        ElementHandle::find_by_accessible_label(&window, "2")
            .next()
            .unwrap()
            .mock_single_click(PointerEventButton::Left);
        settle();
        assert_eq!(window.get_selection(), 2);
        click_trigger(&window);
        press(&window, Key::Escape);
        let state = GitHubSelectState::get(&window);
        assert!(!state.get_open() && state.get_restore_focus());
        assert_eq!(window.get_selection(), 2);
        click_trigger(&window);
        TransientOverlayState::get(&window).invoke_pointer_pressed(899.0, 699.0);
        settle();
        assert!(!state.get_open() && !state.get_restore_focus());
    }
}

fn click_visible_trigger(window: &ClippedSelectHarness) {
    let body = find(window, "ClippedSelectHarness::body");
    let trigger = find(window, "ClippedSelectHarness::trigger");
    let top = body
        .absolute_position()
        .y
        .max(trigger.absolute_position().y);
    let bottom = (body.absolute_position().y + body.size().height)
        .min(trigger.absolute_position().y + trigger.size().height);
    assert!(bottom > top, "click a real visible intersection");
    let position = slint::LogicalPosition::new(
        trigger.absolute_position().x + trigger.size().width / 2.0,
        (top + bottom) / 2.0,
    );
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    TransientOverlayState::get(window).invoke_pointer_pressed(position.x, position.y);
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
}

#[test]
fn shared_select_opens_from_a_partially_clipped_trigger_without_pointer_focus_scrolling() {
    i_slint_backend_testing::init_no_event_loop();
    let window = ClippedSelectHarness::new().unwrap();
    window.window().set_size(LogicalSize::new(800.0, 600.0));
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        for (visible, clipped_at_top) in [
            (4.0, false),
            (16.0, false),
            (24.0, false),
            (4.0, true),
            (16.0, true),
            (24.0, true),
            (40.0, false),
        ] {
            let state = GitHubSelectState::get(&window);
            state.invoke_close(false);
            window.invoke_reset_pointer_focus();
            window.set_selection(0);
            let offset = if clipped_at_top {
                visible - 340.0
            } else {
                -60.0 - visible
            };
            window.set_scroll_offset(offset);
            settle();
            let body = find(&window, "ClippedSelectHarness::body");
            let trigger = find(&window, "ClippedSelectHarness::trigger");
            let top = body
                .absolute_position()
                .y
                .max(trigger.absolute_position().y);
            let bottom = (body.absolute_position().y + body.size().height)
                .min(trigger.absolute_position().y + trigger.size().height);
            near(bottom - top, visible);
            let anchor = trigger.absolute_position();
            let clicks = window.get_click_count();
            click_visible_trigger(&window);
            for elapsed in [0, 16, 100] {
                i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(elapsed));
                assert_eq!(
                    window.get_click_count(),
                    clicks + 1,
                    "one visible click opens once"
                );
                assert!(
                    (window.get_scroll_offset() - offset).abs() < 0.5,
                    "pointer opening must not reveal the full trigger: {visible}px visible, top={clipped_at_top}"
                );
                assert!(
                    state.get_open(),
                    "a clipped trigger must not open and immediately dismiss"
                );
                near(state.get_anchor_y(), anchor.y);
                let menu = find(&window, "GitHubSelectPopup::menu");
                if clipped_at_top {
                    assert!(menu.absolute_position().y >= anchor.y + 43.5);
                } else {
                    assert!(
                        menu.absolute_position().y + menu.size().height <= anchor.y - 3.5,
                        "lower-window menu opens upward even when the trigger is clipped"
                    );
                }
            }
            ElementHandle::find_by_accessible_label(&window, "1")
                .find(|option| option.accessible_checked().is_some())
                .unwrap()
                .mock_single_click(PointerEventButton::Left);
            settle();
            assert_eq!(window.get_selection(), 1);
            assert!(!state.get_open());
            // Selection restores the trigger's keyboard focus. Revealing it
            // after the menu closes is allowed; opening by pointer must not.
            press(&window, Key::DownArrow);
            assert!(
                state.get_open(),
                "restored trigger focus supports keyboard reopening"
            );
            press(&window, Key::End);
            press(&window, Key::Return);
            assert_eq!(window.get_selection(), 2);
            assert!(!state.get_open());
            press(&window, Key::DownArrow);
            assert!(state.get_open());
            press(&window, Key::Escape);
            assert!(!state.get_open());
            assert_eq!(window.get_selection(), 2);
            press(&window, Key::DownArrow);
            assert!(state.get_open());
            window
                .window()
                .dispatch_event(WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(
                        body.absolute_position().x + 12.0,
                        body.absolute_position().y + 12.0,
                    ),
                    delta_x: 0.0,
                    delta_y: -16.0,
                });
            settle();
            assert!(
                !state.get_open(),
                "actual document scrolling still dismisses the old anchor"
            );
        }
    }
}
