use super::*;
use crate::workflow::state::WorkflowViewState;
#[test]
fn duplicate_start_is_rejected_and_late_progress_or_terminal_cannot_replace_a_new_run() {
    let context = WorkflowProjectContext::default();
    let mut view = WorkflowViewState::default();
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
