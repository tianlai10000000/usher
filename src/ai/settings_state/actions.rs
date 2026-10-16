//! Agent CRUD reductions recheck identity and active ownership at execution.

use crate::settings::{AiAgentConfiguration, AiAgentProfile};
use usher_state_store::Reduction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AiAgentSettingsAction {
    Create(AiAgentProfile),
    Update(AiAgentProfile),
    Delete(String),
    Activate(String),
}

impl AiAgentSettingsAction {
    pub(super) fn target_id(&self) -> &str {
        match self {
            Self::Create(profile) | Self::Update(profile) => &profile.id,
            Self::Delete(id) | Self::Activate(id) => id,
        }
    }
}

pub(super) fn reduce_ai_agent_settings(
    current: &AiAgentConfiguration,
    action: AiAgentSettingsAction,
) -> Reduction<AiAgentConfiguration, Result<(), String>> {
    let mut next = current.clone();
    let applied = apply(&mut next, action);
    if applied.is_err() {
        return Reduction::unchanged(current.clone(), applied);
    }
    if next == *current {
        Reduction::unchanged(next, applied)
    } else {
        Reduction::changed(next, applied)
    }
}

fn apply(state: &mut AiAgentConfiguration, action: AiAgentSettingsAction) -> Result<(), String> {
    match action {
        AiAgentSettingsAction::Create(profile) => {
            profile.validate()?;
            let profile = profile.normalized();
            if state.profile(&profile.id).is_some() {
                return Err("AI Agent identity already exists".into());
            }
            if state.profiles.is_empty() {
                state.active_id = profile.id.clone();
            }
            state.profiles.push(profile);
        }
        AiAgentSettingsAction::Update(profile) => {
            profile.validate()?;
            let profile = profile.normalized();
            let target = state
                .profile_mut(&profile.id)
                .ok_or("AI Agent no longer exists")?;
            *target = profile;
        }
        AiAgentSettingsAction::Delete(id) => {
            if state.active_id == id {
                return Err("The active AI Agent cannot be deleted".into());
            }
            let index = state
                .profiles
                .iter()
                .position(|profile| profile.id == id)
                .ok_or("AI Agent no longer exists")?;
            state.profiles.remove(index);
        }
        AiAgentSettingsAction::Activate(id) => {
            if state.profile(&id).is_none() {
                return Err("AI Agent no longer exists".into());
            }
            state.active_id = id;
        }
    }
    Ok(())
}
