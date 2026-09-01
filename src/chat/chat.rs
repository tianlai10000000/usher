use crate::http::{self, HttpError, HttpErrorKind, RequestOptions};
use serde::Deserialize;
use serde_json::json;

/// RAG API 引用条目
#[derive(Debug, Deserialize)]
pub struct ChatReference {
    pub reference_id: String,
    pub file_path: String,
}

/// RAG API 响应结构
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub references: Vec<ChatReference>,
}

/// 发送消息到服务器/LLM 并获取响应
///
/// # Arguments
/// * `user_message` - 用户发送的消息内容
///
/// # Returns
/// * `Ok(String)` - 服务器返回的消息内容
/// * `Err(HttpError)` - 统一 HTTP 错误
pub(crate) async fn send_message_to_server(user_message: &str) -> Result<String, HttpError> {
    let Some(rag_api_url) = crate::settings::load_chat_rag_api_url() else {
        return Err(chat_endpoint_error("chat RAG endpoint is not configured"));
    };
    let rag_api_url = validate_rag_api_url(&rag_api_url).map_err(chat_endpoint_error)?;
    // Never log message content: chat input is user data and the log files are
    // long-lived plain text.
    log::debug!(
        "[Chat] Sending message to server ({} chars)",
        user_message.chars().count()
    );

    let result = http::send_json::<ChatResponse>(
        http::post(rag_api_url).json(&json!({
            "query": user_message,
            "mode": "hybrid",
            "include_references": true,
            "response_type": "Multiple Paragraphs",
            "top_k": 10
        })),
        // Chat already renders an inline assistant error, so suppress duplicate UI feedback.
        RequestOptions::new("Send chat message"),
    )
    .await?;

    // References are not rendered yet, but consume their metadata so the API
    // contract stays exercised without logging repository paths or identifiers.
    let reference_count = result
        .references
        .iter()
        .filter(|reference| !reference.reference_id.is_empty() || !reference.file_path.is_empty())
        .count();
    log::debug!("[Chat] Received {reference_count} non-empty reference(s)");

    Ok(result.response)
}

/// 处理 API 错误并转换为用户友好的错误消息
pub(crate) fn handle_api_error(error: &HttpError) -> String {
    log::error!("[Chat] API error: {}", error);

    let key = match (error.kind, error.status.map(|status| status.as_u16())) {
        (HttpErrorKind::Build, _) => "chat.endpoint_invalid",
        (HttpErrorKind::Timeout | HttpErrorKind::Connect, _) => "chat.network_failed",
        (HttpErrorKind::Status, Some(429)) => "chat.rate_limited",
        (HttpErrorKind::Status, Some(401 | 403)) => "chat.authentication_failed",
        (HttpErrorKind::Status, Some(500..=599)) => "chat.service_unavailable",
        (HttpErrorKind::Decode, _) => "chat.response_unreadable",
        _ => "chat.send_failed",
    };
    crate::i18n::tr(key).into_owned()
}

fn validate_rag_api_url(value: &str) -> Result<reqwest::Url, &'static str> {
    let url = reqwest::Url::parse(value).map_err(|_| "chat RAG endpoint is not a valid URL")?;
    if url.scheme() != "https" {
        return Err("chat RAG endpoint must use HTTPS");
    }
    if url.host_str().is_none() {
        return Err("chat RAG endpoint must include a host");
    }
    Ok(url)
}

fn chat_endpoint_error(message: impl Into<String>) -> HttpError {
    HttpError {
        kind: HttpErrorKind::Build,
        operation: "Send chat message".to_string(),
        method: None,
        url: None,
        status: None,
        message: message.into(),
        response_body: None,
    }
}

#[cfg(test)]
mod tests {
    use super::validate_rag_api_url;

    #[test]
    fn rag_endpoint_requires_https() {
        assert!(validate_rag_api_url("https://rag.example.com/query").is_ok());
        assert_eq!(
            validate_rag_api_url("http://10.0.0.5/query").unwrap_err(),
            "chat RAG endpoint must use HTTPS"
        );
    }

    #[test]
    fn rag_endpoint_rejects_invalid_urls() {
        assert_eq!(
            validate_rag_api_url("not a url").unwrap_err(),
            "chat RAG endpoint is not a valid URL"
        );
    }
}
