//! Shared popovers observe real presses without taking the target's click.

use i_slint_backend_testing::ElementHandle;
use slint::{
    ComponentHandle, Global, LogicalPosition,
    platform::{PointerEventButton, WindowEvent},
};
use std::time::Duration;

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    #[library_path(material) = "material-1.18.1/material.slint"]
    import { GitHubActionMenu, GitHubActionMenuTriggerArea } from "ui/components/github_action_menu.slint";
    import { GitHubDiffSettingsPopover } from "ui/components/pr_review/content.slint";
    import { GitHubDialog } from "ui/components/dialog/github_dialog.slint";
    import { GitHubToast } from "ui/components/toast/toast.slint";
    import { GitHubTheme } from "ui/theme/github_theme.slint";
    import { TransientOverlayState } from "ui/state/transient_overlay_state.slint";
    import { I18n } from "@i18n";
    export { TransientOverlayState }

    export component PopoverDismissHarness inherits Window {
        width: 800px; height: 600px;
        in-out property <bool> first-open: false;
        in-out property <bool> second-open: false;
        in-out property <bool> diff-open: false;
        in-out property <length> host-x: 80px;
        in-out property <string> menu-text: "selection inside the menu";
        in-out property <string> outside-text: "outside";
        out property <bool> menu-input-focused: false;
        out property <bool> outside-input-focused: outside-input.has-focus;
        out property <bool> target-focused: outside-target.has-focus;
        out property <int> target-clicks: 0;
        in-out property <bool> pending-ignore-whitespace: false;
        in-out property <bool> dialog-open: false;
        in-out property <bool> toast-open: false;

        Rectangle {
            x: root.host-x; y: 60px; width: 650px; height: 450px;
            first-trigger := GitHubActionMenuTriggerArea {
                x: 24px; y: 24px; width: 120px; height: 32px;
                menu-open: root.first-open;
                request-open => { root.first-open = true; }
                request-close => { root.first-open = false; }
            }
            second-trigger := GitHubActionMenuTriggerArea {
                x: 180px; y: 24px; width: 120px; height: 32px;
                menu-open: root.second-open;
                request-open => { root.second-open = true; }
                request-close => { root.second-open = false; }
            }
            diff-trigger := GitHubActionMenuTriggerArea {
                x: 360px; y: 24px; width: 32px; height: 32px;
                menu-open: root.diff-open;
                request-open => { root.diff-open = true; }
                request-close => { root.diff-open = false; }
            }

            // Conditional and persistent callers share the production behavior.
            if root.first-open: first-menu := GitHubActionMenu {
                open: true;
                viewport-width: 650px; viewport-height: 450px;
                anchor-x: 24px; anchor-y: 24px;
                anchor-width: 120px; anchor-height: 32px;
                menu-width: 220px; content-height: 140px;
                dismiss-requested => { root.first-open = false; }
                menu-input := TextInput {
                    x: 8px; y: 8px; width: 204px; height: 32px;
                    text <=> root.menu-text;
                    single-line: true;
                    changed has-focus => { root.menu-input-focused = self.has-focus; }
                }
            }
            second-menu := GitHubActionMenu {
                open: root.second-open;
                viewport-width: 650px; viewport-height: 450px;
                anchor-x: 180px; anchor-y: 24px;
                anchor-width: 120px; anchor-height: 32px;
                menu-width: 180px; content-height: 80px;
                dismiss-requested => { root.second-open = false; }
            }
            diff-menu := GitHubDiffSettingsPopover {
                x: 300px; y: 64px;
                open: root.diff-open;
                caret-x: 76px;
                dismiss-trigger-bounds: {
                    x: diff-trigger.absolute-position.x, y: diff-trigger.absolute-position.y,
                    width: diff-trigger.width, height: diff-trigger.height,
                };
                pending-ignore-whitespace <=> root.pending-ignore-whitespace;
                ignore-whitespace-supported: true;
                bg-default: GitHubTheme.canvas-default;
                bg-hover: GitHubTheme.control-raised-hover;
                bg-muted: GitHubTheme.canvas-subtle;
                border-default: GitHubTheme.border-default;
                accent-fg: GitHubTheme.accent-fg;
                fg-default: GitHubTheme.fg-default;
                fg-muted: GitHubTheme.fg-muted;
                addition-color: GitHubTheme.diff-addition-stat;
                deletion-color: GitHubTheme.diff-deletion-stat;
                dismiss-requested => { root.diff-open = false; }
                apply(view, ignore) => { root.diff-open = false; }
            }
        }

        outside-scroll := Flickable {
            x: 620px; y: 80px; width: 160px; height: 160px;
            content-width: 160px; content-height: 480px;
            Rectangle { width: 160px; height: 480px; }
        }
        outside-input := TextInput {
            x: 540px; y: 350px; width: 200px; height: 32px;
            text <=> root.outside-text;
            single-line: true;
        }
        outside-target := FocusScope {
            x: 540px; y: 420px; width: 200px; height: 40px;
            focus-on-click: false;
            TouchArea {
                clicked => {
                    outside-target.focus();
                    root.target-clicks += 1;
                }
            }
        }

        GitHubDialog {
            dialog-visible: root.dialog-open;
            modal: false;
            dismiss-on-backdrop: true;
            title: I18n.common-save;
            close-requested => { root.dialog-open = false; }
        }
        GitHubToast {
            z: 4000;
            message: I18n.common-save;
            type: "text";
            toast-visible: root.toast-open;
            overlay: true;
            close-on-click: true;
            dismiss => { root.toast-open = false; }
        }
    }
}

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(20));
}

