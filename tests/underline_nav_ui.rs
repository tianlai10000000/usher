use i_slint_backend_testing::ElementHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::Duration;

slint::slint! {
    #[library_path(i18n) = "target/slint-macro-tests/usher-i18n/i18n.slint"]
    import { GitHubUnderlineNav, GitHubUnderlineNavVariant, GitHubTabItem, GitHubTabOverflowPopup, GitHubTabOverflowState }
        from "ui/components/navigation/github_underline_nav.slint";
    import { GitHubFixedThreeTabs } from "ui/components/navigation/github_fixed_tabs.slint";
    import { GitHubSelectState } from "ui/components/input/github_select.slint";
    import { I18n } from "@i18n";
    export { GitHubUnderlineNavVariant, GitHubTabItem, GitHubTabOverflowState, GitHubSelectState }
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

    export component VariantHarness inherits Window {
        width: 900px;
        height: 300px;
        in property <length> container-width: 700px;
        in property <GitHubUnderlineNavVariant> variant: inset;
        in-out property <int> dynamic-selected: 0;
        in-out property <int> fixed-selected: 0;
        out property <length> dynamic-natural-width: dynamic.natural-width;
        out property <length> fixed-natural-width: fixed.natural-width;
        out property <bool> dynamic-overflowing: dynamic.overflowing;
        out property <bool> fixed-overflowing: fixed.overflowing;
        out property <string> first-label: I18n.navigation-home;
        private property <GitHubTabItem> first: { label: I18n.navigation-home };
        private property <GitHubTabItem> second: { label: I18n.navigation-chat };
        private property <GitHubTabItem> third: { label: I18n.settings-code-hosting-title };
        dynamic := GitHubUnderlineNav {
            x: 20px; y: 0px; width: root.container-width;
            owner: "variant-dynamic";
            variant: root.variant;
            items: [root.first, root.second, root.third];
            selected-index: root.dynamic-selected;
            selected(index) => { root.dynamic-selected = index; }
        }
        fixed := GitHubFixedThreeTabs {
            x: 20px; y: 64px; width: root.container-width;
            owner: "variant-fixed";
            variant: root.variant;
            first-item: root.first; second-item: root.second; third-item: root.third;
            selected-index: root.fixed-selected;
            selected(index) => { root.fixed-selected = index; }
        }
        custom-dynamic := GitHubUnderlineNav {
            x: 20px; y: 128px; width: root.container-width;
            owner: "custom-dynamic";
            variant: root.variant;
            content-inset: 11px;
            items: [root.first, root.second, root.third];
        }
        custom-fixed := GitHubFixedThreeTabs {
            x: 20px; y: 192px; width: root.container-width;
            owner: "custom-fixed";
            variant: root.variant;
            content-inset: 11px;
            first-item: root.first; second-item: root.second; third-item: root.third;
        }
    }
}

fn settle() {
    for _ in 0..3 {
        i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(16));
    }
}

fn variant_nav(ui: &VariantHarness, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(ui, &format!("VariantHarness::{id}"))
        .next()
        .unwrap_or_else(|| panic!("variant navigation {id}"))
}

fn nav_tabs(nav: &ElementHandle) -> Vec<ElementHandle> {
    let mut tabs = nav
        .query_descendants()
        .match_inherits("GitHubUnderlineNavItem")
        .find_all();
    tabs.sort_by(|left, right| {
        left.absolute_position()
            .x
            .total_cmp(&right.absolute_position().x)
    });
    tabs
}

fn assert_near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.5, "{actual} != {expected}");
}

