use anyhow::Result;

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

    // TODO: 实际应用中这里应该调用真实的 API
    // 例如：
    // let client = reqwest::Client::new();
    // let response = client.post("https://api.example.com/chat")
    //     .json(&json!({"message": user_message}))
    //     .send()
    //     .await?;
    // let result = response.json::<ChatResponse>().await?;
    // Ok(result.content)

    // 模拟 API 调用延迟
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // 模拟成功响应
    let response = format!(
        "我收到了你的消息：\"{}\"。这是一个模拟回复，实际应用中这里会调用 AI API。",
        user_message
    );

    log::info!("[Chat] Received response from server");
    Ok(response)
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