fn window() -> PopoverDismissHarness {
    i_slint_backend_testing::init_no_event_loop();
    let window = PopoverDismissHarness::new().expect("shared popover harness");
    window.show().expect("show shared popover harness");
    settle();
    window
}

fn find(window: &PopoverDismissHarness, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(window, id)
        .next()
        .unwrap_or_else(|| panic!("visible production element {id}"))
}

fn center(element: &ElementHandle) -> LogicalPosition {
    let position = element.absolute_position();
    let size = element.size();
    LogicalPosition::new(
        position.x + size.width / 2.0,
        position.y + size.height / 2.0,
    )
}

fn press(window: &PopoverDismissHarness, position: LogicalPosition) {
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    // This local harness has no MainWindow hook. Publish after actual Slint
    // dispatch, in the same order used by the application input owner.
    TransientOverlayState::get(window).invoke_pointer_pressed(position.x, position.y);
    settle();
}

fn release(window: &PopoverDismissHarness, position: LogicalPosition) {
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    settle();
}

fn click(window: &PopoverDismissHarness, position: LogicalPosition) {
    press(window, position);
    release(window, position);
}

fn click_id(window: &PopoverDismissHarness, id: &str) {
    click(window, center(&find(window, id)));
}

#[test]
fn shared_action_menu_closes_on_consumed_press_and_preserves_the_first_target_click() {
    let window = window();
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        click_id(&window, "PopoverDismissHarness::first-trigger");
        assert!(window.get_first_open());
        click_id(&window, "PopoverDismissHarness::second-trigger");
        assert!(!window.get_first_open() && window.get_second_open());
        click_id(&window, "PopoverDismissHarness::first-trigger");
        assert!(window.get_first_open() && !window.get_second_open());

        let clicks = window.get_target_clicks();
        click_id(&window, "PopoverDismissHarness::outside-target");
        assert!(!window.get_first_open());
        assert_eq!(window.get_target_clicks(), clicks + 1);
        assert!(
            window.get_target_focused(),
            "outside focus must not be restored to the trigger"
        );
        assert!(!TransientOverlayState::get(&window).get_closed_open_menu());

        click_id(&window, "PopoverDismissHarness::first-trigger");
        click_id(&window, "PopoverDismissHarness::outside-scroll");
        assert!(
            !window.get_first_open(),
            "Flickable consumes the press but cannot hide it from dismissal"
        );
    }
}

