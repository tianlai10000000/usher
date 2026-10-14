//! Event-loop projection and worker commands for shared request preferences.
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, Global};
use usher_state_store::Subscription;

use super::request_preferences::{RequestPreferencesFacade, normalize_timeout_ms, wait_for_save};
use crate::git::PullRequestMergeMethod;
use crate::settings_feedback::{SettingsSaveTarget, save_settings_with_feedback};
use crate::{AppState, MainWindow, SettingsPageBridge};

pub(crate) fn install(
    window: &MainWindow,
    page: &SettingsPageBridge,
    preferences: &RequestPreferencesFacade,
) -> Subscription {
    let bridge = page.clone();
    let apply = crate::app_runtime::latest_ui(
        window.as_weak(),
        move |window,
              snapshot: usher_state_store::StoreSnapshot<
            super::request_preferences::RequestPreferences,
        >| {
            let state = snapshot.state();
            let target = AppState::get(window);
            target.set_http_request_timeout_ms(state.http_request_timeout_ms.clone().into());
            target.set_default_pr_merge_method(state.default_pr_merge_method.as_str().into());
            bridge.sync_request_preferences(state);
        },
    );
    // Serialize offers by Store revision, including concurrent worker commits.
    let latest_revision = Mutex::new(None);
    let subscription = preferences.subscribe(Arc::new(move |snapshot| {
        let mut latest = latest_revision
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if latest.is_some_and(|revision| snapshot.revision() <= revision) {
            return;
        }
        *latest = Some(snapshot.revision());
        apply(snapshot);
    }));

    let state = AppState::get(window);
    state.on_validate_http_request_timeout(|value| normalize_timeout_ms(value.as_str()).is_some());
    let weak = window.as_weak();
    let bridge = page.clone();
    let facade = preferences.clone();
    state.on_save_http_request_timeout(move |value| {
        let Some(value) = normalize_timeout_ms(value.as_str()) else {
            return false;
        };
        let receipt = facade.submit_timeout(&value);
        let accepted = receipt.is_ok();
        save_settings_with_feedback(
            weak.clone(),
            bridge.clone(),
            SettingsSaveTarget::RequestPreferences,
            "HTTP request timeout",
            "settings.feedback.http_request_timeout_saved",
            "settings.feedback.http_request_timeout_save_failed",
            move || wait_for_save(receipt),
        );
        accepted
    });

    let weak = window.as_weak();
    let bridge = page.clone();
    let facade = preferences.clone();
    state.on_save_default_pr_merge_method(move |value| {
        let Some(method) = PullRequestMergeMethod::parse(value.as_str()) else {
            return;
        };
        let receipt = facade.submit_merge_method(method);
        save_settings_with_feedback(
            weak.clone(),
            bridge.clone(),
            SettingsSaveTarget::RequestPreferences,
            "default PR merge method",
            "settings.feedback.default_pr_merge_method_saved",
            "settings.feedback.default_pr_merge_method_save_failed",
            move || wait_for_save(receipt),
        );
    });
    subscription
}
