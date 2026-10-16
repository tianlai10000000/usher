use super::*;
use crate::workflow::state::WorkflowViewState;
#[test]
fn duplicate_start_is_rejected_and_late_progress_or_terminal_cannot_replace_a_new_run() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    view.tag_add_ready = true;
    let start = || TagAction::Run {
        remove: false,
        version: "v1.2.3".into(),
        description: "draft 中文 & $()".into(),
        validation: None,
    };
    let effects = reduce(&mut view, &context, start());
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, TagEffect::Run { run: 1, .. }))
    );
    assert!(view.tag_running);
    assert_eq!(view.tag_description, "draft 中文 & $()");
    let effects = reduce(&mut view, &context, start());
    assert_eq!(view.tag_run_generation, 1);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, TagEffect::Run { .. }))
    );
    reduce(&mut view, &context, TagAction::Stop);
    view.tag_add_ready = true;
    reduce(&mut view, &context, start());
    assert_eq!(view.tag_run_generation, 2);
    let before = view.clone();
    reduce(
        &mut view,
        &context,
        TagAction::Progress {
            run: 1,
            steps: Vec::new(),
            summary: LocalizedText::message("tag.stop_requested"),
        },
    );
    assert!(view == before);
    reduce(
        &mut view,
        &context,
        TagAction::Finished {
            run: 1,
            result: Err(TagRunFailure {
                summary: LocalizedText::message("tag.stop_requested"),
                steps: Vec::new(),
            }),
            duration: Duration::ZERO,
            cancelled: true,
        },
    );
    assert!(view == before);
}
#[test]
fn newer_initialization_and_bump_identity_rejects_stale_results_and_preserves_description() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    view.tag_description = "unmounted panel draft 中文".into();
    reduce(
        &mut view,
        &context,
        TagAction::Open {
            remove: false,
            force: true,
        },
    );
    let old = view.tag_add_request_seq;
    reduce(
        &mut view,
        &context,
        TagAction::Open {
            remove: true,
            force: true,
        },
    );
    let before = view.clone();
    reduce(
        &mut view,
        &context,
        TagAction::Initialized {
            remove: false,
            sequence: old,
            result: Ok("v9.9.9".into()),
        },
    );
    assert!(view == before);
    let current = view.tag_remove_request_seq;
    reduce(&mut view, &context, TagAction::Bump(VersionBump::Major));
    reduce(
        &mut view,
        &context,
        TagAction::Initialized {
            remove: true,
            sequence: current,
            result: Ok("v1.2.3".into()),
        },
    );
    assert_eq!(view.tag_next_version, "v2.0.0");
    assert_eq!(view.tag_description, "unmounted panel draft 中文");
}

#[test]
fn resetting_configuration_after_stop_invalidates_old_cancelled_terminal() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    view.tag_add_ready = true;
    reduce(
        &mut view,
        &context,
        TagAction::Run {
            remove: false,
            version: "v1.2.3".into(),
            description: String::new(),
            validation: None,
        },
    );
    let old = view.tag_run_generation;
    reduce(&mut view, &context, TagAction::Stop);
    reduce(&mut view, &context, TagAction::Bump(VersionBump::Major));
    let before = view.clone();
    reduce(
        &mut view,
        &context,
        TagAction::Finished {
            run: old,
            result: Err(TagRunFailure {
                summary: LocalizedText::message("tag.stop_requested"),
                steps: Vec::new(),
            }),
            duration: Duration::ZERO,
            cancelled: true,
        },
    );
    assert!(view == before);
    assert!(!view.job_run.active);
}

#[test]
fn entering_tag_refreshes_cached_version_and_uses_the_selected_bump_without_losing_description() {
    let context = WorkflowProjectContext {
        local_repo: "/repository".into(),
        ..Default::default()
    };
    for (bump, expected_next) in [
        (VersionBump::Patch, "v2.4.7"),
        (VersionBump::Minor, "v2.5.0"),
        (VersionBump::Major, "v3.0.0"),
    ] {
        let mut view = WorkflowViewState::default();
        view.tag_add_ready = true;
        view.tag_current_version = "v1.2.3".into();
        view.tag_next_version = "v1.2.4".into();
        view.tag_version_mode = bump;
        view.tag_description = "draft 中文 & $()".into();
        let effects = reduce(&mut view, &context, TagAction::Enter);
        let sequence = view.tag_add_request_seq;
        assert!(effects.iter().any(|effect| matches!(
            effect,
            TagEffect::Initialize { remove: false, sequence: request, repo }
                if *request == sequence && repo == "/repository"
        )));
        assert!(view.tag_add_initializing);
        assert!(!view.tag_add_ready);
        reduce(
            &mut view,
            &context,
            TagAction::Initialized {
                remove: false,
                sequence,
                result: Ok("v2.4.6".into()),
            },
        );
        assert_eq!(view.tag_current_version, "v2.4.6");
        assert_eq!(view.tag_next_version, expected_next);
        assert_eq!(view.tag_remove_target, "v2.4.6");
        assert_eq!(view.tag_description, "draft 中文 & $()");
        assert!(view.tag_add_ready);
        assert!(!view.tag_add_initializing);
    }
}

