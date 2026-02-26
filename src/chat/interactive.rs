use crate::chat::chat::send_message_to_server;
use crate::chat::markdown::{build_assistant_render_payload_with_wrap_width, looks_like_markdown};
use crate::chat::turn::{
    create_loading_message, create_user_message, handle_error_response, serialize_server_response,
};
use crate::logger::tracing_logger;
use crate::{ChatMessage, MainWindow, MessageRole, Route};
use slint::{ComponentHandle, Model};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::runtime::{Builder, Runtime};

// 微调参数：适当放宽可用宽度预算，降低中文段落过早换行概率
const WRAP_WIDTH_TUNING_FACTOR: f32 = 1.10;
// 性能参数：避免窗口拖拽时频繁重排导致卡顿
const REFLOW_POLL_INTERVAL_MS: u64 = 120;
const REFLOW_STABLE_POLLS_REQUIRED: u8 = 6; // 约 720ms 稳定后再重排
const REFLOW_MIN_WIDTH_DELTA_PX: i32 = 24; // 宽度变化过小时不触发重排
const REFLOW_RECENT_MESSAGE_LIMIT: usize = 40; // 大会话仅重排最近消息，降低主线程压力
const LOADING_PHASE_INTERVAL_MS: u64 = 500; // 2Hz 点点动画
const LOADING_PHASE_PAUSE_AFTER_RESIZE_TICKS: u8 = 10; // 约 1.2s 内暂停点点动画

fn compute_wrap_width_px(ui: &MainWindow) -> i32 {
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
    // 扣除气泡左右内边距（12px * 2）后，得到 markdown 内容可用宽度
    let bubble_width = (chat_content_width * ratio).clamp(280.0, 960.0);
    let content_width = (bubble_width - 24.0) * WRAP_WIDTH_TUNING_FACTOR;
    content_width.round().clamp(200.0, 936.0) as i32
}

