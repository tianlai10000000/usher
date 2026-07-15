//! OpenAI-compatible AI Agent provider connection.
//!
//! This module is intentionally independent from `src/chat/`, whose transport
//! remains the existing RAG endpoint. Future AI Agent capabilities should add
//! their provider calls here and use the shared HTTP module.

use reqwest::{Method, Url};
use serde::Deserialize;

use crate::http::{self, ErrorFeedback, HttpError, HttpErrorKind, RequestOptions};
use crate::settings::AiAgentSettings;

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelDescriptor>,
}

#[derive(Debug, Deserialize)]
struct ModelDescriptor {
    id: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConnectionTestResult {
    pub model_listed: bool,
}

fn configuration_error(message: impl Into<String>) -> HttpError {
    HttpError {
        kind: HttpErrorKind::Request,
        operation: "AI Agent configuration".to_string(),
        method: None,
        url: None,
        status: None,
        message: message.into(),
        response_body: None,
    }
}

fn validate_settings(settings: &AiAgentSettings) -> Result<(), HttpError> {
    if settings.base_url.trim().is_empty() {
        return Err(configuration_error("OpenAI Base URL is required"));
    }
    if settings.api_key.trim().is_empty() {
        return Err(configuration_error("OpenAI API Key is required"));
    }
    if settings.model.trim().is_empty() {
        return Err(configuration_error("Model name is required"));
    }
    Ok(())
}

fn endpoint_url(base_url: &str, resource: &str) -> Result<Url, HttpError> {
    let base_url = base_url.trim().trim_end_matches('/');
    let base_url = base_url
        .strip_suffix("/chat/completions")
        .unwrap_or(base_url);
    let base_url = base_url.strip_suffix("/models").unwrap_or(base_url);
    Url::parse(&format!("{base_url}/{resource}")).map_err(|error| HttpError {
        kind: HttpErrorKind::Build,
        operation: "AI Agent configuration".to_string(),
        method: Some(Method::GET),
        url: None,
        status: None,
        message: format!("Invalid OpenAI Base URL: {error}"),
        response_body: None,
    })
}

/// Verify credentials and provider reachability without creating a billable completion.
pub(crate) async fn test_connection(
    settings: &AiAgentSettings,
) -> Result<ConnectionTestResult, HttpError> {
    validate_settings(settings)?;
    let url = endpoint_url(&settings.base_url, "models")?;
    let response = http::send_json::<ModelsResponse>(
        http::get(url).bearer_auth(settings.api_key.trim()),
        RequestOptions::new("Test AI Agent connection").feedback(ErrorFeedback::Toast),
    )
    .await?;
    Ok(ConnectionTestResult {
        model_listed: response
            .data
            .iter()
            .any(|model| model.id == settings.model.trim()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_provider_base_urls() {
        assert_eq!(
            endpoint_url("https://api.deepseek.com", "models")
                .unwrap()
                .as_str(),
            "https://api.deepseek.com/models"
        );
        assert_eq!(
            endpoint_url("https://api.openai.com/v1/", "chat/completions")
                .unwrap()
                .as_str(),
            "https://api.openai.com/v1/chat/completions"
        );
    }
}
