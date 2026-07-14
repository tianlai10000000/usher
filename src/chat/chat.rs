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

    Ok(result.response)
}

/// 处理 API 错误并转换为用户友好的错误消息
pub(crate) fn handle_api_error(error: &HttpError) -> String {
    log::error!("[Chat] API error: {}", error);

    match (error.kind, error.status.map(|status| status.as_u16())) {
        (HttpErrorKind::Timeout | HttpErrorKind::Connect, _) => {
            "网络连接失败，请检查网络设置后重试。".to_string()
        }
        (HttpErrorKind::Status, Some(429)) => "请求过于频繁，请稍后再试。".to_string(),
        (HttpErrorKind::Status, Some(401 | 403)) => {
            "认证失败，请检查 API 密钥或访问权限。".to_string()
        }
        (HttpErrorKind::Status, Some(500..=599)) => "服务器错误，请稍后重试。".to_string(),
        (HttpErrorKind::Decode, _) => "服务返回了无法识别的数据。".to_string(),
        _ => format!("发送消息失败：{}", error.user_message()),
    }
}
