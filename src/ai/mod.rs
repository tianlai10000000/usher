//! Independent AI Agent integration boundary.
//!
//! `src/chat/` owns the separate RAG chat experience and must not be changed
//! by AI Agent work.

pub(crate) mod agent;
pub(crate) mod identity;
pub(crate) mod prompts;
pub(crate) mod request_headers;
pub(crate) mod settings_state;
pub(crate) mod settings_ui;
