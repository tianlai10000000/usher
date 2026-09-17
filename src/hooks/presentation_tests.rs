use super::*;
use crate::{MainWindow, i18n, jobs};
use slint::{ComponentHandle, Model};
use usher_ui_settings::{GitHubSettingsPage, HookSettingsState};

fn run() -> HookRun {
    HookRun::queued(
        &HookDefinition::default(),
        &HookTrigger::new(PR_REVIEW_APPROVED, "test", serde_json::json!({})),
    )
}

#[test]
fn legacy_unknown_and_typed_errors_preserve_diagnostics_without_showing_them_as_ui_copy() {
    let mut run = run();
    run.conclusion = Some(HookConclusion::StartFailed);
    run.detail = "private path /tmp/user-content and OS error".into();
    let legacy = serde_json::to_value(&run).unwrap();
    assert!(legacy.get("failure_kind").is_none());
    let old: HookRun = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(old.detail, run.detail);
    assert_eq!(old.detail_text().source_text(), "Could not start");
    let mut future = legacy;
    future["failure_kind"] = "future_host_error".into();
    let future: HookRun = serde_json::from_value(future).unwrap();
    assert_eq!(future.failure_kind, Some(HookFailureKind::Unknown));
    assert!(!future.detail_text().source_text().contains("private"));
    run.failure_kind = Some(HookFailureKind::InterpreterStart);
    let typed: HookRun = serde_json::from_slice(&serde_json::to_vec(&run).unwrap()).unwrap();
    assert_eq!(typed.failure_kind, run.failure_kind);
    assert_eq!(typed.detail, run.detail);
}

#[test]
fn validation_returns_categories_and_exit_status_keeps_numeric_context() {
    let mut definition = HookDefinition::default();
    assert_eq!(definition.validate(), Err(HookValidationError::ScriptEmpty));
    definition.script = "true".into();
    definition.timeout_seconds = 0;
    assert_eq!(
        definition.validate(),
        Err(HookValidationError::TimeoutRange)
    );
    let mut run = run();
    run.conclusion = Some(HookConclusion::Failure);
    run.exit_code = Some(7);
    assert!(run.detail_text().source_text().contains("code 7"));
    run.signal = Some(15);
    assert!(run.detail_text().source_text().contains("signal 15"));
    run.failure_kind = Some(HookFailureKind::OutputDrain);
    assert!(
        run.detail_text()
            .source_text()
            .contains("Output collection")
    );
}

#[test]
fn every_status_and_host_error_has_source_and_packaged_copy() {
    for conclusion in [
        HookConclusion::Success,
        HookConclusion::Failure,
        HookConclusion::TimedOut,
        HookConclusion::Cancelled,
        HookConclusion::StartFailed,
        HookConclusion::Interrupted,
        HookConclusion::Skipped,
    ] {
        let key = conclusion.message_key();
        assert_ne!(usher_i18n::translate_for("en", key), key);
        assert_ne!(
            usher_i18n::translate_for("en", key),
            usher_i18n::translate_for("zh-CN", key)
        );
    }
    for kind in [
        HookFailureKind::Journal,
        HookFailureKind::Preparation,
        HookFailureKind::WorkingDirectory,
        HookFailureKind::InterpreterStart,
        HookFailureKind::ProcessTree,
        HookFailureKind::Wait,
        HookFailureKind::OutputDrain,
        HookFailureKind::InvalidDefinition,
        HookFailureKind::Unknown,
    ] {
        let key = kind.message_key();
        assert_ne!(usher_i18n::translate_for("en", key), key);
        assert_ne!(
            usher_i18n::translate_for("en", key),
            usher_i18n::translate_for("zh-CN", key)
        );
    }
    let mut pending = run();
    assert_eq!(pending.status_key(), "hooks.queued");
    pending.started_at_ms = Some(now_ms());
    assert_eq!(pending.status_key(), "hooks.running");
}

#[test]
fn error_category_alone_is_a_durable_change() {
    let (_dir, service) = tests::service();
    let mut run = run();
    run.conclusion = Some(HookConclusion::Failure);
    service
        .state
        .dispatch(state::Action::Run(run.clone()))
        .unwrap();
    let revision = service.state.snapshot().state().durable_revision;
    run.failure_kind = Some(HookFailureKind::Journal);
    service.state.dispatch(state::Action::Run(run)).unwrap();
    assert_eq!(
        service.state.snapshot().state().durable_revision,
        revision + 1
    );
    service.state.shutdown();
}

