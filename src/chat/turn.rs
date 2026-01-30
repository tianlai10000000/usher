use crate::{ChatMessage, MessageRole};
use std::sync::atomic::{AtomicI32, Ordering};

/// 消息 ID 计数器
static MESSAGE_ID_COUNTER: AtomicI32 = AtomicI32::new(1);

/// 生成下一个消息 ID
fn next_message_id() -> i32 {
    MESSAGE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// 创建用户消息
pub fn create_user_message(content: String) -> ChatMessage {
    let id = next_message_id();
    log::debug!("[Chat::Turn] Creating user message (id: {})", id);

    ChatMessage {
        id,
        role: MessageRole::User,
        content: content.into(),
        timestamp: get_current_timestamp(),
    }
}

/// 创建助手消息
pub fn create_assistant_message(content: String) -> ChatMessage {
    let id = next_message_id();
    log::debug!("[Chat::Turn] Creating assistant message (id: {})", id);

    ChatMessage {
        id,
        role: MessageRole::Assistant,
        content: content.into(),
        timestamp: get_current_timestamp(),
    }
}

/// 创建占位的 AI 消息（用于显示加载状态）
pub fn create_loading_message() -> ChatMessage {
    let id = next_message_id();
    log::debug!("[Chat::Turn] Creating loading message (id: {})", id);

    ChatMessage {
        id,
        role: MessageRole::Assistant,
        content: "".into(), // 空内容表示加载中
        timestamp: "".into(),
    }
}

/// 创建错误消息（用户友好的错误提示）
pub fn create_error_message(error: String) -> ChatMessage {
    let id = next_message_id();
    log::warn!(
        "[Chat::Turn] Creating error message (id: {}): {}",
        id,
        error
    );

    ChatMessage {
        id,
        role: MessageRole::Assistant,
        content: format!("❌ {}", error).into(),
        timestamp: get_current_timestamp(),
    }
}

/// 序列化服务器响应为 ChatMessage
///
/// # Arguments
/// * `response` - 服务器返回的原始响应
///
/// # Returns
/// * `Ok(ChatMessage)` - 成功序列化的消息
/// * `Err(String)` - 错误信息
pub fn serialize_server_response(response: String) -> Result<ChatMessage, String> {
    // 在实际应用中，这里可能需要解析 JSON 或其他格式
    // 例如：
    // let parsed: ServerResponse = serde_json::from_str(&response)?;
    // Ok(create_assistant_message(parsed.content))

    // 当前实现：直接使用响应内容
    Ok(create_assistant_message(response))
}

/// 处理服务器错误响应
///
/// # Arguments
/// * `error` - 错误信息
///
/// # Returns
/// * `ChatMessage` - 用户友好的错误消息
pub fn handle_error_response(error: String) -> ChatMessage {
    use crate::chat::chat::handle_api_error;

    let friendly_error = handle_api_error(&error);
    create_error_message(friendly_error)
}

/// 获取当前时间戳（格式化）
fn get_current_timestamp() -> slint::SharedString {
    use chrono::Local;

    let now = Local::now();
    let timestamp = now.format("%H:%M").to_string();
    timestamp.into()
}
