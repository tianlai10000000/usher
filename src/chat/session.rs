use super::persistence::{
    AI_HISTORY_FILE_NAME, HistoryPage, JsonlHistoryStore, StoredChatMessage, StoredMessageRole,
    StoredMessageStatus,
};
use std::io;
use std::time::{Duration, Instant};

const DEFAULT_STARTUP_LOAD: usize = 20;
const DEFAULT_PAGE_SIZE: usize = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ChatMode {
    #[default]
    Ai,
    Rag,
}

#[derive(Debug, Default)]
pub(crate) struct ChatRequestState {
    ai_sending: bool,
    rag_sending: bool,
    ai_started_at: Option<Instant>,
    rag_started_at: Option<Instant>,
}

impl ChatRequestState {
    pub(crate) fn is_sending(&self, mode: ChatMode) -> bool {
        match mode {
            ChatMode::Ai => self.ai_sending,
            ChatMode::Rag => self.rag_sending,
        }
    }

    pub(crate) fn start(&mut self, mode: ChatMode, started_at: Instant) {
        match mode {
            ChatMode::Ai => {
                self.ai_sending = true;
                self.ai_started_at = Some(started_at);
            }
            ChatMode::Rag => {
                self.rag_sending = true;
                self.rag_started_at = Some(started_at);
            }
        }
    }

    pub(crate) fn finish(&mut self, mode: ChatMode) -> Option<Duration> {
        match mode {
            ChatMode::Ai => {
                self.ai_sending = false;
                self.ai_started_at.take().map(|started| started.elapsed())
            }
            ChatMode::Rag => {
                self.rag_sending = false;
                self.rag_started_at.take().map(|started| started.elapsed())
            }
        }
    }

    pub(crate) fn elapsed_seconds(&self, mode: ChatMode) -> u64 {
        match mode {
            ChatMode::Ai => self.ai_started_at,
            ChatMode::Rag => self.rag_started_at,
        }
        .map_or(0, |started| started.elapsed().as_secs())
    }
}

#[derive(Debug)]
pub struct ChatSessionService {
    store: JsonlHistoryStore,
    loaded_messages: Vec<StoredChatMessage>,
    can_load_more: bool,
    next_id: i64,
    page_size: usize,
}

impl ChatSessionService {
    pub fn bootstrap_default() -> io::Result<Self> {
        let store = JsonlHistoryStore::new_default()?;
        Self::bootstrap(store, DEFAULT_STARTUP_LOAD, DEFAULT_PAGE_SIZE)
    }

    pub fn bootstrap(
        store: JsonlHistoryStore,
        startup_load: usize,
        page_size: usize,
    ) -> io::Result<Self> {
        let loaded_messages = store.load_recent(startup_load)?;
        let latest_id = loaded_messages
            .last()
            .map(|m| m.id)
            .unwrap_or(store.latest_message_id()?);
        let can_load_more = loaded_messages
            .first()
            .map(|m| store.has_older_than(m.id))
            .transpose()?
            .unwrap_or(false);

        Ok(Self {
            store,
            loaded_messages,
            can_load_more,
            next_id: latest_id.saturating_add(1),
            page_size,
        })
    }

    pub fn loaded_messages(&self) -> &[StoredChatMessage] {
        &self.loaded_messages
    }

    pub fn can_load_more(&self) -> bool {
        self.can_load_more
    }

    pub fn load_more_history(&mut self) -> io::Result<usize> {
        let Some(first) = self.loaded_messages.first() else {
            self.can_load_more = false;
            return Ok(0);
        };

        let HistoryPage {
            mut messages,
            has_more_before,
        } = self.store.load_before(first.id, self.page_size)?;

        if messages.is_empty() {
            self.can_load_more = false;
            return Ok(0);
        }

        let loaded_count = messages.len();
        messages.append(&mut self.loaded_messages);
        self.loaded_messages = messages;
        self.can_load_more = has_more_before;
        Ok(loaded_count)
    }

    pub fn append_user_message(&mut self, content: String) -> io::Result<StoredChatMessage> {
        self.append_message(
            StoredMessageRole::User,
            StoredMessageStatus::Normal,
            content,
            None,
        )
    }

