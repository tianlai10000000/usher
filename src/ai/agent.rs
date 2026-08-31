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
#[cfg(feature = "ai-code-review")]
const MAX_STREAMED_COMPLETION_BYTES: usize = 16 * 1024 * 1024;

/// Total wall-clock budget AI Review callers wrap around one
/// [`complete_review`] call. Twice the idle timeout: a stalled provider trips
/// the 300s idle guard first, while this ceiling additionally bounds
/// responses that keep trickling bytes without ever finishing, so a task
/// fails instead of hanging forever.
#[cfg(feature = "ai-code-review")]
pub(crate) const REVIEW_CHUNK_TOTAL_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(600);
#[cfg(feature = "ai-code-review")]
pub(crate) const REVIEW_CHAT_TOTAL_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(600);

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

#[cfg(feature = "ai-code-review")]
#[derive(Debug, Default, Deserialize)]
struct StreamingCompletionMessage {
    #[serde(default)]
    content: Option<String>,
}

#[cfg(feature = "ai-code-review")]
#[derive(Debug, Deserialize)]
struct StreamingCompletionChoice {
    #[serde(default)]
    delta: StreamingCompletionMessage,
    #[serde(default)]
    message: StreamingCompletionMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[cfg(feature = "ai-code-review")]
#[derive(Debug, Deserialize)]
struct StreamingCompletionError {
    message: String,
}

#[cfg(feature = "ai-code-review")]
#[derive(Debug, Deserialize)]
struct StreamingCompletionResponse {
    #[serde(default)]
    choices: Vec<StreamingCompletionChoice>,
    #[serde(default)]
    error: Option<StreamingCompletionError>,
}

#[cfg(feature = "ai-code-review")]
#[derive(Debug, Default)]
struct CompletionSseParser {
    line_buffer: Vec<u8>,
    event_data: Vec<u8>,
    content: String,
    received_bytes: usize,
    done: bool,
}

#[cfg(feature = "ai-code-review")]
impl CompletionSseParser {
    fn push(&mut self, chunk: &[u8]) -> Result<(), String> {
        self.received_bytes = self.received_bytes.saturating_add(chunk.len());
        if self.received_bytes > MAX_STREAMED_COMPLETION_BYTES {
            return Err(format!(
                "The AI provider response exceeded the {} byte limit",
                MAX_STREAMED_COMPLETION_BYTES
            ));
        }
        for byte in chunk {
            self.line_buffer.push(*byte);
            if *byte == b'\n' {
                let mut line = std::mem::take(&mut self.line_buffer);
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.process_line(line.as_slice())?;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<String, String> {
        if !self.line_buffer.is_empty() {
            let mut line = std::mem::take(&mut self.line_buffer);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.process_line(line.as_slice())?;
        }
        self.dispatch_event()?;
        if self.content.trim().is_empty() {
            return Err("The AI provider returned no completion content".to_string());
        }
        if !self.done {
            return Err("The AI provider closed the stream before a completion marker".to_string());
        }
        Ok(self.content)
    }

    fn process_line(&mut self, line: &[u8]) -> Result<(), String> {
        if line.is_empty() {
            return self.dispatch_event();
        }
        let Some(data) = line.strip_prefix(b"data:") else {
            return Ok(());
        };
        let data = data.strip_prefix(b" ").unwrap_or(data);
        if !self.event_data.is_empty() {
            self.event_data.push(b'\n');
        }
        self.event_data.extend_from_slice(data);
        Ok(())
    }

    fn dispatch_event(&mut self) -> Result<(), String> {
        if self.event_data.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.event_data);
        if data.as_slice() == b"[DONE]" {
            self.done = true;
            return Ok(());
        }
        if self.done {
            return Ok(());
        }
        let response = serde_json::from_slice::<StreamingCompletionResponse>(&data)
            .map_err(|error| format!("Invalid streaming completion event: {error}"))?;
        if let Some(error) = response.error {
            let message = error.message.chars().take(512).collect::<String>();
            return Err(format!("The AI provider returned an error: {message}"));
        }
        for choice in response.choices {
            if choice.finish_reason.is_some() {
                self.done = true;
            }
            if let Some(content) = choice.delta.content.or(choice.message.content) {
                self.content.push_str(content.as_str());
            }
        }
        Ok(())
    }
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
    complete_with_options(settings, messages, RequestOptions::new(operation), false).await
}

/// Execute an explicitly cancellable AI Review completion without a total
/// request deadline of its own. The shared connect timeout still applies, and
/// callers bound the overall duration with [`REVIEW_CHUNK_TOTAL_TIMEOUT`].
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
            .body_diagnostics(diagnostic_context),
        true,
    )
    .await
}

async fn complete_with_options(
    settings: &AiAgentSettings,
    messages: &[CompletionMessage],
    options: RequestOptions,
    stream: bool,
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
        stream,
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
            "[AI Agent][Completion] event=payload_prepared operation={} context={} model={:?} messages={} system_messages={} user_messages={} assistant_messages={} message_bytes={} largest_message_bytes={} temperature=0.1 stream={}",
            operation,
            context,
            model,
            messages.len(),
            system_messages,
            user_messages,
            assistant_messages,
            message_bytes,
            largest_message_bytes,
            stream
        );
    }
    let response = http::send(
        http::post(url)
            .bearer_auth(settings.api_key.trim())
            .json(&request),
        options,
    )
    .await?;
    #[cfg(feature = "ai-code-review")]
    if stream {
        let is_event_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
        if is_event_stream {
            let mut parser = CompletionSseParser::default();
            response
                .for_each_chunk(|chunk| {
                    parser.push(chunk).map_err(|message| {
                        Box::new(HttpError {
                            kind: HttpErrorKind::Decode,
                            operation: operation.clone(),
                            method: Some(Method::POST),
                            url: None,
                            status: None,
                            message,
                            response_body: None,
                        })
                    })
                })
                .await?;
            return parser.finish().map_err(|message| HttpError {
                kind: HttpErrorKind::Decode,
                operation,
                method: Some(Method::POST),
                url: None,
                status: None,
                message,
                response_body: None,
            });
        }
    }
    let response = response.json::<CompletionResponse>().await?;
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
    fn ordinary_completion_requests_explicitly_use_non_streaming_json_transport() {
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
    fn streamed_completion_parser_handles_split_crlf_events() {
        let mut parser = CompletionSseParser::default();
        parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel")
            .unwrap();
        parser
            .push(b"lo\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\r\n")
            .unwrap();
        parser.push(b"\r\ndata: [DONE]\r\n\r\n").unwrap();

        assert_eq!(parser.finish().unwrap(), "hello world");
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn streamed_completion_parser_accepts_message_fallback() {
        let mut parser = CompletionSseParser::default();
        parser
            .push(b"data: {\"choices\":[{\"message\":{\"content\":\"complete\"},\"finish_reason\":\"stop\"}]}\n\n")
            .unwrap();

        assert_eq!(parser.finish().unwrap(), "complete");
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn streamed_completion_parser_rejects_clean_truncation_without_terminal_marker() {
        let mut parser = CompletionSseParser::default();
        parser
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .unwrap();

        assert!(parser.finish().unwrap_err().contains("completion marker"));
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn streamed_completion_parser_surfaces_provider_error_events() {
        let mut parser = CompletionSseParser::default();
        let error = parser
            .push(b"data: {\"error\":{\"message\":\"capacity unavailable\"}}\n\n")
            .unwrap_err();

        assert!(error.contains("capacity unavailable"));
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn review_completion_allows_five_minutes_without_body_progress() {
        assert_eq!(
            REVIEW_READ_IDLE_TIMEOUT,
            std::time::Duration::from_secs(300)
        );
    }

    #[cfg(feature = "ai-code-review")]
    #[test]
    fn review_chunk_total_budget_doubles_the_idle_timeout() {
        assert_eq!(REVIEW_CHUNK_TOTAL_TIMEOUT, REVIEW_READ_IDLE_TIMEOUT * 2);
        assert_eq!(REVIEW_CHAT_TOTAL_TIMEOUT, REVIEW_CHUNK_TOTAL_TIMEOUT);
    }
}