fn schedule_markdown_reflow_poll_with_interval(
    ui_weak: slint::Weak<MainWindow>,
    last_wrap_width_px: Rc<Cell<i32>>,
    observed_width_px: Rc<Cell<i32>>,
    pending_reflow_width_px: Rc<Cell<i32>>,
    stable_polls: Rc<Cell<u8>>,
    polling_active: Rc<Cell<bool>>,
    deferred_log_printed: Rc<Cell<bool>>,
    resize_activity_ticks: Rc<Cell<u8>>,
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

            let wrap_width_px = compute_wrap_width_px(&ui);
            // 宽度变化期间先记录，不立即重排；等宽度稳定后再执行一次重排，避免拖拽时卡顿
            if wrap_width_px != observed_width_px.get() {
                let prev_width = observed_width_px.get();
                observed_width_px.set(wrap_width_px);
                pending_reflow_width_px.set(wrap_width_px);
                stable_polls.set(0);
                resize_activity_ticks.set(LOADING_PHASE_PAUSE_AFTER_RESIZE_TICKS);
                if prev_width < 0 || (wrap_width_px - prev_width).abs() >= 48 {
                    log::debug!(
                        "[Chat::Wrap] Resize observed width change: {} -> {}",
                        prev_width,
                        wrap_width_px
                    );
                }
            } else if pending_reflow_width_px.get() != -1 {
                let next = stable_polls.get().saturating_add(1);
                stable_polls.set(next);
            }
            if resize_activity_ticks.get() > 0 {
                resize_activity_ticks.set(resize_activity_ticks.get().saturating_sub(1));
            }

            let should_reflow = pending_reflow_width_px.get() != -1
                && stable_polls.get() >= REFLOW_STABLE_POLLS_REQUIRED
                && pending_reflow_width_px.get() != last_wrap_width_px.get();

            if should_reflow {
                // 关键优化：等待回复期间暂停 markdown 重排，避免与发送态 UI 更新叠加导致拖拽卡顿。
                // 只保留最新目标宽度，等回复结束后再进行一次重排。
                if ui.get_chat_is_sending() {
                    if !deferred_log_printed.get() {
                        log::debug!(
                            "[Chat::Wrap] Reflow deferred while sending (pending_width_px={})",
                            pending_reflow_width_px.get()
                        );
                        deferred_log_printed.set(true);
                    }
                    schedule_markdown_reflow_poll_with_interval(
                        ui_weak.clone(),
                        last_wrap_width_px.clone(),
                        observed_width_px.clone(),
                        pending_reflow_width_px.clone(),
                        stable_polls.clone(),
                        polling_active.clone(),
                        deferred_log_printed.clone(),
                        resize_activity_ticks.clone(),
                        REFLOW_POLL_INTERVAL_MS,
                    );
                    return;
                }
                // 发送结束后重置一次性日志开关，确保下次发送还能打印一次命中日志
                deferred_log_printed.set(false);

                let target_width_px = pending_reflow_width_px.get();
                let width_delta = if last_wrap_width_px.get() < 0 {
                    i32::MAX
                } else {
                    (target_width_px - last_wrap_width_px.get()).abs()
                };
                if width_delta < REFLOW_MIN_WIDTH_DELTA_PX {
                    // 太小的宽度变化直接吞掉，避免频繁重排卡顿
                    log::debug!(
                        "[Chat::Wrap] Reflow skipped: width delta {}px < {}px",
                        width_delta,
                        REFLOW_MIN_WIDTH_DELTA_PX
                    );
                    last_wrap_width_px.set(target_width_px);
                    pending_reflow_width_px.set(-1);
                    stable_polls.set(0);
                    schedule_markdown_reflow_poll_with_interval(
                        ui_weak.clone(),
                        last_wrap_width_px.clone(),
                        observed_width_px.clone(),
                        pending_reflow_width_px.clone(),
                        stable_polls.clone(),
                        polling_active.clone(),
                        deferred_log_printed.clone(),
                        resize_activity_ticks.clone(),
                        REFLOW_POLL_INTERVAL_MS,
                    );
                    return;
                }
                last_wrap_width_px.set(target_width_px);
                pending_reflow_width_px.set(-1);
                stable_polls.set(0);
                let reflow_start = Instant::now();

                let mut messages: Vec<ChatMessage> = ui.get_chat_messages().iter().collect();
                let mut changed = false;
                let start_index = messages.len().saturating_sub(REFLOW_RECENT_MESSAGE_LIMIT);
                let mut scanned_assistant_messages = 0usize;
                let mut reparsed_markdown_messages = 0usize;

                for message in messages.iter_mut().skip(start_index) {
                    if message.role == MessageRole::Assistant && message.content.as_str() != "" {
                        scanned_assistant_messages += 1;
                        let raw_content = message.content.as_str();
                        if !message.is_markdown && !looks_like_markdown(raw_content) {
                            continue;
                        }
                        let payload = build_assistant_render_payload_with_wrap_width(
                            raw_content,
                            Some(target_width_px as f32),
                        );

                        if message.is_markdown || payload.is_markdown {
                            reparsed_markdown_messages += 1;
                            message.rendered_content = payload.rendered_text.into();
                            message.is_markdown = payload.is_markdown;
                            message.markdown_blocks = payload.blocks.as_slice().into();
                            changed = true;
                        }
                    }
                }

                if changed {
                    let apply_start = Instant::now();
                    ui.set_chat_messages(messages.as_slice().into());
                    let apply_ms = apply_start.elapsed().as_millis();
                    let total_ms = reflow_start.elapsed().as_millis();
                    log::debug!(
                        "[Chat::Wrap] Reflowed markdown width_px={} scanned={} reparsed={} total_ms={} apply_ms={}",
                        target_width_px,
                        scanned_assistant_messages,
                        reparsed_markdown_messages,
                        total_ms,
                        apply_ms
                    );
                    if total_ms > 24 {
                        log::warn!(
                            "[Chat::Perf] Slow reflow detected width_px={} total_ms={} apply_ms={} scanned={} reparsed={}",
                            target_width_px,
                            total_ms,
                            apply_ms,
                            scanned_assistant_messages,
                            reparsed_markdown_messages
                        );
                    }
                } else {
                    let total_ms = reflow_start.elapsed().as_millis();
                    log::debug!(
                        "[Chat::Wrap] Reflow no-op width_px={} scanned={} total_ms={}",
                        target_width_px,
                        scanned_assistant_messages,
                        total_ms
                    );
                }
            }

            schedule_markdown_reflow_poll_with_interval(
                ui_weak.clone(),
                last_wrap_width_px.clone(),
                observed_width_px.clone(),
                pending_reflow_width_px.clone(),
                stable_polls.clone(),
                polling_active.clone(),
                deferred_log_printed.clone(),
                resize_activity_ticks.clone(),
                REFLOW_POLL_INTERVAL_MS,
            );
        } else {
            polling_active.set(false);
        }
    });
}

fn start_markdown_reflow_poll_if_needed(
    ui_weak: slint::Weak<MainWindow>,
    last_wrap_width_px: Rc<Cell<i32>>,
    observed_width_px: Rc<Cell<i32>>,
    pending_reflow_width_px: Rc<Cell<i32>>,
    stable_polls: Rc<Cell<u8>>,
    polling_active: Rc<Cell<bool>>,
    deferred_log_printed: Rc<Cell<bool>>,
    resize_activity_ticks: Rc<Cell<u8>>,
) {
    if polling_active.get() {
        return;
    }
    polling_active.set(true);
    schedule_markdown_reflow_poll_with_interval(
        ui_weak,
        last_wrap_width_px,
        observed_width_px,
        pending_reflow_width_px,
        stable_polls,
        polling_active,
        deferred_log_printed,
        resize_activity_ticks,
        0,
    );
}

