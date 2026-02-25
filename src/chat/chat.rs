use crate::constants::chat::RAG_API_URL;
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
/// * `Err(String)` - 错误信息（用户友好的错误消息）
pub async fn send_message_to_server(user_message: &str) -> Result<String, String> {
    log::debug!("[Chat] Sending message to server: {}", user_message);

    let client = reqwest::Client::new();
    let response = client
        .post(RAG_API_URL)
        .json(&json!({
            "query": user_message,
            "mode": "hybrid",
            "include_references": true,
            "response_type": "Multiple Paragraphs",
            "top_k": 10
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let result = response
        .json::<ChatResponse>()
        .await
        .map_err(|e| e.to_string())?;

    Ok(result.response)
}

/// 处理 API 错误并转换为用户友好的错误消息
pub fn handle_api_error(error: &str) -> String {
    log::error!("[Chat] API error: {}", error);

    // 根据错误类型返回用户友好的消息
    if error.contains("network") || error.contains("timeout") {
        "网络连接失败，请检查网络设置后重试。".to_string()
    } else if error.contains("rate limit") || error.contains("429") {
        "请求过于频繁，请稍后再试。".to_string()
    } else if error.contains("unauthorized") || error.contains("401") {
        "认证失败，请检查 API 密钥。".to_string()
    } else if error.contains("server error") || error.contains("500") {
        "服务器错误，请稍后重试。".to_string()
    } else {
        format!("发送消息失败：{}", error)
    }
}
