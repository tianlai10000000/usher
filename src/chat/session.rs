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

#[derive(Debug)]
pub(crate) struct PendingChatMessage {
    store: JsonlHistoryStore,
    message: StoredChatMessage,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingHistoryLoad {
    store: JsonlHistoryStore,
    before_id: i64,
    page_size: usize,
}

impl PendingHistoryLoad {
    pub(crate) fn load(&self) -> io::Result<HistoryPage> {
        self.store.load_before(self.before_id, self.page_size)
    }
}

impl PendingChatMessage {
    pub(crate) fn message(&self) -> &StoredChatMessage {
        &self.message
    }

    pub(crate) fn persist(&self) -> io::Result<()> {
        self.store.append(&self.message)
    }
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

    fn prepare_history_load(&mut self) -> Option<PendingHistoryLoad> {
        let Some(first) = self.loaded_messages.first() else {
            self.can_load_more = false;
            return None;
        };

        Some(PendingHistoryLoad {
            store: self.store.clone(),
            before_id: first.id,
            page_size: self.page_size,
        })
    }

    fn commit_history_load(
        &mut self,
        request: &PendingHistoryLoad,
        page: HistoryPage,
    ) -> io::Result<usize> {
        let HistoryPage {
            mut messages,
            has_more_before,
        } = page;
        if self.loaded_messages.first().map(|message| message.id) != Some(request.before_id) {
            return Err(io::Error::other(
                "chat history changed before the loaded page could be committed",
            ));
        }

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

    #[cfg(test)]
    pub fn append_user_message(&mut self, content: String) -> io::Result<StoredChatMessage> {
        self.append_message(
            StoredMessageRole::User,
            StoredMessageStatus::Normal,
            content,
            None,
        )
    }

    #[cfg(test)]
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

    #[cfg(test)]
    fn append_message(
        &mut self,
        role: StoredMessageRole,
        status: StoredMessageStatus,
        content: String,
        elapsed_ms: Option<u64>,
    ) -> io::Result<StoredChatMessage> {
        let pending = self.prepare_message(role, status, content, elapsed_ms);
        pending.persist()?;
        self.commit_message(pending)
    }

    fn prepare_message(
        &self,
        role: StoredMessageRole,
        status: StoredMessageStatus,
        content: String,
        elapsed_ms: Option<u64>,
    ) -> PendingChatMessage {
        PendingChatMessage {
            store: self.store.clone(),
            message: StoredChatMessage {
                id: self.next_id,
                role,
                status,
                content,
                timestamp: current_timestamp(),
                elapsed_ms,
            },
        }
    }

    fn commit_message(&mut self, pending: PendingChatMessage) -> io::Result<StoredChatMessage> {
        if pending.message.id != self.next_id {
            return Err(io::Error::other(
                "chat append sequence changed before commit",
            ));
        }
        self.next_id = self.next_id.saturating_add(1);
        self.loaded_messages.push(pending.message.clone());
        Ok(pending.message)
    }
}

#[derive(Debug)]
pub(crate) struct ChatSessions {
    active_mode: ChatMode,
    ai: ChatSessionService,
    rag: ChatSessionService,
}

impl ChatSessions {
    #[cfg(test)]
    pub(crate) fn from_test_stores(ai: JsonlHistoryStore, rag: JsonlHistoryStore) -> Self {
        Self {
            active_mode: ChatMode::Ai,
            ai: ChatSessionService::bootstrap(ai, DEFAULT_STARTUP_LOAD, DEFAULT_PAGE_SIZE).unwrap(),
            rag: ChatSessionService::bootstrap(rag, DEFAULT_STARTUP_LOAD, DEFAULT_PAGE_SIZE)
                .unwrap(),
        }
    }

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

    pub(crate) fn prepare_history_load_for(
        &mut self,
        mode: ChatMode,
    ) -> Option<PendingHistoryLoad> {
        self.session_mut(mode).prepare_history_load()
    }

    pub(crate) fn commit_history_load_for(
        &mut self,
        mode: ChatMode,
        request: &PendingHistoryLoad,
        page: HistoryPage,
    ) -> io::Result<usize> {
        self.session_mut(mode).commit_history_load(request, page)
    }