    pub fn append_assistant_message(
        &mut self,
        content: String,
        elapsed_ms: u64,
    ) -> io::Result<StoredChatMessage> {
        self.append_message(
            StoredMessageRole::Assistant,
            StoredMessageStatus::Normal,
            content,
            Some(elapsed_ms),
        )
    }

    pub fn append_assistant_error(
        &mut self,
        content: String,
        elapsed_ms: u64,
    ) -> io::Result<StoredChatMessage> {
        self.append_message(
            StoredMessageRole::Assistant,
            StoredMessageStatus::Error,
            content,
            Some(elapsed_ms),
        )
    }

    pub(crate) fn install_performance_fixture(&mut self) {
        if !super::performance::fixture_enabled() {
            return;
        }

        let timestamp = super::timestamp::current_timestamp();
        let assistant_body = "The optimized Chat fixture keeps a bounded readable projection while the complete response remains available in Preview. ".repeat(14);
        self.loaded_messages = (0..45)
            .flat_map(|group| {
                let base = i64::from(group) * 4 + 1;
                [
                    StoredChatMessage {
                        id: base,
                        role: StoredMessageRole::User,
                        status: StoredMessageStatus::Normal,
                        content: format!("Performance fixture prompt {}", group + 1),
                        timestamp: timestamp.clone(),
                        elapsed_ms: None,
                    },
                    StoredChatMessage {
                        id: base + 1,
                        role: StoredMessageRole::Assistant,
                        status: StoredMessageStatus::Normal,
                        content: format!("# Fixture response\n\n{assistant_body}"),
                        timestamp: timestamp.clone(),
                        elapsed_ms: Some(1_250),
                    },
                    StoredChatMessage {
                        id: base + 2,
                        role: StoredMessageRole::Assistant,
                        status: StoredMessageStatus::Normal,
                        content: format!("## Follow-up detail\n\n{assistant_body}"),
                        timestamp: timestamp.clone(),
                        elapsed_ms: Some(2_500),
                    },
                    StoredChatMessage {
                        id: base + 3,
                        role: StoredMessageRole::Assistant,
                        status: StoredMessageStatus::Normal,
                        content: format!("### Final detail\n\n{assistant_body}"),
                        timestamp: timestamp.clone(),
                        elapsed_ms: Some(3_750),
                    },
                ]
            })
            .collect();
        self.can_load_more = false;
        self.next_id = self
            .loaded_messages
            .last()
            .map_or(1, |message| message.id.saturating_add(1));
        log::info!(
            "[Chat::PerformanceFixture] Installed {} in-memory messages",
            self.loaded_messages.len()
        );
    }

    fn append_message(
        &mut self,
        role: StoredMessageRole,
        status: StoredMessageStatus,
        content: String,
        elapsed_ms: Option<u64>,
    ) -> io::Result<StoredChatMessage> {
        let message = StoredChatMessage {
            id: self.next_id,
            role,
            status,
            content,
            timestamp: current_timestamp(),
            elapsed_ms,
        };
        self.next_id = self.next_id.saturating_add(1);

        self.store.append(&message)?;
        self.loaded_messages.push(message.clone());
        Ok(message)
    }
}

#[derive(Debug)]
pub(crate) struct ChatSessions {
    active_mode: ChatMode,
    ai: ChatSessionService,
    rag: ChatSessionService,
}

impl ChatSessions {
    pub(crate) fn bootstrap_default() -> io::Result<Self> {
        Ok(Self {
            active_mode: ChatMode::Ai,
            ai: ChatSessionService::bootstrap(
                JsonlHistoryStore::new_default_named(AI_HISTORY_FILE_NAME)?,
                DEFAULT_STARTUP_LOAD,
                DEFAULT_PAGE_SIZE,
            )?,
            rag: ChatSessionService::bootstrap_default()?,
        })
    }

    pub(crate) fn set_active_mode(&mut self, mode: ChatMode) {
        self.active_mode = mode;
    }

    pub(crate) fn loaded_messages(&self) -> &[StoredChatMessage] {
        self.session(self.active_mode).loaded_messages()
    }

    pub(crate) fn messages_for(&self, mode: ChatMode) -> &[StoredChatMessage] {
        self.session(mode).loaded_messages()
    }

    pub(crate) fn can_load_more(&self) -> bool {
        self.session(self.active_mode).can_load_more()
    }

