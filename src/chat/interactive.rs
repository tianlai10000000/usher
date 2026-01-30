use slint::{ComponentHandle, Model};
use crate::{MainWindow, ChatMessage, MessageRole};
use crate::chat::chat::send_message_to_server;
use crate::chat::turn::{create_user_message, create_loading_message, serialize_server_response, handle_error_response};
use std::sync::Arc;
use tokio::runtime::Runtime;

// 全局 Tokio Runtime（用于异步操作）
lazy_static::lazy_static! {
    static ref RUNTIME: Arc<Runtime> = Arc::new(Runtime::new().expect("Failed to create Tokio runtime"));
}

/// 设置聊天交互逻辑和事件回调
pub fn setup_interactive(ui: &MainWindow) {
    log::info!("[Chat::Interactive] Setting up interactive callbacks...");
    let ui_weak = ui.as_weak();
    
    // 处理发送消息回调
    ui.on_chat_send_message(move |text: slint::SharedString| {
        log::info!("[Chat::Interactive] ===== Send message callback triggered =====");
        log::debug!("[Chat::Interactive] Input text length: {}", text.len());
        log::debug!("[Chat::Interactive] Input text: {}", text);
        
        let ui_weak_local = ui_weak.clone();
        
        match ui_weak_local.upgrade() {
            Some(ui) => {
                log::debug!("[Chat::Interactive] UI upgraded successfully");
                
                // 转换为拥有的 String 用于异步闭包
                let text_str = text.to_string();
                log::debug!("[Chat::Interactive] Converted to String, length: {}", text_str.len());
                
                // 验证消息不为空
                if text_str.trim().is_empty() {
                    log::warn!("[Chat::Interactive] Attempted to send empty message, ignoring");
                    return;
                }
                
                log::debug!("[Chat::Interactive] Getting current messages...");
                // 获取当前消息列表
                let mut messages: Vec<ChatMessage> = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ui.get_chat_messages().iter().collect::<Vec<ChatMessage>>()
                })) {
                    Ok(msgs) => {
                        log::debug!("[Chat::Interactive] Successfully got {} messages", msgs.len());
                        msgs
                    }
                    Err(e) => {
                        log::error!("[Chat::Interactive] Panic while getting messages: {:?}", e);
                        return;
                    }
                };
                
                log::debug!("[Chat::Interactive] Creating user message...");
                // 创建并添加用户消息
                let user_message = create_user_message(text_str.clone());
                log::info!("[Chat::Interactive] Adding user message (id: {}, content length: {})", 
                          user_message.id, text_str.len());
                messages.push(user_message);
                
                // 添加占位的加载消息，用于显示加载状态
                let loading_message = create_loading_message();
                log::debug!("[Chat::Interactive] Adding loading placeholder message (id: {})", loading_message.id);
                messages.push(loading_message);
                
                log::debug!("[Chat::Interactive] Updating UI with user message and loading placeholder...");
                // 更新 UI：显示用户消息和加载占位符，清空输入框，设置发送中状态
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ui.set_chat_messages(messages.as_slice().into());
                    ui.set_chat_input_text("".into());
                    ui.set_chat_is_sending(true);
                })) {
                    Ok(_) => {
                        log::debug!("[Chat::Interactive] UI updated successfully");
                        
                        // 延迟设置滚动位置，确保内容已渲染
                        let ui_weak_scroll = ui.as_weak();
                        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
                            if let Some(ui) = ui_weak_scroll.upgrade() {
                                // 设置一个很大的负值来滚动到底部
                                // Flickable 会自动限制到有效范围
                                ui.set_chat_scroll_position(-99999.0);
                                log::debug!("[Chat::Interactive] Triggered scroll to bottom after user message");
                            }
                        });
                    }
                    Err(e) => {
                        log::error!("[Chat::Interactive] Panic while updating UI: {:?}", e);
                        return;
                    }
                }
                
                log::debug!("[Chat::Interactive] User message added, total messages: {}", messages.len());
                
                // 异步发送消息到服务器并处理响应
                let ui_weak_clone = ui_weak.clone();
                let text_for_api = text_str.clone();
                
                log::debug!("[Chat::Interactive] Spawning async task...");
                // 使用全局 Runtime 执行异步任务
                RUNTIME.spawn(async move {
                        log::info!("[Chat::Interactive] Async task started");
                        
                        // 发送消息到服务器
                        log::debug!("[Chat::Interactive] Calling send_message_to_server...");
                        match send_message_to_server(&text_for_api).await {
                            Ok(response) => {
                                log::info!("[Chat::Interactive] Server response received, length: {}", response.len());
                                
                                // 序列化服务器响应
                                log::debug!("[Chat::Interactive] Serializing server response...");
                                match serialize_server_response(response) {
                                    Ok(ai_message) => {
                                        log::info!("[Chat::Interactive] AI message created (id: {})", ai_message.id);
                                        
                                        // 在主线程中更新 UI
                                        log::debug!("[Chat::Interactive] Invoking from event loop to update UI...");
                                        match slint::invoke_from_event_loop(move || {
                                            log::debug!("[Chat::Interactive] Event loop callback executing");
                                            
                                            match ui_weak_clone.upgrade() {
                                                Some(ui) => {
                                                    log::debug!("[Chat::Interactive] UI upgraded in event loop");
                                                    
                                                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                                        let mut messages: Vec<ChatMessage> = ui.get_chat_messages().iter().collect::<Vec<ChatMessage>>();
                                                        
                                                        // 移除加载占位消息（空白回复）
                                                        if messages.len() > 0 && 
                                                           messages[messages.len() - 1].role == MessageRole::Assistant &&
                                                           messages[messages.len() - 1].content.as_str() == "" {
                                                            log::debug!("[Chat::Interactive] Removing loading placeholder message before adding AI response");
                                                            messages.pop();
                                                        }
                                                        
                                                        // 添加 AI 回复
                                                        messages.push(ai_message);
                                                        ui.set_chat_messages(messages.as_slice().into());
                                                        ui.set_chat_is_sending(false);
                                                        
                                                        messages.len()
                                                    })) {
                                                        Ok(count) => {
                                                            log::info!("[Chat::Interactive] AI response added successfully, total messages: {}", count);
                                                            
                                                            // 延迟设置滚动位置，确保内容已渲染
                                                            let ui_weak_scroll = ui.as_weak();
                                                            slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
                                                                if let Some(ui) = ui_weak_scroll.upgrade() {
                                                                    ui.set_chat_scroll_position(-99999.0);
                                                                    log::debug!("[Chat::Interactive] Triggered scroll to bottom after AI response");
                                                                }
                                                            });
                                                        }
                                                        Err(e) => {
                                                            log::error!("[Chat::Interactive] Panic while updating UI with AI message: {:?}", e);
                                                        }
                                                    }
                                                }
                                                None => {
                                                    log::warn!("[Chat::Interactive] UI weak reference expired in event loop");
                                                }
                                            }
                                        }) {
                                            Ok(_) => {
                                                log::debug!("[Chat::Interactive] Event loop invocation successful");
                                            }
                                            Err(e) => {
                                                log::error!("[Chat::Interactive] Failed to invoke from event loop: {:?}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("[Chat::Interactive] Failed to serialize response: {}", e);
                                        let error_message = handle_error_response(e);
                                        
                                        slint::invoke_from_event_loop(move || {
                                            if let Some(ui) = ui_weak_clone.upgrade() {
                                                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                                    let mut messages: Vec<ChatMessage> = ui.get_chat_messages().iter().collect::<Vec<ChatMessage>>();
                                                    
                                                    // 移除加载占位消息
                                                    if messages.len() > 0 && 
                                                       messages[messages.len() - 1].role == MessageRole::Assistant &&
                                                       messages[messages.len() - 1].content.as_str() == "" {
                                                        messages.pop();
                                                    }
                                                    
                                                    messages.push(error_message);
                                                    ui.set_chat_messages(messages.as_slice().into());
                                                    ui.set_chat_is_sending(false);
                                                })) {
                                                    Ok(_) => log::debug!("[Chat::Interactive] Error message added to UI"),
                                                    Err(e) => log::error!("[Chat::Interactive] Panic while adding error message: {:?}", e),
                                                }
                                            }
                                        }).ok();
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("[Chat::Interactive] Failed to send message to server: {}", e);
                                let error_message = handle_error_response(e);
                                
                                slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_weak_clone.upgrade() {
                                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            let mut messages: Vec<ChatMessage> = ui.get_chat_messages().iter().collect::<Vec<ChatMessage>>();
                                            
                                            // 移除加载占位消息
                                            if messages.len() > 0 && 
                                               messages[messages.len() - 1].role == MessageRole::Assistant &&
                                               messages[messages.len() - 1].content.as_str() == "" {
                                                messages.pop();
                                            }
                                            
                                            messages.push(error_message);
                                            ui.set_chat_messages(messages.as_slice().into());
                                            ui.set_chat_is_sending(false);
                                        })) {
                                            Ok(_) => log::debug!("[Chat::Interactive] Error message added to UI"),
                                            Err(e) => log::error!("[Chat::Interactive] Panic while adding error message: {:?}", e),
                                        }
                                    }
                                }).ok();
                            }
                        }
                        
                        log::info!("[Chat::Interactive] Async task completed");
                    });
                
                log::debug!("[Chat::Interactive] Task spawned successfully");
            }
            None => {
                log::error!("[Chat::Interactive] Failed to upgrade UI weak reference");
            }
        }
        
        log::info!("[Chat::Interactive] ===== Send message callback completed =====");
    });
    
    // 处理复制到剪贴板回调
    let ui_weak_copy = ui.as_weak();
    ui.on_chat_copy_to_clipboard(move |index: i32, text: slint::SharedString| {
        let text_str = text.to_string();
        let text_len = text_str.chars().count();
        let preview = if text_len > 50 {
            let short: String = text_str.chars().take(50).collect();
            format!("{}...", short)
        } else {
            text_str.clone()
        };
        
        log::info!("[Chat::Copy] Step 1: Copy button clicked, message index: {}, content length: {} chars", index, text_len);
        log::debug!("[Chat::Copy] Content preview: \"{}\"", preview);
        
        log::info!("[Chat::Copy] Step 2: Creating clipboard instance...");
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                log::info!("[Chat::Copy] Step 3: Clipboard instance created, setting text...");
                match clipboard.set_text(&text_str) {
                    Ok(_) => {
                        log::info!("[Chat::Copy] Step 4: Copy SUCCESS! Text copied to clipboard ({} chars)", text_len);
                        
                        // 设置复制成功的消息索引
                        if let Some(ui) = ui_weak_copy.upgrade() {
                            log::info!("[Chat::Copy] Step 5: Setting copied-message-index to {}", index);
                            ui.set_chat_copied_message_index(index);
                            
                            // 2s 后重置状态
                            let ui_weak_timer = ui.as_weak();
                            slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
                                log::info!("[Chat::Copy] Step 6: Timer triggered, resetting copied-message-index to -1");
                                if let Some(ui) = ui_weak_timer.upgrade() {
                                    ui.set_chat_copied_message_index(-1);
                                    log::info!("[Chat::Copy] Step 7: Icon restored to 'copy', ready for next copy");
                                }
                            });
                        }
                    }
                    Err(e) => {
                        log::error!("[Chat::Copy] Step 4: Copy FAILED! Error setting text: {}", e);
                    }
                }
            }
            Err(e) => {
                log::error!("[Chat::Copy] Step 3: FAILED to create clipboard instance: {}", e);
            }
        }
    });
    
    log::info!("[Chat::Interactive] Interactive callbacks initialized successfully");
}
