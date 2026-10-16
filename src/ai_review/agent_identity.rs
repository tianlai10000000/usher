//! Capture non-secret Agent attribution and project it at the locale boundary.

use crate::{AiReviewConversationItem, MainWindow};
use slint::Model;
use usher_ai_review::{AgentIdentity, ReviewConversationMessage};

pub(super) fn capture(profile: &crate::settings::AiAgentProfile) -> AgentIdentity {
    AgentIdentity {
        name: profile.name.clone(),
        provider: profile.connection.provider.clone(),
        model: profile.connection.model.clone(),
    }
}

pub(super) fn display(identity: Option<&AgentIdentity>, legacy_model: &str) -> String {
    match identity {
        Some(identity) => {
            crate::ai::identity::display(&identity.name, &identity.provider, &identity.model)
        }
        None => crate::ai::identity::display("", "", legacy_model),
    }
}

pub(super) fn attribute(
    mut message: ReviewConversationMessage,
    identity: &AgentIdentity,
) -> ReviewConversationMessage {
    message.agent = Some(identity.clone());
    message
}

pub(super) fn project(item: &mut AiReviewConversationItem, identity: Option<&AgentIdentity>) {
    if let Some(identity) = identity {
        item.agent_name = identity.name.as_str().into();
        item.agent_provider = identity.provider.as_str().into();
        item.agent_model = identity.model.as_str().into();
    }
    refresh(item);
}

pub(super) fn refresh(item: &mut AiReviewConversationItem) {
    if !item.agent_provider.is_empty()
        || !item.agent_name.is_empty()
        || !item.agent_model.is_empty()
    {
        item.agent_label =
            crate::ai::identity::display(&item.agent_name, &item.agent_provider, &item.agent_model)
                .into();
    }
}

pub(super) fn attribute_latest_ui_message(ui: &MainWindow, identity: &AgentIdentity) {
    let conversation = ui.get_ai_review_conversation();
    if let Some(index) = conversation.row_count().checked_sub(1)
        && let Some(mut item) = conversation.row_data(index)
    {
        project(&mut item, Some(identity));
        conversation.set_row_data(index, item);
    }
}

pub(super) fn inherit_latest(item: &mut AiReviewConversationItem, ui: &MainWindow) {
    let conversation = ui.get_ai_review_conversation();
    if let Some(previous) = conversation
        .row_count()
        .checked_sub(1)
        .and_then(|index| conversation.row_data(index))
    {
        item.agent_name = previous.agent_name;
        item.agent_provider = previous.agent_provider;
        item.agent_model = previous.agent_model;
        refresh(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_attribution_survives_profile_edit_and_serialization() {
        let mut profile = crate::settings::AiAgentProfile {
            id: "review-agent-stable-id".into(),
            name: "PR Reviewer".into(),
            connection: crate::settings::AiAgentSettings {
                provider: "deepseek".into(),
                model: "model-before".into(),
                ..Default::default()
            },
        };
        let identity = capture(&profile);
        profile.name = "New Agent".into();
        profile.connection.model = "model-after".into();
        let restored: AgentIdentity =
            serde_json::from_str(&serde_json::to_string(&identity).unwrap()).unwrap();
        assert_eq!(restored.name, "PR Reviewer");
        assert_eq!(restored.model, "model-before");
        let encoded = serde_json::to_string(&identity).unwrap();
        assert!(!encoded.contains("api_key"));
        assert!(!encoded.contains("base_url"));
    }
}
