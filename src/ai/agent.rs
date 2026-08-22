//! OpenAI-compatible AI Agent provider connection.
//!
//! This module is intentionally independent from `src/chat/`, whose transport
//! remains the existing RAG endpoint. Future AI Agent capabilities should add
//! their provider calls here and use the shared HTTP module.

use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};

use crate::http::{self, HttpError, HttpErrorKind, RequestOptions};
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CompletionMessage {
    pub role: &'static str,
    pub content: String,
}

impl CompletionMessage {
    pub(crate) fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system",
            content: content.into(),
        }
    }

    pub(crate) fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
        }
    }

    pub(crate) fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant",
            content: content.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: &'a [CompletionMessage],
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    #[serde(default)]
    choices: Vec<CompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    message: CompletionResponseMessage,
}

#[derive(Debug, Deserialize)]
struct CompletionResponseMessage {
    #[serde(default)]
    content: String,
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
        // The Settings action owns one bottom Toast for every terminal result.
        RequestOptions::new("Test AI Agent connection"),
    )
    .await?;
    Ok(ConnectionTestResult {
        model_listed: response
            .data
            .iter()
            .any(|model| model.id == settings.model.trim()),
    })
}

/// Execute one OpenAI-compatible chat completion through the shared HTTP path.
pub(crate) async fn complete(
    settings: &AiAgentSettings,
    messages: &[CompletionMessage],
    operation: &'static str,
) -> Result<String, HttpError> {
    validate_settings(settings)?;
    if messages.is_empty() {
        return Err(configuration_error(
            "At least one completion message is required",
        ));
    }
    let url = endpoint_url(&settings.base_url, "chat/completions")?;
    let request = CompletionRequest {
        model: settings.model.trim(),
        messages,
        temperature: 0.1,
    };
    let response = http::send_json::<CompletionResponse>(
        http::post(url)
            .bearer_auth(settings.api_key.trim())
            .json(&request),
        RequestOptions::new(operation),
    )
    .await?;
    response
        .choices
        .into_iter()
        .map(|choice| choice.message.content)
        .find(|content| !content.trim().is_empty())
        .ok_or_else(|| HttpError {
            kind: HttpErrorKind::Decode,
            operation: operation.to_string(),
            method: Some(Method::POST),
            url: None,
            status: None,
            message: "The AI provider returned no completion content".to_string(),
            response_body: None,
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
