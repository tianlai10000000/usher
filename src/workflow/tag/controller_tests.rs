use super::{
    LOCAL_TAG_ERROR_DETAIL_CHARS, LocalTagReadError, bounded_diagnostic_detail, format_command_log,
    run_command,
};
use crate::i18n::test_locale_guard;
use crate::jobs::{JobConclusion, JobStepData, JobStepDefinition};
use slint::Model;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[test]
fn command_log_matches_githubs_continuous_run_output() {
    assert_eq!(
        format_command_log(
            "git -C /repo checkout release",
            "already up to date\n",
            "warning: example\n",
        ),
        "Run git -C /repo checkout release\nalready up to date\nwarning: example"
    );
}

#[test]
fn command_log_omits_empty_stream_headings_and_blank_sections() {
    assert_eq!(
        format_command_log(
            "git -C /repo checkout release",
            "",
            "error: pathspec did not match\n",
        ),
        "Run git -C /repo checkout release\nerror: pathspec did not match"
    );
}

#[test]
fn local_tag_error_identifies_unaccepted_xcode_license() {
    let _locale_guard = test_locale_guard();
    let error = LocalTagReadError::classify_failed_output(
        "69".into(),
        "You have not agreed to the Xcode and Apple SDKs license.",
    );

    assert_eq!(error, LocalTagReadError::XcodeLicenseNotAccepted);
    assert!(
        error
            .localized()
            .source_text()
            .contains("sudo xcodebuild -license accept")
    );
    usher_i18n::activate_preference("zh-CN");
    assert!(error.localized().resolve().contains("尚未接受 Xcode"));
    usher_i18n::activate_preference(usher_i18n::fallback_locale().id);
}

#[test]
fn remote_tag_initialization_copy_retranslates_without_retaining_local_tag_wording() {
    let _locale_guard = test_locale_guard();
    let error =
        LocalTagReadError::classify_failed_output("128".into(), "fixture remote unavailable")
            .localized();
    let loading = crate::i18n::LocalizedText::message("tag.loading_remote_context");
    for (locale, error_fragment, loading_fragment) in [
        ("en", "querying remote Tags", "latest remote Tag"),
        ("zh-CN", "查询远端 Tag", "最新远端 Tag"),
    ] {
        usher_i18n::activate_preference(locale);
        assert!(error.resolve().contains(error_fragment));
        assert!(error.resolve().contains("128"));
        assert!(error.resolve().contains("fixture remote unavailable"));
        assert!(loading.resolve().contains(loading_fragment));
    }
    usher_i18n::activate_preference(usher_i18n::fallback_locale().id);
}

#[test]
fn local_tag_error_classifies_common_git_failures() {
    assert_eq!(
        LocalTagReadError::classify_failed_output(
            "128".into(),
            "fatal: not a git repository (or any parent directories): .git",
        ),
        LocalTagReadError::NotRepository
    );
    assert_eq!(
        LocalTagReadError::classify_failed_output(
            "128".into(),
            "fatal: detected dubious ownership in repository at '/repo'",
        ),
        LocalTagReadError::UnsafeRepository
    );
    assert_eq!(
        LocalTagReadError::classify_failed_output(
            "1".into(),
            "xcrun: error: invalid active developer path",
        ),
        LocalTagReadError::DeveloperToolsUnavailable
    );
}

#[test]
fn unknown_git_failure_retains_bounded_diagnostic_and_exit_status() {
    let diagnostic = "unexpected failure ".repeat(64);
    let error = LocalTagReadError::classify_failed_output("42".into(), &diagnostic);
    let LocalTagReadError::GitFailed { status, detail } = error else {
        panic!("expected generic Git failure");
    };

    assert_eq!(status, "42");
    assert!(detail.ends_with("..."));
    assert_eq!(
        detail.trim_end_matches("...").chars().count(),
        LOCAL_TAG_ERROR_DETAIL_CHARS
    );
    assert_eq!(
        bounded_diagnostic_detail(" one\n two\tthree "),
        "one two three"
    );
    assert_eq!(
        LocalTagReadError::classify_failed_output("128".into(), " \n\t"),
        LocalTagReadError::GitFailedWithoutDetail {
            status: "128".into()
        }
    );
}

#[test]
fn slint_step_exposes_one_log_line_model_row_per_rendered_line() {
    let mut step = JobStepData::queued(
        JobStepDefinition::new("Example", vec!["git".into(), "status".into()]),
        "",
    );
    step.finish(JobConclusion::Success, Some(Duration::ZERO), "", false);
    step.log_requested = true;
    step.set_log("Run git status\nclean\ncomplete".into());
    let slint_step = step.to_slint();
    assert_eq!(slint_step.log_lines.row_count(), 3);
    assert_eq!(
        slint_step.log_lines.row_data(0).unwrap().text.as_str(),
        "Run git status"
    );
    assert_eq!(
        slint_step.log_lines.row_data(1).unwrap().text.as_str(),
        "clean"
    );
    assert_eq!(
        slint_step.log_lines.row_data(2).unwrap().text.as_str(),
        "complete"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn command_output_is_published_before_process_completion() {
    let token = Arc::new(AtomicBool::new(false));
    let mut snapshots = Vec::new();
    let result = run_command(
        vec![
            "sh".into(),
            "-c".into(),
            "printf 'warning: first\\n'; sleep 0.15; printf 'error: second\\n' >&2".into(),
        ],
        &token,
        |snapshot| snapshots.push(snapshot),
    )
    .await
    .expect("shell command should succeed");

    assert!(result.log.contains("error: second"));
    assert!(
        snapshots
            .iter()
            .take(snapshots.len().saturating_sub(1))
            .any(|snapshot| snapshot.contains("warning: first"))
    );
}