    pub(crate) fn can_load_more_for(&self, mode: ChatMode) -> bool {
        self.session(mode).can_load_more()
    }

    pub(crate) fn load_more_history_for(&mut self, mode: ChatMode) -> io::Result<usize> {
        self.session_mut(mode).load_more_history()
    }

    pub(crate) fn append_user_message_for(
        &mut self,
        mode: ChatMode,
        content: String,
    ) -> io::Result<StoredChatMessage> {
        self.session_mut(mode).append_user_message(content)
    }

    pub(crate) fn append_assistant_message_for(
        &mut self,
        mode: ChatMode,
        content: String,
        elapsed_ms: u64,
    ) -> io::Result<StoredChatMessage> {
        self.session_mut(mode)
            .append_assistant_message(content, elapsed_ms)
    }

    pub(crate) fn append_assistant_error_for(
        &mut self,
        mode: ChatMode,
        content: String,
        elapsed_ms: u64,
    ) -> io::Result<StoredChatMessage> {
        self.session_mut(mode)
            .append_assistant_error(content, elapsed_ms)
    }

    pub(crate) fn install_performance_fixture(&mut self) {
        self.ai.install_performance_fixture();
    }

    pub(crate) fn log_storage_paths(&self) {
        log::info!(
            "[Chat::Storage] AI history file: {}",
            self.ai.store.file_path().display()
        );
        log::info!(
            "[Chat::Storage] RAG history file: {}",
            self.rag.store.file_path().display()
        );
    }

    fn session(&self, mode: ChatMode) -> &ChatSessionService {
        match mode {
            ChatMode::Ai => &self.ai,
            ChatMode::Rag => &self.rag,
        }
    }

    fn session_mut(&mut self, mode: ChatMode) -> &mut ChatSessionService {
        match mode {
            ChatMode::Ai => &mut self.ai,
            ChatMode::Rag => &mut self.rag,
        }
    }
}

fn current_timestamp() -> String {
    super::timestamp::current_timestamp()
}

#[cfg(test)]
mod tests {
    use super::{ChatMode, ChatRequestState, ChatSessionService, ChatSessions};
    use crate::chat::persistence::JsonlHistoryStore;
    use std::fs;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn sending_state_is_isolated_by_mode() {
        let mut state = ChatRequestState::default();
        state.start(ChatMode::Ai, Instant::now());

        assert!(state.is_sending(ChatMode::Ai));
        assert!(!state.is_sending(ChatMode::Rag));

        state.start(ChatMode::Rag, Instant::now());
        state.finish(ChatMode::Ai);
        assert!(!state.is_sending(ChatMode::Ai));
        assert!(state.is_sending(ChatMode::Rag));
    }

    #[test]
    fn ai_and_rag_histories_are_persisted_independently() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "usher-chat-mode-test-{}-{nonce}",
            std::process::id()
        ));
        let ai_path = dir.join("ai.jsonl");
        let rag_path = dir.join("rag.jsonl");
        let ai_store = JsonlHistoryStore::open_test_path(ai_path.clone()).unwrap();
        let rag_store = JsonlHistoryStore::open_test_path(rag_path.clone()).unwrap();
        let mut sessions = ChatSessions {
            active_mode: ChatMode::Ai,
            ai: ChatSessionService::bootstrap(ai_store, 20, 10).unwrap(),
            rag: ChatSessionService::bootstrap(rag_store, 20, 10).unwrap(),
        };

        sessions
            .append_user_message_for(ChatMode::Ai, "AI question".into())
            .unwrap();
        sessions
            .append_assistant_message_for(ChatMode::Ai, "AI answer".into(), 1_250)
            .unwrap();
        sessions.set_active_mode(ChatMode::Rag);
        sessions
            .append_user_message_for(ChatMode::Rag, "RAG question".into())
            .unwrap();

        assert_eq!(sessions.messages_for(ChatMode::Ai).len(), 2);
        assert_eq!(sessions.messages_for(ChatMode::Rag).len(), 1);
        assert!(
            !fs::read_to_string(ai_path)
                .unwrap()
                .contains("RAG question")
        );
        assert!(
            !fs::read_to_string(rag_path)
                .unwrap()
                .contains("AI question")
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
