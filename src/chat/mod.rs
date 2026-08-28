// `chat::chat` is the established transport module path used throughout the app.
#[allow(clippy::module_inception)]
pub mod chat;
pub mod interactive;
pub mod markdown;
pub(crate) mod performance;
pub mod persistence;
pub mod renderer;
pub mod session;
mod text_selection;
mod timestamp;
mod virtual_layout;

use crate::MainWindow;

/// 初始化聊天模块
/// 设置所有聊天相关的回调和处理逻辑
pub fn setup_chat(ui: &MainWindow) {
    log::info!("[Chat] Initializing chat module");

    text_selection::setup(ui);

    // 设置交互逻辑和事件回调
    interactive::setup_interactive(ui);

    log::info!("[Chat] Chat module initialized successfully");
}
