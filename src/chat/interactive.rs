use crate::chat::chat::send_message_to_server;
use crate::chat::markdown::build_assistant_render_payload_with_wrap_cols;
use crate::chat::turn::{
    create_loading_message, create_user_message, handle_error_response, serialize_server_response,
};
use crate::logger::tracing_logger;
use crate::{ChatMessage, MainWindow, MessageRole, Route};
use slint::{ComponentHandle, Model};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};

fn compute_wrap_cols(ui: &MainWindow) -> i32 {
    // 与 chat.slint 的断点策略保持一致，按窗口可用宽度估算气泡宽度
    let window_width = ui.get_chat_layout_window_width();
    let nav_width = if ui.get_navigation_drawer_expanded() {
        120.0
    } else {
        56.0
    };
    let chat_content_width = (window_width - nav_width - 24.0).max(320.0);

    let ratio = if chat_content_width < 720.0 {
        0.58
    } else if chat_content_width < 1200.0 {
        0.62
    } else {
        0.66
    };
    let bubble_width = (chat_content_width * ratio).clamp(280.0, 960.0);
    (((bubble_width - 24.0) / 8.4).floor() as i32).clamp(16, 140)
}

fn schedule_markdown_reflow_poll_with_interval(
    ui_weak: slint::Weak<MainWindow>,
    last_wrap_cols: Rc<Cell<i32>>,
    polling_active: Rc<Cell<bool>>,
    interval_ms: u64,
) {
    slint::Timer::single_shot(std::time::Duration::from_millis(interval_ms), move || {
        if !polling_active.get() {
            return;
        }

        if let Some(ui) = ui_weak.upgrade() {
            // 仅在 Chat 页面可见时执行重排，减少无关页面开销
            if ui.get_current_route() != Route::Chat {
                polling_active.set(false);
                return;
            }

            let cols = compute_wrap_cols(&ui);
            if cols != last_wrap_cols.get() {
                last_wrap_cols.set(cols);

                let mut messages: Vec<ChatMessage> = ui.get_chat_messages().iter().collect();
                let mut changed = false;

                for message in &mut messages {
                    if message.role == MessageRole::Assistant && message.content.as_str() != "" {
                        let raw_content = message.content.to_string();
                        let payload = build_assistant_render_payload_with_wrap_cols(
                            &raw_content,
                            Some(cols as usize),
                        );

                        if message.is_markdown || payload.is_markdown {
                            message.rendered_content = payload.rendered_text.into();
                            message.is_markdown = payload.is_markdown;
                            message.markdown_blocks = payload.blocks.as_slice().into();
                            changed = true;
                        }
                    }
                }

                if changed {
                    ui.set_chat_messages(messages.as_slice().into());
                    log::debug!(
                        "[Chat::Wrap] Reflowed assistant markdown with cols={}",
                        cols
                    );
                }
            }

            schedule_markdown_reflow_poll_with_interval(
                ui_weak.clone(),
                last_wrap_cols.clone(),
                polling_active.clone(),
                250,
            );
        } else {
            polling_active.set(false);
        }
    });
}

fn start_markdown_reflow_poll_if_needed(
    ui_weak: slint::Weak<MainWindow>,
    last_wrap_cols: Rc<Cell<i32>>,
    polling_active: Rc<Cell<bool>>,
) {
    if polling_active.get() {
        return;
    }
    polling_active.set(true);
    schedule_markdown_reflow_poll_with_interval(ui_weak, last_wrap_cols, polling_active, 0);
}

// 全局 Tokio Runtime（用于异步操作）
lazy_static::lazy_static! {
    static ref RUNTIME: Arc<Runtime> = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .on_thread_start(|| tracing_logger::install_thread_dispatch())
            .build()
            .expect("Failed to create Tokio runtime")
    );
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

                                // 在主线程中进行序列化与 UI 更新，避免跨线程传递 Slint Model 数据
                                log::debug!("[Chat::Interactive] Invoking from event loop to update UI...");
                                match slint::invoke_from_event_loop(move || {
                                    log::debug!("[Chat::Interactive] Event loop callback executing");

                                    if let Some(ui) = ui_weak_clone.upgrade() {
                                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                            let mut messages: Vec<ChatMessage> =
                                                ui.get_chat_messages().iter().collect::<Vec<ChatMessage>>();

                                            // 移除加载占位消息（空白回复）
                                            if !messages.is_empty()
                                                && messages[messages.len() - 1].role == MessageRole::Assistant
                                                && messages[messages.len() - 1].content.as_str() == ""
                                            {
                                                log::debug!("[Chat::Interactive] Removing loading placeholder message before adding AI response");
                                                messages.pop();
                                            }

                                            // 在 UI 线程序列化，避免把 ChatMessage(含 ModelRc) 从 worker 线程 move 过来
                                            match serialize_server_response(response) {
                                                Ok(ai_message) => messages.push(ai_message),
                                                Err(err) => messages.push(handle_error_response(err)),
                                            }

                                            ui.set_chat_messages(messages.as_slice().into());
                                            ui.set_chat_is_sending(false);
                                        })) {
                                            Ok(_) => {
                                                log::info!("[Chat::Interactive] Response added successfully");
                                                let ui_weak_scroll = ui.as_weak();
                                                slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
                                                    if let Some(ui) = ui_weak_scroll.upgrade() {
                                                        ui.set_chat_scroll_position(-99999.0);
                                                        log::debug!("[Chat::Interactive] Triggered scroll to bottom after AI response");
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                log::error!("[Chat::Interactive] Panic while updating UI with response: {:?}", e);
                                            }
                                        }
                                    } else {
                                        log::warn!("[Chat::Interactive] UI weak reference expired in event loop");
                                    }
                                }) {
                                    Ok(_) => log::debug!("[Chat::Interactive] Event loop invocation successful"),
                                    Err(e) => log::error!("[Chat::Interactive] Failed to invoke from event loop: {:?}", e),
                                }
                            }
                            Err(e) => {
                                log::error!("[Chat::Interactive] Failed to send message to server: {}", e);

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

                                            let error_message = handle_error_response(e);
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

    // 处理打开链接回调（来自 Markdown 渲染块）
    ui.on_chat_open_link(move |url: slint::SharedString| {
        let url = url.to_string();
        if url.trim().is_empty() {
            return;
        }

        match webbrowser::open(&url) {
            Ok(_) => log::info!("[Chat::Link] Opened URL: {}", url),
            Err(e) => log::error!("[Chat::Link] Failed to open URL '{}': {}", url, e),
        }
    });

    // 仅在 Chat 页面可见时启动轮询；离开 Chat 立即停止
    let last_wrap_cols = Rc::new(Cell::new(-1));
    let polling_active = Rc::new(Cell::new(false));
    if ui.get_current_route() == Route::Chat {
        start_markdown_reflow_poll_if_needed(
            ui.as_weak(),
            last_wrap_cols.clone(),
            polling_active.clone(),
        );
    }

    let ui_weak_route = ui.as_weak();
    let last_wrap_cols_route = last_wrap_cols.clone();
    let polling_active_route = polling_active.clone();
    ui.on_chat_route_visible_changed(move |visible| {
        if visible {
            start_markdown_reflow_poll_if_needed(
                ui_weak_route.clone(),
                last_wrap_cols_route.clone(),
                polling_active_route.clone(),
            );
        } else {
            polling_active_route.set(false);
        }
    });

    log::info!("[Chat::Interactive] Interactive callbacks initialized successfully");
}
