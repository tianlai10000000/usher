//! Transient project-form selection; durable opt-in belongs to ProjectStateFacade.
use crate::{MainWindow, ProjectHookChoice};
use slint::{ComponentHandle, Model, VecModel};

use super::state::ProjectStateFacade;

pub(super) fn selected_ids(ui: &MainWindow) -> Vec<String> {
    ui.get_project_hook_choices()
        .iter()
        .filter(|row| row.preferred_selected)
        .map(|row| row.id.to_string())
        .collect()
}

pub(super) fn bind(ui: &MainWindow, state: ProjectStateFacade) {
    let weak = ui.as_weak();
    ui.on_project_prepare_hooks(move |alias| {
        let Some(ui) = weak.upgrade() else { return };
        let snapshot = state.snapshot();
        let project = snapshot
            .state()
            .projects
            .iter()
            .find(|project| !alias.is_empty() && project.project_alias == alias.as_str());
        let selected = project
            .map(|project| project.hook_ids.clone())
            .unwrap_or_default();
        ui.set_project_hooks_enabled(project.is_some_and(|project| project.hooks_enabled));
        project_choices(&ui, &selected);
    });
    let weak = ui.as_weak();
    ui.on_project_toggle_hook(move |id, selected| {
        let Some(ui) = weak.upgrade() else { return };
        let choices = ui.get_project_hook_choices();
        if let Some(model) = choices
            .as_any()
            .downcast_ref::<VecModel<ProjectHookChoice>>()
        {
            set_choice_selected(model, id.as_str(), selected);
        }
        update_counts(&ui);
    });
}

fn set_choice_selected(model: &VecModel<ProjectHookChoice>, id: &str, selected: bool) {
    for (index, mut row) in model.iter().enumerate() {
        if row.id == id {
            if row.enabled {
                row.selected = selected;
                row.preferred_selected = selected;
                model.set_row_data(index, row);
            }
            break;
        }
    }
}

fn update_counts(ui: &MainWindow) {
    let rows = ui.get_project_hook_choices();
    ui.set_project_selected_hook_count(
        rows.iter().filter(|row| row.preferred_selected).count() as i32
    );
    ui.set_project_selectable_hook_count(rows.iter().filter(|row| row.enabled).count() as i32);
}

pub(crate) fn refresh_hook_choices(ui: &MainWindow) {
    if ui.get_project_show_add_modal() && ui.get_project_modal_mode() != "delete" {
        project_choices(ui, &selected_ids(ui));
    }
}

fn project_choices(ui: &MainWindow, selected: &[String]) {
    let service = crate::hooks::current();
    let snapshot = service.as_ref().map(|service| service.state.snapshot());
    let state = snapshot.as_ref().map(|snapshot| snapshot.state());
    let ready = state.is_some_and(|state| state.ready);
    ui.set_project_hooks_ready(ready);
    ui.set_project_hooks_status(
        if let Some(state) = state.filter(|state| !state.error.is_empty()) {
            state.error.clone().into()
        } else if !ready {
            crate::i18n::tr("projects.hooks_loading")
                .into_owned()
                .into()
        } else if state.is_some_and(|state| !state.enabled) {
            crate::i18n::tr("projects.hooks_disabled_in_settings")
                .into_owned()
                .into()
        } else {
            slint::SharedString::default()
        },
    );
    let hooks = state
        .map(|state| state.hooks.as_slice())
        .unwrap_or_default();
    let rows = choices(hooks, selected, state.is_some_and(|state| state.enabled));
    if ui.get_project_hook_choices().iter().collect::<Vec<_>>() != rows {
        ui.set_project_hook_choices(std::rc::Rc::new(VecModel::from(rows)).into());
    }
    update_counts(ui);
}

