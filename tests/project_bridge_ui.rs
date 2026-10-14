//! Production Project page/dialog integration, with an in-memory presentation only.
use i_slint_backend_testing::ElementHandle;
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use usher_lib::{MainWindow, ProjectBridge, ProjectItem, Route};

fn settle() {
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(32));
}

fn click(window: &MainWindow, locale: &str, key: &str) {
    let label = usher_i18n::translate_for(locale, key);
    ElementHandle::find_by_accessible_label(window, label.as_ref())
        .last()
        .unwrap_or_else(|| panic!("missing mounted action {key} ({label})"))
        .mock_single_click(PointerEventButton::Left);
    settle();
}

#[test]
fn project_page_and_root_dialog_share_one_draft_across_modes_and_locales() {
    i_slint_backend_testing::init_no_event_loop();
    let window = MainWindow::new().unwrap();
    window.window().set_size(LogicalSize::new(1_200.0, 900.0));
    window.set_platform_is_macos(false);
    window.set_show_breadcrumb(false);
    window.set_show_navigation_drawer(false);
    window.set_current_route(Route::Project);
    let bridge = window.global::<ProjectBridge>();
    bridge.set_items(ModelRc::new(VecModel::from(vec![ProjectItem {
        project_name: "Synthetic Project".into(),
        project_alias: "synthetic".into(),
        local_repo: "/synthetic/project".into(),
        remote_repo: "https://example.test/synthetic/project.git".into(),
        repo_type: "github".into(),
        is_pr_auto_update: true,
        ..Default::default()
    }])));
    bridge.set_hooks_ready(true);
    window.show().unwrap();
    let updates = Rc::new(RefCell::new(Vec::new()));
    let observed = updates.clone();
    bridge.on_update_project(move |original, _, alias, _, _, _, _, prefetch| {
        observed.borrow_mut().push((original, alias, prefetch));
    });
    let additions = Rc::new(RefCell::new(Vec::new()));
    let observed = additions.clone();
    bridge.on_add_project(move |_, alias, _, _, _, _, prefetch| {
        observed.borrow_mut().push((alias, prefetch));
    });

    for locale in ["en", "zh-CN"] {
        slint::select_bundled_translation(locale).unwrap();
        for mode in ["grid", "list"] {
            bridge.set_view_mode(mode.into());
            settle();
            click(&window, locale, "tooltip.edit_project");
            assert!(
                bridge.get_show_add_modal(),
                "{locale}/{mode}: one pointer click must open the edit dialog"
            );
            assert!(window.get_modal_overlay_visible());
            assert_eq!(bridge.get_modal_mode(), "edit");
            assert_eq!(bridge.get_new_alias(), "synthetic");
            assert!(
                bridge.get_new_pr_auto_update(),
                "{locale}/{mode}: edit must restore the saved prefetch value into the dialog"
            );

            // Switch a mounted dialog, not a newly created route: semantic copy
            // must repaint while the user's draft and model identity stay intact.
            let other_locale = if locale == "en" { "zh-CN" } else { "en" };
            let model = bridge.get_items();
            slint::select_bundled_translation(other_locale).unwrap();
            settle();
            assert!(bridge.get_new_pr_auto_update());
            assert_eq!(bridge.get_items(), model);
            click(&window, other_locale, "common.cancel");
            assert!(!bridge.get_show_add_modal());
            assert!(!window.get_modal_overlay_visible());
            assert!(bridge.get_new_alias().is_empty());
            assert!(!bridge.get_new_pr_auto_update());
            slint::select_bundled_translation(locale).unwrap();

            // Reopen from the actual row/card. Save must dispatch the original
            // flag, not the previous cancelled form's default.
            click(&window, locale, "tooltip.edit_project");
            click(&window, locale, "projects.save_changes");
            assert!(!bridge.get_show_add_modal());
            assert_eq!(
                updates.borrow().last().cloned(),
                Some(("synthetic".into(), "synthetic".into(), true))
            );

            click(&window, locale, "projects.new_project");
            assert_eq!(bridge.get_modal_mode(), "add");
            assert!(bridge.get_new_alias().is_empty());
            assert!(!bridge.get_new_pr_auto_update());
            let additions_before = additions.borrow().len();
            click(&window, locale, "common.cancel");
            assert_eq!(additions.borrow().len(), additions_before);

            click(&window, locale, "projects.new_project");
            bridge.set_new_name("Fresh Project".into());
            bridge.set_new_alias("fresh".into());
            bridge.set_new_local_repo("/synthetic/fresh".into());
            bridge.set_new_remote_repo("https://example.test/fresh.git".into());
            click(&window, locale, "projects.add_project");
            assert_eq!(additions.borrow().len(), additions_before + 1);
            assert_eq!(
                additions.borrow().last().cloned(),
                Some(("fresh".into(), false))
            );
            assert!(!bridge.get_show_add_modal());
        }
    }
    assert_eq!(updates.borrow().len(), 4);
    assert_eq!(additions.borrow().len(), 4);
}
