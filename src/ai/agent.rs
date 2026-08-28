//! OpenAI-compatible AI Agent provider connection.
//!
//! This module is intentionally independent from `src/chat/`, whose transport
//! remains the existing RAG endpoint. Future AI Agent capabilities should add
//! their provider calls here and use the shared HTTP module.

use crate::http::{self, HttpError, HttpErrorKind, RequestOptions};
use crate::settings::AiAgentSettings;
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ai-code-review")]
const REVIEW_READ_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

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
    stream: bool,
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

// Preserve the shared HTTP error contract used by all provider operations.
#[allow(clippy::result_large_err)]
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

#[allow(clippy::result_large_err)]
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
    complete_with_options(settings, messages, RequestOptions::new(operation)).await
}

/// Execute an explicitly cancellable AI Review completion without a total
/// request deadline. The shared connect timeout still applies.
#[cfg(feature = "ai-code-review")]
pub(crate) async fn complete_review(
    settings: &AiAgentSettings,
    messages: &[CompletionMessage],
    operation: &'static str,
    diagnostic_context: impl Into<String>,
) -> Result<String, HttpError> {
    complete_with_options(
        settings,
        messages,
        RequestOptions::new(operation)
            .timeout(std::time::Duration::ZERO)
            .read_idle_timeout(REVIEW_READ_IDLE_TIMEOUT)
            .require_non_whitespace_body_progress()
            .body_diagnostics(diagnostic_context),
    )
    .await
}

async fn complete_with_options(
    settings: &AiAgentSettings,
    messages: &[CompletionMessage],
    options: RequestOptions,
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
        stream: false,
    };
    let operation = options.operation.clone();
    if let Some(context) = options.body_diagnostic_context.as_deref() {
        let system_messages = messages
            .iter()
            .filter(|message| message.role == "system")
            .count();
        let user_messages = messages
            .iter()
            .filter(|message| message.role == "user")
            .count();
        let assistant_messages = messages
            .iter()
            .filter(|message| message.role == "assistant")
            .count();
        let message_bytes = messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
        let largest_message_bytes = messages
            .iter()
            .map(|message| message.content.len())
            .max()
            .unwrap_or(0);
        let model = settings.model.trim().chars().take(128).collect::<String>();
        log::debug!(
            "[AI Agent][Completion] event=payload_prepared operation={} context={} model={:?} messages={} system_messages={} user_messages={} assistant_messages={} message_bytes={} largest_message_bytes={} temperature=0.1 stream=false",
            operation,
            context,
            model,
            messages.len(),
            system_messages,
            user_messages,
            assistant_messages,
            message_bytes,
            largest_message_bytes
        );
    }
    let response = http::send_json::<CompletionResponse>(
        http::post(url)
            .bearer_auth(settings.api_key.trim())
            .json(&request),
        options,
    )
    .await?;
    response
        .choices
        .into_iter()
        .map(|choice| choice.message.content)
        .find(|content| !content.trim().is_empty())
        .ok_or_else(|| HttpError {
            kind: HttpErrorKind::Decode,
            operation,
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

    #[test]
    fn completion_requests_explicitly_use_non_streaming_json_transport() {
        let messages = [CompletionMessage::user("review this")];
        let request = CompletionRequest {
            model: "provider-model",
            messages: &messages,
            temperature: 0.1,
            stream: false,
        };
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value.get("stream").and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn review_completion_allows_five_minutes_without_body_progress() {
        assert_eq!(
            REVIEW_READ_IDLE_TIMEOUT,
            std::time::Duration::from_secs(300)
        );
    }
}
