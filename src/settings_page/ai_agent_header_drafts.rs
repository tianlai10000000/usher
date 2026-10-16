//! Dialog-local Header input modes; only validated normalized rows can be saved.

use super::ai_agent_header_object;
use crate::settings::{AiAgentHeader, AiAgentSettings};
use slint::{ComponentHandle, Global, Model, ModelRc, VecModel};
use usher_ui_settings::{AiAgentHeaderItem, AppState, GitHubSettingsPage};

const INVALID_OBJECT: &str = "settings.feedback.ai_agent_invalid_header_object";

fn rows(page: &GitHubSettingsPage) -> Vec<AiAgentHeader> {
    AppState::get(page)
        .get_ai_agent_editor_headers()
        .iter()
        .map(|header| AiAgentHeader {
            enabled: header.enabled,
            name: header.name.to_string(),
            value: header.value.to_string(),
        })
        .collect()
}

fn project(page: &GitHubSettingsPage, headers: Vec<AiAgentHeader>) {
    AppState::get(page).set_ai_agent_editor_headers_object_text("".into());
    AppState::get(page).set_ai_agent_editor_headers(ModelRc::new(VecModel::from(
        headers
            .into_iter()
            .map(|header| AiAgentHeaderItem {
                enabled: header.enabled,
                name: header.name.into(),
                value: header.value.into(),
            })
            .collect::<Vec<_>>(),
    )));
}

pub(crate) fn normalized(page: &GitHubSettingsPage) -> Result<Vec<AiAgentHeader>, &'static str> {
    let state = AppState::get(page);
    let previous = rows(page);
    let headers = if state.get_ai_agent_editor_headers_object_mode() {
        ai_agent_header_object::merge(
            state.get_ai_agent_editor_headers_object_text().as_str(),
            &previous,
        )
        .map_err(|_| INVALID_OBJECT)?
    } else {
        previous
    };
    crate::ai::request_headers::validate(&AiAgentSettings {
        headers: headers.clone(),
        ..Default::default()
    })
    .map_err(|_| "settings.feedback.ai_agent_invalid_headers")?;
    Ok(headers)
}

fn edited(page: &GitHubSettingsPage) {
    let state = AppState::get(page);
    state.invoke_cancel_ai_agent_test();
    state.set_ai_agent_editor_headers_input_revision(
        state
            .get_ai_agent_editor_headers_input_revision()
            .wrapping_add(1),
    );
    state.set_ai_agent_editor_feedback_open(false);
    state.set_ai_agent_editor_test_result_key("".into());
    state.set_ai_agent_editor_test_result("".into());
    state.set_ai_agent_editor_error_key("".into());
    state.set_ai_agent_editor_error("".into());
}

fn error(page: &GitHubSettingsPage, key: &'static str) {
    let state = AppState::get(page);
    state.set_ai_agent_editor_error_key(key.into());
    state.set_ai_agent_editor_error(crate::i18n::tr(key).into_owned().into());
    state.set_ai_agent_editor_feedback_open(true);
}

fn edit_row(page: &GitHubSettingsPage, index: i32, edit: impl FnOnce(&mut AiAgentHeader)) {
    let state = AppState::get(page);
    if state.get_ai_agent_editor_busy() || state.get_ai_agent_editor_headers_object_mode() {
        return;
    }
    let mut headers = rows(page);
    if let Some(header) = usize::try_from(index)
        .ok()
        .and_then(|index| headers.get_mut(index))
    {
        let previous = header.clone();
        edit(header);
        if *header == previous {
            return;
        }
        project(page, headers);
        edited(page);
    }
}

pub(super) fn bind(page: &GitHubSettingsPage) {
    let weak = page.as_weak();
    // Revision identity also distinguishes a parked empty source from no draft.
    // Projection/reset and real row edits invalidate this visit-local marker.
    let parked_revision = std::cell::Cell::new(None);
    AppState::get(page).on_select_ai_agent_header_mode(move |object_mode| {
        let Some(page) = weak.upgrade() else { return };
        let state = AppState::get(&page);
        if state.get_ai_agent_editor_busy()
            || object_mode == state.get_ai_agent_editor_headers_object_mode()
        {
            return;
        }
        if object_mode {
            if parked_revision.get() != Some(state.get_ai_agent_editor_headers_input_revision()) {
                state.set_ai_agent_editor_headers_object_text(
                    ai_agent_header_object::format(&rows(&page)).into(),
                );
            }
            parked_revision.set(None);
            state.set_ai_agent_editor_headers_object_mode(true);
            edited(&page);
            if let Err(key) = normalized(&page) {
                error(&page, key);
            }
        } else {
            let invalid = match normalized(&page) {
                Ok(headers) => {
                    project(&page, headers);
                    false
                }
                Err(_) => true,
            };
            state.set_ai_agent_editor_headers_object_mode(false);
            edited(&page);
            parked_revision
                .set(invalid.then_some(state.get_ai_agent_editor_headers_input_revision()));
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_edit_ai_agent_header_object(move |source| {
        if let Some(page) = weak.upgrade() {
            let state = AppState::get(&page);
            if !state.get_ai_agent_editor_busy() && state.get_ai_agent_editor_headers_object_mode()
            {
                state.set_ai_agent_editor_headers_object_text(source);
                edited(&page);
            }
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_add_ai_agent_header(move || {
        if let Some(page) = weak.upgrade() {
            let state = AppState::get(&page);
            let mut headers = rows(&page);
            if !state.get_ai_agent_editor_busy()
                && !state.get_ai_agent_editor_headers_object_mode()
                && headers.len() < crate::settings::MAX_AI_AGENT_HEADERS
            {
                headers.push(AiAgentHeader {
                    enabled: true,
                    ..Default::default()
                });
                project(&page, headers);
                edited(&page);
            }
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_remove_ai_agent_header(move |index| {
        if let Some(page) = weak.upgrade() {
            let state = AppState::get(&page);
            let mut headers = rows(&page);
            if !state.get_ai_agent_editor_busy()
                && !state.get_ai_agent_editor_headers_object_mode()
                && index >= 0
                && (index as usize) < headers.len()
            {
                headers.remove(index as usize);
                project(&page, headers);
                edited(&page);
            }
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_set_ai_agent_header_enabled(move |index, value| {
        if let Some(page) = weak.upgrade() {
            edit_row(&page, index, |header| header.enabled = value);
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_set_ai_agent_header_name(move |index, value| {
        if let Some(page) = weak.upgrade() {
            edit_row(&page, index, |header| header.name = value.to_string());
        }
    });
    let weak = page.as_weak();
    AppState::get(page).on_set_ai_agent_header_value(move |index, value| {
        if let Some(page) = weak.upgrade() {
            edit_row(&page, index, |header| header.value = value.to_string());
        }
    });
}

#[cfg(test)]
#[path = "ai_agent_header_drafts_tests.rs"]
mod tests;
