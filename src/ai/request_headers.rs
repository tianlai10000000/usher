//! OpenAI-compatible request headers with explicit credential boundaries.

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};

use super::agent::configuration_error;
use crate::http::HttpError;
use crate::settings::AiAgentSettings;

#[allow(clippy::result_large_err)]
pub(super) fn build(settings: &AiAgentSettings) -> Result<HeaderMap, HttpError> {
    let mut headers = custom_headers(settings)?;
    if !settings.api_key.trim().is_empty() && !headers.contains_key(AUTHORIZATION) {
        let mut value = HeaderValue::from_str(&format!("Bearer {}", settings.api_key.trim()))
            .map_err(|_| configuration_error("Invalid AI Agent API key"))?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }
    if settings.api_key.trim().is_empty()
        && !headers.values().any(|value| !value.as_bytes().is_empty())
    {
        return Err(configuration_error(
            "OpenAI API Key or an enabled authentication header is required",
        ));
    }
    Ok(headers)
}

/// Validate a saved/draft header table without requiring a usable connection.
#[allow(clippy::result_large_err)]
pub(crate) fn validate(settings: &AiAgentSettings) -> Result<(), HttpError> {
    custom_headers(settings).map(|_| ())
}

#[allow(clippy::result_large_err)]
fn custom_headers(settings: &AiAgentSettings) -> Result<HeaderMap, HttpError> {
    if settings.headers.len() > crate::settings::MAX_AI_AGENT_HEADERS {
        return Err(configuration_error("Too many AI Agent headers"));
    }
    let mut headers = HeaderMap::new();
    for header in settings.headers.iter().filter(|header| header.enabled) {
        if header.name.contains(['\r', '\n']) {
            return Err(configuration_error("Invalid AI Agent header name"));
        }
        let name = HeaderName::from_bytes(header.name.trim().as_bytes())
            .map_err(|_| configuration_error("Invalid AI Agent header name"))?;
        if matches!(
            name.as_str(),
            "host"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "proxy-connection"
                | "te"
                | "trailer"
                | "upgrade"
                | "proxy-authorization"
        ) {
            return Err(configuration_error(
                "AI Agent header controls transport and is not allowed",
            ));
        }
        let mut value = HeaderValue::from_str(&header.value)
            .map_err(|_| configuration_error("Invalid AI Agent header value"))?;
        // Arbitrarily named credentials must select the shared origin-bound
        // redirect client and remain redacted in HeaderMap/Request diagnostics.
        value.set_sensitive(true);
        headers.append(name, value);
    }
    Ok(headers)
}

#[cfg(test)]
#[path = "request_headers_tests.rs"]
mod tests;