fn choices(
    hooks: &[crate::hooks::HookDefinition],
    selected: &[String],
    enabled: bool,
) -> Vec<ProjectHookChoice> {
    let mut rows: Vec<_> = hooks
        .iter()
        .map(|hook| {
            let row_enabled = enabled && hook.enabled;
            let preferred_selected = selected.contains(&hook.id);
            ProjectHookChoice {
                id: hook.id.clone().into(),
                name: hook.name.clone().into(),
                description: match hook.event.as_str() {
                    crate::hooks::PR_REVIEW_APPROVED => {
                        crate::i18n::tr("hooks.event_pr_approved").into_owned()
                    }
                    crate::hooks::PR_MERGED => {
                        crate::i18n::tr("hooks.event_pr_merged").into_owned()
                    }
                    "ai.review.completed.v1" => {
                        crate::i18n::tr("hooks.event_ai_review").into_owned()
                    }
                    event => event.to_owned(),
                }
                .into(),
                status: if row_enabled {
                    String::new()
                } else {
                    crate::i18n::tr("projects.hook_disabled").into_owned()
                }
                .into(),
                enabled: row_enabled,
                selected: preferred_selected,
                preferred_selected,
            }
        })
        .collect();
    // Preserve removed IDs until explicitly cleared, including during hydration.
    for id in selected {
        if !rows.iter().any(|row| row.id == id.as_str()) {
            rows.push(ProjectHookChoice {
                id: id.clone().into(),
                name: id.clone().into(),
                description: slint::SharedString::default(),
                status: crate::i18n::tr("projects.hook_unavailable")
                    .into_owned()
                    .into(),
                enabled: false,
                selected: true,
                preferred_selected: true,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::model::{ProjectConfig, ProjectPrOverview};
    use i_slint_backend_testing::ElementHandle;
    use slint::ModelRc;
    use slint::platform::WindowEvent;
    use std::cell::Cell;
    use std::rc::Rc;

    fn project(alias: &str, hooks_enabled: bool, hook_ids: &[&str]) -> ProjectConfig {
        ProjectConfig {
            project_name: alias.into(),
            project_alias: alias.into(),
            local_repo: format!("/{alias}"),
            remote_repo: format!("https://example.test/{alias}.git"),
            repo_project_id: alias.into(),
            repo_type: "github".into(),
            logo: String::new(),
            project_tags: Vec::new(),
            is_pr_auto_update: false,
            hooks_enabled,
            hook_ids: hook_ids.iter().map(|id| (*id).to_owned()).collect(),
            pr_overview: ProjectPrOverview::default(),
        }
    }

    #[test]
    fn preparing_each_project_replaces_the_shared_form_mode() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        let state = ProjectStateFacade::memory(vec![
            project("alpha", true, &["hook-one"]),
            project("beta", false, &["hook-two"]),
        ]);
        bind(&ui, state.clone());

        ui.invoke_project_prepare_hooks("alpha".into());
        assert!(ui.get_project_hooks_enabled());
        assert_eq!(selected_ids(&ui), ["hook-one"]);

        ui.invoke_project_prepare_hooks("beta".into());
        assert!(!ui.get_project_hooks_enabled());
        assert_eq!(selected_ids(&ui), ["hook-two"]);

        ui.invoke_project_prepare_hooks("".into());
        assert!(!ui.get_project_hooks_enabled());
        assert!(selected_ids(&ui).is_empty());
        state.shutdown();
    }

    #[test]
    fn new_projects_and_new_hooks_are_unchecked_and_edits_restore_exact_ids() {
        let mut first = crate::hooks::HookDefinition::default();
        first.enabled = true;
        let mut second = crate::hooks::HookDefinition::default();
        second.enabled = false;
        let hooks = vec![first.clone(), second.clone()];
        assert!(choices(&hooks, &[], true).iter().all(|row| !row.selected));
        let rows = choices(&hooks, &[first.id.clone(), "removed".into()], false);
        assert!(rows[0].selected);
        assert!(rows[0].preferred_selected);
        assert!(!rows[1].selected);
        assert!(!rows[1].preferred_selected);
        assert!(rows[2].selected);
        assert!(rows[2].preferred_selected);
        assert_eq!(rows[2].id, "removed");
        assert!(choices(&hooks, &[], true).iter().all(|row| !row.selected));

        let rows = choices(&hooks, &[first.id.clone(), second.id.clone()], true);
        assert!(rows[0].enabled);
        assert!(rows[0].selected);
        assert!(!rows[1].enabled);
        assert!(rows[1].selected);
        assert!(rows[1].preferred_selected);

        second.enabled = true;
        let restored = choices(&[first, second], &[rows[1].id.to_string()], true);
        assert!(restored[1].enabled);
        assert!(restored[1].selected);
    }

    #[test]
    fn project_hook_choices_distinguish_approval_from_actual_merge() {
        let _locale = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let _ui = MainWindow::new().unwrap();
        crate::i18n::apply_preference("en").unwrap();
        let mut approved = crate::hooks::HookDefinition::default();
        approved.enabled = true;
        let mut merged = approved.clone();
        merged.id = "merged-hook".into();
        merged.event = crate::hooks::PR_MERGED.into();

        let rows = choices(&[approved, merged], &[], true);
        assert_eq!(rows[0].description, "Pull request approved");
        assert_eq!(rows[1].description, "Pull request merged");
    }

    #[test]
    fn global_reenable_restores_only_each_hooks_previous_selection() {
        let mut selected_hook = crate::hooks::HookDefinition::default();
        selected_hook.enabled = true;
        let mut unselected_hook = crate::hooks::HookDefinition::default();
        unselected_hook.enabled = true;
        let hooks = vec![selected_hook.clone(), unselected_hook];

        let enabled = choices(&hooks, &[selected_hook.id], true);
        assert!(enabled[0].selected);
        assert!(!enabled[1].selected);

        let retained_ids = enabled
            .iter()
            .filter(|row| row.preferred_selected)
            .map(|row| row.id.to_string())
            .collect::<Vec<_>>();
        let disabled = choices(&hooks, &retained_ids, false);
        assert!(disabled.iter().all(|row| !row.enabled));
        assert!(disabled[0].selected);
        assert!(!disabled[1].selected);

        let restored_ids = disabled
            .iter()
            .filter(|row| row.preferred_selected)
            .map(|row| row.id.to_string())
            .collect::<Vec<_>>();
        let restored = choices(&hooks, &restored_ids, true);
        assert!(restored[0].selected);
        assert!(!restored[1].selected);
    }

    #[test]
    fn disabled_rows_reject_direct_interaction_without_losing_project_selection() {
        let model = VecModel::from(vec![ProjectHookChoice {
            id: "disabled".into(),
            name: "Disabled".into(),
            description: "".into(),
            status: "Disabled".into(),
            enabled: false,
            selected: true,
            preferred_selected: true,
        }]);

        set_choice_selected(&model, "disabled", false);
        let row = model.row_data(0).unwrap();
        assert!(row.selected);
        assert!(row.preferred_selected);
    }

    #[test]
    fn project_dialog_unmounts_off_mode_and_keeps_disabled_selection_inert() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().unwrap();
        ui.window().dispatch_event(WindowEvent::Resized {
            size: slint::LogicalSize::new(1_000.0, 900.0),
        });
        ui.set_platform_is_macos(false);
        ui.set_project_hook_choices(ModelRc::new(Rc::new(VecModel::from(vec![
            ProjectHookChoice {
                id: "disabled".into(),
                name: "Disabled Hook".into(),
                description: "Pull request approved".into(),
                status: "Disabled".into(),
                enabled: false,
                selected: true,
                preferred_selected: true,
            },
        ]))));
        ui.set_project_selected_hook_count(1);
        ui.set_project_selectable_hook_count(0);
        ui.set_project_hooks_ready(true);
        ui.set_project_show_add_modal(true);

        let activations = Rc::new(Cell::new(0));
        let observed = activations.clone();
        ui.on_project_toggle_hook(move |_, _| observed.set(observed.get() + 1));
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::ZERO);

        assert!(
            ElementHandle::find_by_element_type_name(&ui, "GitHubCheckbox")
                .all(|element| element.accessible_label().as_deref() != Some("Disabled Hook"))
        );

        ui.set_project_hooks_enabled(true);
        let form_scroll = ElementHandle::find_by_element_type_name(&ui, "GitHubScrollView")
            .find(|element| element.size().width > 400.0 && element.size().width < 500.0)
            .expect("project form scroll view");
        let pointer = slint::LogicalPosition::new(
            form_scroll.absolute_position().x + form_scroll.size().width / 2.0,
            form_scroll.absolute_position().y + form_scroll.size().height / 2.0,
        );
        let selected_radio = (0..16)
            .find_map(|_| {
                let radio = ElementHandle::find_by_element_type_name(&ui, "GitHubRadio").next();
                if radio.is_none() {
                    ui.window().dispatch_event(WindowEvent::PointerScrolled {
                        position: pointer,
                        delta_x: 0.0,
                        delta_y: -80.0,
                    });
                    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(
                        16,
                    ));
                }
                radio
            })
            .expect("selected Hook mode radio");
        let radio_y = selected_radio.absolute_position().y;
        let title = selected_radio
            .query_descendants()
            .match_id("GitHubRadio::label")
            .find_first()
            .expect("Hook mode radio title");
        let description = selected_radio
            .query_descendants()
            .match_id("GitHubRadio::description-label")
            .find_first()
            .expect("Hook mode radio description");
        assert!((title.absolute_position().y - radio_y).abs() < 0.5);
        assert!((title.size().height - 32.0).abs() < 0.5);
        assert!((description.absolute_position().y - radio_y - 32.0).abs() < 0.5);
        assert!((description.size().height - 20.0).abs() < 0.5);

        let checkbox = (0..16)
            .find_map(|_| {
                let checkbox = ElementHandle::find_by_element_type_name(&ui, "GitHubCheckbox")
                    .find(|element| {
                        element
                            .accessible_label()
                            .is_some_and(|label| label.as_str() == "Disabled Hook")
                    });
                if checkbox.is_none() {
                    ui.window().dispatch_event(WindowEvent::PointerScrolled {
                        position: pointer,
                        delta_x: 0.0,
                        delta_y: -80.0,
                    });
                    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(
                        16,
                    ));
                }
                checkbox
            })
            .expect("disabled project Hook checkbox after scrolling");
        assert_eq!(checkbox.accessible_enabled(), Some(false));
        assert_eq!(checkbox.accessible_checked(), Some(true));
        checkbox.invoke_accessible_default_action();
        assert_eq!(activations.get(), 0);
    }
}
