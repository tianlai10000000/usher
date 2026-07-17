use super::persistence::{
    HistoryPage, JsonlHistoryStore, StoredChatMessage, StoredMessageRole, StoredMessageStatus,
};
use chrono::Local;
use std::io;

const DEFAULT_STARTUP_LOAD: usize = 20;
const DEFAULT_PAGE_SIZE: usize = 10;

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
        )
    }

    pub fn append_assistant_message(&mut self, content: String) -> io::Result<StoredChatMessage> {
        self.append_message(
            StoredMessageRole::Assistant,
            StoredMessageStatus::Normal,
            content,
        )
    }

    pub fn append_assistant_error(&mut self, content: String) -> io::Result<StoredChatMessage> {
        self.append_message(
            StoredMessageRole::Assistant,
            StoredMessageStatus::Error,
            content,
        )
    }

    pub fn log_storage_path(&self) {
        log::info!(
            "[Chat::Storage] Message history file: {}",
            self.store.file_path().display()
        );
    }

    fn append_message(
        &mut self,
        role: StoredMessageRole,
        status: StoredMessageStatus,
        content: String,
    ) -> io::Result<StoredChatMessage> {
        let message = StoredChatMessage {
            id: self.next_id,
            role,
            status,
            content,
            timestamp: current_timestamp(),
        };
        self.next_id = self.next_id.saturating_add(1);

        self.store.append(&message)?;
        self.loaded_messages.push(message.clone());
        Ok(message)
    }
}

fn current_timestamp() -> String {
    Local::now().format("%H:%M").to_string()
}