#[test]
fn inset_and_flush_preserve_item_geometry_and_explicit_insets_in_both_locales() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = VariantHarness::new().unwrap();
    ui.show().unwrap();
    settle();
    for id in ["dynamic", "fixed"] {
        let nav = variant_nav(&ui, id);
        assert_near(
            nav_tabs(&nav)[0].absolute_position().x - nav.absolute_position().x,
            16.0,
        );
    }
    let fixed_first = nav_tabs(&variant_nav(&ui, "fixed"))[0].clone();

    for locale in ["en", "zh-CN", "en"] {
        slint::select_bundled_translation(locale).unwrap();
        ui.set_container_width(700.0);
        ui.set_variant(GitHubUnderlineNavVariant::Inset);
        settle();
        assert!(
            fixed_first.is_valid(),
            "locale changes keep fixed leaves mounted"
        );
        assert_eq!(fixed_first.accessible_label(), Some(ui.get_first_label()));
        let natural = ui.get_dynamic_natural_width();
        assert_near(natural, ui.get_fixed_natural_width());
        let mut widths = Vec::new();
        for id in ["dynamic", "fixed"] {
            let nav = variant_nav(&ui, id);
            let tabs = nav_tabs(&nav);
            assert_eq!(tabs.len(), 3);
            assert_near(
                tabs[0].absolute_position().x - nav.absolute_position().x,
                16.0,
            );
            widths.push(tabs.iter().map(|tab| tab.size().width).collect::<Vec<_>>());
        }
        ui.set_variant(GitHubUnderlineNavVariant::Flush);
        settle();
        assert_near(ui.get_dynamic_natural_width(), natural - 32.0);
        assert_near(ui.get_fixed_natural_width(), natural - 32.0);
        for (id, expected_widths) in ["dynamic", "fixed"].into_iter().zip(widths) {
            let nav = variant_nav(&ui, id);
            let tabs = nav_tabs(&nav);
            assert_eq!(tabs.len(), 3);
            assert_near(tabs[0].absolute_position().x, nav.absolute_position().x);
            for (index, tab) in tabs.iter().enumerate() {
                assert_near(tab.size().width, expected_widths[index]);
                let text = tab
                    .query_descendants()
                    .match_type_name("Text")
                    .find_first()
                    .unwrap();
                assert_near(text.absolute_position().x - tab.absolute_position().x, 8.0);
                assert_near(tab.size().width - text.size().width, 16.0);
                if index > 0 {
                    let previous = &tabs[index - 1];
                    assert_near(
                        tab.absolute_position().x
                            - previous.absolute_position().x
                            - previous.size().width,
                        4.0,
                    );
                }
            }
            // A single pointer click and then keyboard navigation both still activate.
            if id == "dynamic" {
                ui.set_dynamic_selected(2);
            } else {
                ui.set_fixed_selected(2);
            }
            settle();
            tabs[0].mock_single_click(PointerEventButton::Left);
            assert_eq!(
                if id == "dynamic" {
                    ui.get_dynamic_selected()
                } else {
                    ui.get_fixed_selected()
                },
                0
            );
            for text in [Key::RightArrow, Key::Return] {
                ui.window()
                    .dispatch_event(WindowEvent::KeyPressed { text: text.into() });
                settle();
            }
            assert_eq!(
                if id == "dynamic" {
                    ui.get_dynamic_selected()
                } else {
                    ui.get_fixed_selected()
                },
                1
            );
        }
        for variant in [
            GitHubUnderlineNavVariant::Inset,
            GitHubUnderlineNavVariant::Flush,
        ] {
            ui.set_variant(variant);
            settle();
            for id in ["custom-dynamic", "custom-fixed"] {
                let nav = variant_nav(&ui, id);
                assert_near(
                    nav_tabs(&nav)[0].absolute_position().x - nav.absolute_position().x,
                    11.0,
                );
            }
        }
    }
}

#[test]
fn flush_uses_both_reclaimed_edges_for_overflow_and_more_placement() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = VariantHarness::new().unwrap();
    ui.show().unwrap();
    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        ui.set_container_width(700.0);
        ui.set_variant(GitHubUnderlineNavVariant::Flush);
        settle();
        let items_width = ui.get_dynamic_natural_width();
        assert_near(items_width, ui.get_fixed_natural_width());
        ui.set_container_width(items_width + 16.0);
        settle();
        assert!(!ui.get_dynamic_overflowing() && !ui.get_fixed_overflowing());
        ui.set_variant(GitHubUnderlineNavVariant::Inset);
        settle();
        assert!(ui.get_dynamic_overflowing() && ui.get_fixed_overflowing());
        ui.set_container_width(items_width + 32.0);
        settle();
        assert!(!ui.get_dynamic_overflowing() && !ui.get_fixed_overflowing());

        ui.set_container_width(items_width - 1.0);
        for (variant, inset) in [
            (GitHubUnderlineNavVariant::Inset, 16.0),
            (GitHubUnderlineNavVariant::Flush, 0.0),
        ] {
            ui.set_variant(variant);
            settle();
            assert!(ui.get_dynamic_overflowing() && ui.get_fixed_overflowing());
            for (id, more_id) in [
                ("dynamic", "GitHubUnderlineNav::more"),
                ("fixed", "GitHubFixedThreeTabs::more"),
            ] {
                let nav = variant_nav(&ui, id);
                let more = nav
                    .query_descendants()
                    .match_id(more_id)
                    .find_first()
                    .unwrap();
                assert_near(
                    more.absolute_position().x + more.size().width,
                    nav.absolute_position().x + ui.get_container_width() - inset,
                );
                for tab in nav_tabs(&nav)
                    .into_iter()
                    .filter(|tab| tab.id().as_deref() != Some(more_id))
                {
                    assert!(
                        tab.absolute_position().x + tab.size().width
                            <= more.absolute_position().x - 3.5
                    );
                }
                more.mock_single_click(PointerEventButton::Left);
                settle();
                assert!(ui.global::<GitHubSelectState>().get_open());
                ui.set_variant(if variant == GitHubUnderlineNavVariant::Inset {
                    GitHubUnderlineNavVariant::Flush
                } else {
                    GitHubUnderlineNavVariant::Inset
                });
                settle();
                assert!(
                    !ui.global::<GitHubSelectState>().get_open(),
                    "changing the inset dismisses the old More anchor without resizing"
                );
                ui.set_variant(variant);
                settle();
            }
        }
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
