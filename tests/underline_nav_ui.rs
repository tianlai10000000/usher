use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::Duration;

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    import { GitHubUnderlineNav, GitHubTabItem, GitHubTabOverflowPopup, GitHubTabOverflowState }
        from "ui/components/navigation/github_underline_nav.slint";
    import { GitHubSelectState } from "ui/components/input/github_select.slint";
    export { GitHubTabItem, GitHubTabOverflowState, GitHubSelectState }
    export component TabHarness inherits Window {
        width: 900px;
        height: 400px;
        in property <length> container-width: 900px;
        in property <[GitHubTabItem]> items;
        in-out property <int> selected: 0;
        out property <int> visible-count: nav.visible-count;
        out property <bool> overflowing: nav.overflowing;
        nav := GitHubUnderlineNav {
            x: 0px;
            y: 0px;
            width: root.container-width;
            owner: "test-tabs";
            group-label: "Review";
            items: root.items;
            selected-index: root.selected;
            selected(index) => { root.selected = index; }
        }
        GitHubTabOverflowPopup {}
    }
}

fn settle() {
    for _ in 0..3 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

#[test]
fn additional_tabs_and_large_counts_overflow_as_whole_items() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = TabHarness::new().unwrap();
    ui.set_items(ModelRc::new(VecModel::from(
        [0, 10, 99, 100, 1000]
            .into_iter()
            .enumerate()
            .map(|(index, count)| GitHubTabItem {
                label: format!("Review section {index}").into(),
                count,
                show_count: true,
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    ui.show().unwrap();
    for width in [1100.0, 500.0, 280.0, 1100.0, 500.0] {
        ui.set_container_width(width);
        settle();
        let visible = ui.get_visible_count();
        if width > 1000.0 {
            assert!(!ui.get_overflowing());
            assert_eq!(visible, 5);
        } else {
            assert!(
                ui.get_overflowing(),
                "width={width}, actual={:?}, tabs={:?}",
                ui.window().size(),
                ElementHandle::find_by_element_id(&ui, "GitHubUnderlineNav::tab")
                    .map(|e| (e.absolute_position(), e.size()))
                    .collect::<Vec<_>>()
            );
            assert!(visible > 0 && visible < 5, "visible {visible} at {width}");
            let more = ElementHandle::find_by_element_id(&ui, "GitHubUnderlineNav::more")
                .next()
                .unwrap();
            let last_index = visible as usize - 1;
            let last = ElementHandle::find_by_accessible_label(
                &ui,
                &format!(
                    "Review section {last_index}, {}",
                    [0, 10, 99, 100, 1000][last_index]
                ),
            )
            .next()
            .expect("last visible tab");
            assert!(last.absolute_position().x + last.size().width <= more.absolute_position().x);
            assert!(more.absolute_position().x + more.size().width <= width - 16.0);
            more.mock_single_click(slint::platform::PointerEventButton::Left);
            settle();
            assert!(ui.global::<GitHubSelectState>().get_open());
            assert_eq!(ui.global::<GitHubTabOverflowState>().get_first(), visible);
            let option = ElementHandle::find_by_accessible_label(&ui, "Review section 4, 1000")
                .find(|e| e.absolute_position().y >= 48.0)
                .expect("overflow menu option");
            option.mock_single_click(slint::platform::PointerEventButton::Left);
            settle();
            assert_eq!(ui.get_selected(), 4);
            assert!(!ui.global::<GitHubSelectState>().get_open());
            more.mock_single_click(slint::platform::PointerEventButton::Left);
            settle();
            ui.window().dispatch_event(WindowEvent::KeyPressed {
                text: Key::Escape.into(),
            });
            settle();
            assert!(
                !ui.global::<GitHubSelectState>().get_open(),
                "Escape dismisses More"
            );
            more.mock_single_click(slint::platform::PointerEventButton::Left);
            settle();
            ui.global::<GitHubSelectState>()
                .invoke_pointer_released(4.0, 350.0);
            assert!(
                !ui.global::<GitHubSelectState>().get_open(),
                "outside pointer dismisses More"
            );
        }
    }
    ui.set_container_width(1100.0);
    settle();
    let counts: Vec<_> = ElementHandle::find_by_element_id(&ui, "GitHubCounterLabel::counter-text")
        .filter_map(|e| e.accessible_label())
        .map(|s| s.to_string())
        .collect();
    assert!(counts.iter().any(|s| s == "10"));
    assert!(counts.iter().any(|s| s == "99"));
    assert!(counts.iter().filter(|s| s.as_str() == "99+").count() >= 2);
    assert!(!counts.iter().any(|s| s == "100" || s == "1000"));
    let full = ElementHandle::find_by_accessible_label(&ui, "Review section 4, 1000").next();
    assert!(full.is_some(), "full count is retained for accessibility");

    ui.set_items(ModelRc::new(VecModel::from(vec![
        GitHubTabItem {
            label: "First section".into(),
            ..Default::default()
        },
        GitHubTabItem {
            label: "Another section".into(),
            ..Default::default()
        },
        GitHubTabItem {
            label: "Disabled section".into(),
            disabled: true,
            ..Default::default()
        },
    ])));
    ui.set_selected(0);
    ui.set_container_width(280.0);
    settle();
    let more = ElementHandle::find_by_element_id(&ui, "GitHubUnderlineNav::more")
        .next()
        .unwrap();
    more.mock_single_click(slint::platform::PointerEventButton::Left);
    settle();
    let disabled = ElementHandle::find_by_accessible_label(&ui, "Disabled section")
        .next()
        .unwrap();
    assert_eq!(disabled.accessible_enabled(), Some(false));
    disabled.mock_single_click(slint::platform::PointerEventButton::Left);
    assert_eq!(ui.get_selected(), 0);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::End.into(),
    });
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Return.into(),
    });
    assert_eq!(
        ui.get_selected(),
        0,
        "disabled menu entry cannot commit by keyboard"
    );
    ui.set_container_width(1100.0);
    settle();
    assert!(
        !ui.global::<GitHubSelectState>().get_open(),
        "resize dismisses stale menu"
    );
}