fn schedule_loading_phase_tick(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
    resize_activity_ticks: Rc<Cell<u8>>,
) {
    slint::Timer::single_shot(
        std::time::Duration::from_millis(LOADING_PHASE_INTERVAL_MS),
        move || {
            if !loading_phase_active.load(Ordering::Relaxed) {
                return;
            }

            if let Some(ui) = ui_weak.upgrade() {
                if ui.get_chat_is_sending() && ui.get_current_route() == Route::Chat {
                    if resize_activity_ticks.get() == 0 {
                        let next = (ui.get_chat_loading_phase() + 1) % 3;
                        ui.set_chat_loading_phase(next);
                    }
                }
                schedule_loading_phase_tick(
                    ui_weak.clone(),
                    loading_phase_active.clone(),
                    resize_activity_ticks.clone(),
                );
            } else {
                loading_phase_active.store(false, Ordering::Relaxed);
            }
        },
    );
}

fn start_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
    resize_activity_ticks: Rc<Cell<u8>>,
) {
    if loading_phase_active.load(Ordering::Relaxed) {
        return;
    }
    loading_phase_active.store(true, Ordering::Relaxed);
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_chat_loading_phase(0);
    }
    schedule_loading_phase_tick(ui_weak, loading_phase_active, resize_activity_ticks);
}

fn stop_loading_phase_animation(
    ui_weak: slint::Weak<MainWindow>,
    loading_phase_active: Arc<AtomicBool>,
) {
    loading_phase_active.store(false, Ordering::Relaxed);
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_chat_loading_phase(0);
    }
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
    let loading_phase_active = Arc::new(AtomicBool::new(false));
    let loading_phase_active_send = loading_phase_active.clone();
    let resize_activity_ticks = Rc::new(Cell::new(0u8));
    let resize_activity_ticks_send = resize_activity_ticks.clone();

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
                        start_loading_phase_animation(
                            ui.as_weak(),
                            loading_phase_active_send.clone(),
                            resize_activity_ticks_send.clone(),
                        );
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
                let loading_phase_active_response = loading_phase_active_send.clone();

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
                                        loading_phase_active_response.store(false, Ordering::Relaxed);
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
                                            ui.set_chat_loading_phase(0);
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
                                        loading_phase_active_response.store(false, Ordering::Relaxed);
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
                                            ui.set_chat_loading_phase(0);
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
    let last_wrap_width_px = Rc::new(Cell::new(-1));
    let observed_width_px = Rc::new(Cell::new(-1));
    let pending_reflow_width_px = Rc::new(Cell::new(-1));
    let stable_polls = Rc::new(Cell::new(0u8));
    let polling_active = Rc::new(Cell::new(false));
    let deferred_log_printed = Rc::new(Cell::new(false));
    if ui.get_current_route() == Route::Chat {
        start_markdown_reflow_poll_if_needed(
            ui.as_weak(),
            last_wrap_width_px.clone(),
            observed_width_px.clone(),
            pending_reflow_width_px.clone(),
            stable_polls.clone(),
            polling_active.clone(),
            deferred_log_printed.clone(),
            resize_activity_ticks.clone(),
        );
    }

    let ui_weak_route = ui.as_weak();
    let last_wrap_width_px_route = last_wrap_width_px.clone();
    let observed_width_px_route = observed_width_px.clone();
    let pending_reflow_width_px_route = pending_reflow_width_px.clone();
    let stable_polls_route = stable_polls.clone();
    let polling_active_route = polling_active.clone();
    let deferred_log_printed_route = deferred_log_printed.clone();
    let resize_activity_ticks_route = resize_activity_ticks.clone();
    let loading_phase_active_route = loading_phase_active.clone();
    ui.on_chat_route_visible_changed(move |visible| {
        if visible {
            start_markdown_reflow_poll_if_needed(
                ui_weak_route.clone(),
                last_wrap_width_px_route.clone(),
                observed_width_px_route.clone(),
                pending_reflow_width_px_route.clone(),
                stable_polls_route.clone(),
                polling_active_route.clone(),
                deferred_log_printed_route.clone(),
                resize_activity_ticks_route.clone(),
            );
        } else {
            polling_active_route.set(false);
            deferred_log_printed_route.set(false);
            resize_activity_ticks_route.set(0);
            stop_loading_phase_animation(ui_weak_route.clone(), loading_phase_active_route.clone());
        }
    });

    log::info!("[Chat::Interactive] Interactive callbacks initialized successfully");
}