#[test]
fn shared_action_menu_keeps_interior_input_drag_and_outside_input_focus() {
    let window = window();
    click_id(&window, "PopoverDismissHarness::first-trigger");
    let input = find(&window, "PopoverDismissHarness::menu-input");
    press(&window, center(&input));
    assert!(window.get_first_open() && window.get_menu_input_focused());
    let end = LogicalPosition::new(760.0, 520.0);
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position: end });
    release(&window, end);
    assert!(
        window.get_first_open(),
        "a release is not a new outside press"
    );
    assert!(window.get_menu_input_focused());

    click_id(&window, "PopoverDismissHarness::outside-input");
    assert!(!window.get_first_open());
    assert!(
        window.get_outside_input_focused(),
        "the actual outside input retains its first click and focus"
    );
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: "z".into() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text: "z".into() });
    assert!(window.get_outside_text().contains('z'));
}

#[test]
fn conditional_keyboard_open_ignores_old_presses_and_uses_live_nested_trigger_bounds() {
    let window = window();
    let state = TransientOverlayState::get(&window);
    state.invoke_pointer_pressed(799.0, 599.0);
    settle();
    window.set_first_open(true);
    settle();
    assert!(
        window.get_first_open(),
        "initial mount must not replay the old outside press"
    );
    click_id(&window, "PopoverDismissHarness::first-trigger");
    assert!(
        !window.get_first_open(),
        "the real nested trigger toggles once"
    );

    // A persistent guard must also ignore a closed-state sample when opening
    // queues before that sample's changed callback has run.
    state.invoke_pointer_pressed(799.0, 599.0);
    window.set_second_open(true);
    settle();
    assert!(
        window.get_second_open(),
        "persistent open cannot replay a queued old press"
    );
    let old_trigger = center(&find(&window, "PopoverDismissHarness::second-trigger"));
    window.set_host_x(280.0);
    settle();
    press(&window, old_trigger);
    assert!(
        !window.get_second_open(),
        "trigger bounds follow the moved parent"
    );
    release(&window, old_trigger);
}

#[test]
fn shared_diff_settings_uses_the_same_guard_without_changing_interior_controls() {
    let window = window();
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        click_id(&window, "PopoverDismissHarness::diff-trigger");
        assert!(window.get_diff_open());
        let ignore = window.get_pending_ignore_whitespace();
        click_id(&window, "GitHubDiffSettingsPopover::whitespace-touch");
        assert!(window.get_diff_open());
        assert_ne!(window.get_pending_ignore_whitespace(), ignore);
        click_id(&window, "PopoverDismissHarness::diff-trigger");
        assert!(!window.get_diff_open(), "the trigger closes once");
        click_id(&window, "PopoverDismissHarness::diff-trigger");
        let clicks = window.get_target_clicks();
        click_id(&window, "PopoverDismissHarness::outside-target");
        assert!(!window.get_diff_open());
        assert_eq!(window.get_target_clicks(), clicks + 1);
    }
}

#[test]
fn outside_pointer_observation_keeps_dialog_and_toast_dismissal_channels_separate() {
    let window = window();
    window.set_first_open(true);
    window.set_dialog_open(true);
    window.set_toast_open(true);
    settle();
    find(&window, "PopoverDismissHarness::first-menu");
    let state = TransientOverlayState::get(&window);
    let dismiss_revision = state.get_dismiss_revision();
    let popover_revision = state.get_popover_dismiss_revision();
    state.invoke_pointer_pressed(799.0, 599.0);
    settle();
    assert!(!window.get_first_open());
    assert!(window.get_dialog_open() && window.get_toast_open());
    assert_eq!(state.get_dismiss_revision(), dismiss_revision);
    assert_eq!(state.get_popover_dismiss_revision(), popover_revision);

    state.invoke_dismiss_all();
    settle();
    assert!(
        !window.get_dialog_open() && !window.get_toast_open(),
        "explicit chrome dismissal remains the positive control"
    );
}