    #[cfg(test)]
    pub(crate) fn append_user_message_for(
        &mut self,
        mode: ChatMode,
        content: String,
    ) -> io::Result<StoredChatMessage> {
        self.session_mut(mode).append_user_message(content)
    }

    pub(crate) fn prepare_user_message_for(
        &self,
        mode: ChatMode,
        content: String,
    ) -> PendingChatMessage {
        self.session(mode).prepare_message(
            StoredMessageRole::User,
            StoredMessageStatus::Normal,
            content,
            None,
        )
    }

    pub(crate) fn prepare_assistant_message_for(
        &self,
        mode: ChatMode,
        content: String,
        status: StoredMessageStatus,
        elapsed_ms: u64,
    ) -> PendingChatMessage {
        self.session(mode).prepare_message(
            StoredMessageRole::Assistant,
            status,
            content,
            Some(elapsed_ms),
        )
    }

    pub(crate) fn commit_message_for(
        &mut self,
        mode: ChatMode,
        pending: PendingChatMessage,
    ) -> io::Result<StoredChatMessage> {
        self.session_mut(mode).commit_message(pending)
    }

    #[cfg(test)]
    pub(crate) fn append_assistant_message_for(
        &mut self,
        mode: ChatMode,
        content: String,
        elapsed_ms: u64,
    ) -> io::Result<StoredChatMessage> {
        self.session_mut(mode)
            .append_assistant_message(content, elapsed_ms)
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
    use crate::chat::persistence::{
        JsonlHistoryStore, StoredChatMessage, StoredMessageRole, StoredMessageStatus,
    };
    use std::fs;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn stored_message(id: i64) -> StoredChatMessage {
        StoredChatMessage {
            id,
            role: StoredMessageRole::User,
            status: StoredMessageStatus::Normal,
            content: format!("message {id}"),
            timestamp: "2026-08-30T12:00:00+08:00".into(),
            elapsed_ms: None,
        }
    }

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
    fn history_pages_are_loaded_before_taking_the_session_commit_lock() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "usher-chat-history-request-test-{}-{nonce}",
            std::process::id()
        ));
        let store = JsonlHistoryStore::open_test_path(dir.join("history.jsonl")).unwrap();
        for id in 1..=10 {
            store.append(&stored_message(id)).unwrap();
        }
        let mut session = ChatSessionService::bootstrap(store, 4, 3).unwrap();

        let first_request = session.prepare_history_load().expect("first request");
        let first_page = first_request
            .load()
            .expect("read first page without session");
        assert_eq!(
            session
                .commit_history_load(&first_request, first_page)
                .unwrap(),
            3
        );
        assert_eq!(session.loaded_messages().first().unwrap().id, 4);

        let stale_request = session.prepare_history_load().expect("stale request");
        let current_request = session.prepare_history_load().expect("current request");
        let current_page = current_request.load().unwrap();
        session
            .commit_history_load(&current_request, current_page)
            .unwrap();
        let stale_page = stale_request.load().unwrap();
        assert!(
            session
                .commit_history_load(&stale_request, stale_page)
                .is_err()
        );
        fs::remove_dir_all(dir).unwrap();
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

    #[test]
    fn failed_persistence_does_not_commit_or_advance_a_message() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "usher-chat-persist-failure-test-{}-{nonce}",
            std::process::id()
        ));
        let path = dir.join("history.jsonl");
        let store = JsonlHistoryStore::open_test_path(path).unwrap();
        let mut session = ChatSessionService::bootstrap(store, 20, 10).unwrap();
        fs::remove_dir_all(&dir).unwrap();

        let pending = session.prepare_message(
            crate::chat::persistence::StoredMessageRole::User,
            crate::chat::persistence::StoredMessageStatus::Normal,
            "keep this draft".into(),
            None,
        );
        assert!(pending.persist().is_err());
        assert!(session.loaded_messages().is_empty());

        fs::create_dir_all(&dir).unwrap();
        let replacement = session.prepare_message(
            crate::chat::persistence::StoredMessageRole::User,
            crate::chat::persistence::StoredMessageStatus::Normal,
            "retry".into(),
            None,
        );
        assert_eq!(replacement.message().id, 1);
        replacement.persist().unwrap();
        session.commit_message(replacement).unwrap();
        assert_eq!(session.loaded_messages().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