#[test]
fn entering_tag_preserves_remove_selection_and_reuses_an_in_flight_initialization() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    view.tag_selected_tab = 1;
    view.tag_remove_ready = true;
    let effects = reduce(&mut view, &context, TagAction::Enter);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, TagEffect::Initialize { remove: true, .. }))
    );
    assert_eq!(view.tag_selected_tab, 1);
    assert!(view.tag_remove_initializing);
    let before = view.clone();
    assert!(reduce(&mut view, &context, TagAction::Enter).is_empty());
    assert!(view == before);
}

#[test]
fn entering_tag_during_a_run_preserves_the_complete_job_state() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    view.tag_add_ready = true;
    reduce(
        &mut view,
        &context,
        TagAction::Run {
            remove: false,
            version: "v1.2.3".into(),
            description: "running draft".into(),
            validation: None,
        },
    );
    assert!(view.tag_running);
    let before = view.clone();
    assert!(reduce(&mut view, &context, TagAction::Enter).is_empty());
    assert!(view == before);
}

#[test]
fn a_new_tag_entry_rejects_results_from_the_previous_entry() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    reduce(&mut view, &context, TagAction::Enter);
    let old = view.tag_add_request_seq;
    reduce(
        &mut view,
        &context,
        TagAction::Initialized {
            remove: false,
            sequence: old,
            result: Ok("v1.2.3".into()),
        },
    );
    reduce(&mut view, &context, TagAction::Enter);
    let before = view.clone();
    reduce(
        &mut view,
        &context,
        TagAction::Initialized {
            remove: false,
            sequence: old,
            result: Ok("v99.0.0".into()),
        },
    );
    assert!(view == before);
}

#[test]
fn failed_remote_inspection_clears_unknown_versions_and_rejects_create() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
    view.tag_add_ready = true;
    view.tag_description = "preserved after failure".into();
    reduce(&mut view, &context, TagAction::Enter);
    let sequence = view.tag_add_request_seq;
    let error = LocalizedText::message("tag.initialization_timeout");
    let effects = reduce(
        &mut view,
        &context,
        TagAction::Initialized {
            remove: false,
            sequence,
            result: Err(error.clone()),
        },
    );
    assert!(view.tag_current_version.is_empty());
    assert!(view.tag_next_version.is_empty());
    assert!(view.tag_remove_target.is_empty());
    assert_eq!(view.tag_description, "preserved after failure");
    assert!(!view.tag_add_ready);
    assert!(!view.tag_add_initializing);
    assert_eq!(view.tag_result, error);
    assert!(effects.iter().any(|effect| matches!(
        effect,
        TagEffect::Toast(false, message) if message == &error
    )));
    for bump in [VersionBump::Patch, VersionBump::Minor, VersionBump::Major] {
        reduce(&mut view, &context, TagAction::Bump(bump));
        assert!(view.tag_current_version.is_empty());
        assert!(view.tag_next_version.is_empty());
        assert_eq!(view.tag_result, error);
        assert_eq!(view.tag_version_mode, bump);
    }
    let before = view.clone();
    let effects = reduce(
        &mut view,
        &context,
        TagAction::Run {
            remove: false,
            version: "v0.0.1".into(),
            description: "must not run".into(),
            validation: None,
        },
    );
    assert!(view == before);
    assert!(
        matches!(effects.as_slice(), [TagEffect::Toast(false, message)]
        if message == &LocalizedText::message("tag.remote_tag_not_ready"))
    );
}

#[test]
fn a_successful_run_restores_its_own_ready_state_and_a_failed_run_requires_refresh() {
    let context = WorkflowProjectContext::default();
    for (remove, success) in [(false, true), (true, true), (false, false)] {
        let mut view = WorkflowViewState::default();
        view.tag_add_ready = true;
        reduce(
            &mut view,
            &context,
            TagAction::Run {
                remove,
                version: "v1.2.3".into(),
                description: String::new(),
                validation: None,
            },
        );
        let run = view.tag_run_generation;
        // A tab intent during execution must not change which completed
        // operation owns the known remote version.
        reduce(
            &mut view,
            &context,
            TagAction::Open {
                remove: !remove,
                force: false,
            },
        );
        let summary = LocalizedText::message("tag.stop_requested");
        let result = if success {
            Ok(TagRunSuccess {
                summary,
                current_version: "v2.4.6".into(),
                steps: Vec::new(),
            })
        } else {
            Err(TagRunFailure {
                summary,
                steps: Vec::new(),
            })
        };
        reduce(
            &mut view,
            &context,
            TagAction::Finished {
                run,
                result,
                duration: Duration::ZERO,
                cancelled: false,
            },
        );
        assert!(!view.tag_running);
        assert_eq!(view.tag_add_ready, success && !remove);
        assert_eq!(view.tag_remove_ready, success && remove);
        if success {
            assert_eq!(view.tag_current_version, "v2.4.6");
            assert_eq!(view.tag_next_version, "v2.4.7");
        }
    }
}
