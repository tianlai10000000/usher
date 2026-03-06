pub mod chat;
pub mod interactive;
pub mod markdown;
pub mod persistence;
pub mod renderer;
pub mod session;

use crate::MainWindow;

/// 初始化聊天模块
/// 设置所有聊天相关的回调和处理逻辑
pub fn setup_chat(ui: &MainWindow) {
    log::info!("[Chat] Initializing chat module");

    // 设置交互逻辑和事件回调
    interactive::setup_interactive(ui);

    log::info!("[Chat] Chat module initialized successfully");
}
