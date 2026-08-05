use crate::constants::chat::RAG_API_URL;
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
    log::debug!("[Chat] Sending message to server: {}", user_message);

    let result = http::send_json::<ChatResponse>(
        http::post(RAG_API_URL).json(&json!({
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

    match (error.kind, error.status.map(|status| status.as_u16())) {
        (HttpErrorKind::Timeout | HttpErrorKind::Connect, _) => {
            "Network connection failed. Check your network settings and try again.".to_string()
        }
        (HttpErrorKind::Status, Some(429)) => "Too many requests. Try again shortly.".to_string(),
        (HttpErrorKind::Status, Some(401 | 403)) => {
            "Authentication failed. Check the API key or access permissions.".to_string()
        }
        (HttpErrorKind::Status, Some(500..=599)) => {
            "The service is unavailable. Try again later.".to_string()
        }
        (HttpErrorKind::Decode, _) => "The service returned an unreadable response.".to_string(),
        _ => "The message could not be sent. Try again.".to_string(),
    }
}