#[test]
fn mounted_history_result_and_job_refresh_without_rebuilding_log_models() {
    let _locale = i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    let mut run = run();
    run.started_at_ms = Some(now_ms());
    run.finished_at_ms = Some(now_ms());
    run.conclusion = Some(HookConclusion::StartFailed);
    run.failure_kind = Some(HookFailureKind::InterpreterStart);
    run.test = true;
    run.detail = "OS diagnostic /private/fixture".into();
    run.log = Arc::new("external stderr stays original\n".into());
    let state = HookState {
        ready: true,
        runs: vec![run.clone()],
        ..Default::default()
    };
    page.global::<HookSettingsState>()
        .set_test_run(run.id.clone().into());
    i18n::apply_preference("en").unwrap();
    let projection = projection::project_run(&run);
    // A worker's semantic projection can arrive after the UI changed locale.
    i18n::apply_preference("zh-CN").unwrap();
    jobs::apply_job_run(
        &ui,
        &projection.descriptor,
        &projection.job,
        &projection.detail,
        false,
    );
    let rows = ui.get_job_run_steps();
    let logs = rows.row_data(0).unwrap().log_lines;
    for locale in ["en", "zh-CN", "en"] {
        i18n::apply_preference(locale).unwrap();
        bridge::apply_settings(&page, &state);
        bridge::refresh_job_copy(&ui, &run);
        let g = page.global::<HookSettingsState>();
        assert!(
            g.get_history()
                .row_data(0)
                .unwrap()
                .label
                .contains(i18n::tr(run.status_key()).as_ref())
        );
        assert_eq!(g.get_result().as_str(), run.detail_text().resolve());
        assert!(!g.get_result().contains("/private"));
        assert_eq!(
            ui.get_job_run_summary().as_str(),
            run.detail_text().resolve()
        );
        assert!(
            ui.get_job_run_trigger_detail()
                .contains(&presentation::event_label_for(locale, &run.event))
        );
        let current = ui.get_job_run_steps();
        assert!(std::ptr::eq(rows.as_any(), current.as_any()));
        assert!(std::ptr::eq(
            logs.as_any(),
            current.row_data(0).unwrap().log_lines.as_any()
        ));
        assert!(
            current
                .row_data(0)
                .unwrap()
                .log_lines
                .row_data(0)
                .unwrap()
                .text
                .contains("external stderr")
        );
    }
}

#[test]
fn validation_feedback_retranslates_and_new_draft_clears_it() {
    let _locale = i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let page = GitHubSettingsPage::new().unwrap();
    i18n::apply_preference("en").unwrap();
    let (_dir, service) = tests::service();
    bridge::bind_settings_with_service(&ui, &page, service.clone());
    page.global::<HookSettingsState>().invoke_create();
    page.global::<HookSettingsState>().invoke_save();
    assert_eq!(
        page.global::<HookSettingsState>().get_result(),
        "Hook script is empty."
    );
    i18n::apply_preference("zh-CN").unwrap();
    bridge::sync_language(&page);
    assert_eq!(
        page.global::<HookSettingsState>().get_result().as_str(),
        i18n::tr("hooks.validation.script_empty")
    );
    bridge::set_draft(&page, &HookDefinition::default());
    bridge::sync_language(&page);
    assert!(page.global::<HookSettingsState>().get_result().is_empty());
    i18n::apply_preference("en").unwrap();
    service.state.shutdown();
}

#[test]
fn notifications_reproject_from_metadata_after_persistence_and_ignore_untrusted_ingress() {
    let (_locale, dir) = (i18n::test_locale_guard(), tempfile::tempdir().unwrap());
    let center = usher_messages::MessageCenter::open_with(
        dir.path().join("messages.json"),
        Arc::new(usher_messages::NoopDesktopNotifier),
    )
    .unwrap();
    let mut run = run();
    run.test = true;
    run.conclusion = Some(HookConclusion::TimedOut);
    let message = center
        .publish(presentation::notification_draft(&run, false))
        .unwrap();
    assert_eq!(message.title, run.name);
    assert!(message.body.is_empty());
    let encoded = serde_json::to_vec(&message).unwrap();
    let mut reloaded: usher_messages::Message = serde_json::from_slice(&encoded).unwrap();
    let en = presentation::notification_copy(&reloaded, "en").unwrap();
    let zh = presentation::notification_copy(&reloaded, "zh-CN").unwrap();
    assert!(en.0.contains("[Test]"));
    assert!(en.1.contains("Timed out"));
    assert!(
        zh.1.contains(usher_i18n::translate_for("zh-CN", "hooks.conclusion.timed_out").as_ref())
    );
    assert_ne!(en, zh);
    reloaded.channel = "webhook".into();
    assert!(presentation::notification_copy(&reloaded, "zh-CN").is_none());
    reloaded.channel = "application".into();
    reloaded.metadata.clear();
    assert!(presentation::notification_copy(&reloaded, "zh-CN").is_none());
    center.shutdown();
}

#[test]
fn mounted_hook_banner_and_expired_toast_refresh_on_locale_change() {
    let _locale = i18n::test_locale_guard();
    i_slint_backend_testing::init_no_event_loop();
    let ui = MainWindow::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let center = usher_messages::MessageCenter::open_with(
        dir.path().join("messages.json"),
        Arc::new(usher_messages::NoopDesktopNotifier),
    )
    .unwrap();
    let mut run = run();
    run.conclusion = Some(HookConclusion::Success);
    let message = center
        .publish(presentation::notification_draft(&run, false))
        .unwrap();
    i18n::apply_preference("en").unwrap();
    crate::notification::show_recorded_message(&ui, &center, &message);
    crate::toast::setup_toast(&ui);
    crate::toast::localized_error(&ui, "hooks.error.expired");
    i18n::apply_preference("zh-CN").unwrap();
    crate::notification::sync_hook_language(&ui, &center);
    crate::toast::sync_language(&ui);
    let row = ui
        .global::<crate::NotificationManager>()
        .get_queue()
        .row_data(0)
        .unwrap();
    assert_eq!(
        row.message.as_str(),
        presentation::notification_copy(&message, "zh-CN")
            .unwrap()
            .1
    );
    assert_eq!(
        ui.global::<crate::ToastManager>().get_message().as_str(),
        i18n::tr("hooks.error.expired")
    );
    ui.global::<crate::ToastManager>()
        .invoke_error(crate::ToastOptions {
            message: "External literal".into(),
            ..Default::default()
        });
    i18n::apply_preference("en").unwrap();
    crate::toast::sync_language(&ui);
    assert_eq!(
        ui.global::<crate::ToastManager>().get_message(),
        "External literal"
    );
    center.shutdown();
}
