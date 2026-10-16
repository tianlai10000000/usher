//! Search and bounded page projection of committed Agent registry metadata.

use crate::settings::{AiAgentConfiguration, AiAgentProfile};
use slint::{Global, ModelRc, VecModel};
use usher_ui_settings::{AiAgentListItem, AppState as PageState, GitHubSettingsPage};

pub(super) const PAGE_SIZE: usize = 10;

pub(super) fn filtered_profiles<'a>(
    config: &'a AiAgentConfiguration,
    query: &str,
) -> Vec<&'a AiAgentProfile> {
    let query = query.trim().to_lowercase();
    config
        .profiles
        .iter()
        .rev()
        .filter(|profile| {
            query.is_empty()
                || [
                    &profile.name,
                    &profile.connection.model,
                    &profile.connection.provider,
                ]
                .iter()
                .any(|value| value.to_lowercase().contains(&query))
        })
        .collect()
}

pub(crate) fn apply_ai_agent_choices_to_page(
    page: &GitHubSettingsPage,
    config: &AiAgentConfiguration,
) {
    let state = PageState::get(page);
    if state.get_ai_agent_delete_confirm_open() {
        if let Some(profile) = config
            .profile(state.get_ai_agent_delete_id().as_str())
            .filter(|profile| profile.id != config.active_id)
        {
            let name =
                crate::ai::identity::display(&profile.name, &profile.connection.provider, "");
            state.set_ai_agent_delete_name(name.clone().into());
            state.set_ai_agent_delete_message(
                crate::i18n::tr_args("settings.ai_agent.delete_confirmation", &[("name", &name)])
                    .into(),
            );
        } else {
            state.set_ai_agent_delete_confirm_open(false);
        }
    }
    let rows = filtered_profiles(config, state.get_ai_agent_query().as_str());
    let pages = rows.len().div_ceil(PAGE_SIZE).max(1);
    let selected = (state.get_ai_agent_page().max(1) as usize).min(pages);
    let items = rows
        .into_iter()
        .skip((selected - 1) * PAGE_SIZE)
        .take(PAGE_SIZE)
        .map(|profile| {
            let identity =
                crate::ai::identity::display(&profile.name, &profile.connection.provider, "");
            AiAgentListItem {
                id: profile.id.clone().into(),
                name: identity.into(),
                model: profile.connection.model.clone().into(),
                provider: profile.connection.provider.clone().into(),
                active: config.active_id == profile.id,
            }
        })
        .collect::<Vec<_>>();
    state.set_ai_agent_list(ModelRc::new(VecModel::from(items)));
    state.set_ai_agent_page(selected as i32);
    state.set_ai_agent_total_pages(pages as i32);
    state.set_ai_agent_total_count(
        filtered_profiles(config, state.get_ai_agent_query().as_str()).len() as i32,
    );
    // Only show a bounded page-number window for arbitrarily large registries.
    let first = selected.saturating_sub(2).max(1);
    let last = (first + 4).min(pages);
    state.set_ai_agent_pages(ModelRc::new(VecModel::from(
        (first..=last).map(|n| n as i32).collect::<Vec<_>>(),
    )));
}
