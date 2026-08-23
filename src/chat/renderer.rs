use super::markdown::{AssistantProjection, project_assistant_content};
use super::persistence::{StoredChatMessage, StoredMessageRole, StoredMessageStatus};
use crate::{ChatMessage, MessageRole, MessageStatus};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_RENDER_CACHE_CAPACITY: usize = 96;
const DEFAULT_RENDER_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;
const LOADING_PLACEHOLDER_ID: i32 = -1;

struct CachedRender {
    rendered_content: Arc<str>,
    preview_available: bool,
    preview_title: &'static str,
    approx_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    message_id: i64,
}

struct LruRenderCache {
    capacity: usize,
    max_bytes: usize,
    current_bytes: usize,
    order: VecDeque<CacheKey>,
    values: HashMap<CacheKey, Arc<CachedRender>>,
}

impl LruRenderCache {
    fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            capacity,
            max_bytes,
            current_bytes: 0,
            order: VecDeque::new(),
            values: HashMap::new(),
        }
    }

    fn get(&mut self, key: CacheKey) -> Option<Arc<CachedRender>> {
        let value = self.values.get(&key).cloned()?;
        self.touch(key);
        Some(value)
    }

    fn insert(&mut self, key: CacheKey, value: Arc<CachedRender>) {
        if let Some(old) = self.values.insert(key, value.clone()) {
            self.current_bytes = self.current_bytes.saturating_sub(old.approx_bytes);
        }
        self.current_bytes = self.current_bytes.saturating_add(value.approx_bytes);
        self.touch(key);
        while self.values.len() > self.capacity || self.current_bytes > self.max_bytes {
            if let Some(evict) = self.order.pop_front()
                && let Some(old) = self.values.remove(&evict)
            {
                self.current_bytes = self.current_bytes.saturating_sub(old.approx_bytes);
            }
        }
    }

    fn touch(&mut self, key: CacheKey) {
        self.order.retain(|candidate| *candidate != key);
        self.order.push_back(key);
    }
}

pub struct ChatMessageRenderer {
    cache: LruRenderCache,
}

impl ChatMessageRenderer {
    fn new() -> Self {
        Self {
            cache: LruRenderCache::new(
                DEFAULT_RENDER_CACHE_CAPACITY,
                DEFAULT_RENDER_CACHE_MAX_BYTES,
            ),
        }
    }

    fn render_messages(
        &mut self,
        source: &[StoredChatMessage],
        include_loading_placeholder: bool,
    ) -> Vec<ChatMessage> {
        let mut result =
            Vec::with_capacity(source.len() + usize::from(include_loading_placeholder));
        result.extend(source.iter().map(|message| self.render_single(message)));
        if include_loading_placeholder {
            result.push(Self::loading_placeholder());
        }
        result
    }

    fn assistant_projection(&mut self, source: &StoredChatMessage) -> Arc<CachedRender> {
        let key = CacheKey {
            message_id: source.id,
        };
        if let Some(hit) = self.cache.get(key) {
            return hit;
        }

        let (_, display_content) = assistant_display(source);
        let AssistantProjection {
            text,
            preview_available,
            preview_title,
            markdown,
        } = project_assistant_content(display_content);
        if markdown {
            MARKDOWN_PROJECTION_COUNT.with(|count| count.set(count.get().wrapping_add(1)));
        }
        let rendered_content = Arc::<str>::from(text);
        let computed = Arc::new(CachedRender {
            approx_bytes: rendered_content
                .len()
                .saturating_add(preview_title.len())
                .saturating_add(std::mem::size_of::<CachedRender>()),
            rendered_content,
            preview_available,
            preview_title,
        });
        self.cache.insert(key, computed.clone());
        computed
    }

    fn render_single(&mut self, source: &StoredChatMessage) -> ChatMessage {
        let id = i64_to_i32(source.id);
        let timestamp = super::timestamp::format_for_display(&source.timestamp);

        match source.role {
            StoredMessageRole::User => ChatMessage {
                id,
                role: MessageRole::User,
                status: MessageStatus::Normal,
                content: source.content.clone().into(),
                rendered_content: "".into(),
                preview_available: false,
                preview_title: "".into(),
                timestamp: timestamp.into(),
                elapsed: "".into(),
            },
            StoredMessageRole::Assistant => {
                let (stored_status, display_content) = assistant_display(source);
                let rendered = self.assistant_projection(source);
                ChatMessage {
                    id,
                    role: MessageRole::Assistant,
                    status: message_status_for_ui(stored_status),
                    content: display_content.into(),
                    rendered_content: rendered.rendered_content.as_ref().into(),
                    preview_available: rendered.preview_available,
                    preview_title: rendered.preview_title.into(),
                    timestamp: timestamp.into(),
                    elapsed: source
                        .elapsed_ms
                        .map(format_elapsed)
                        .unwrap_or_default()
                        .into(),
                }
            }
        }
    }

    fn loading_placeholder() -> ChatMessage {
        ChatMessage {
            id: LOADING_PLACEHOLDER_ID,
            role: MessageRole::Assistant,
            status: MessageStatus::Normal,
            content: "".into(),
            rendered_content: "".into(),
            preview_available: false,
            preview_title: "".into(),
            timestamp: "".into(),
            elapsed: "".into(),
        }
    }
}

fn format_elapsed(elapsed_ms: u64) -> String {
    if elapsed_ms < 1_000 {
        return format!("{}ms", elapsed_ms.max(1));
    }
    let seconds = elapsed_ms / 1_000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    let remaining_seconds = seconds % 60;
    if minutes < 60 {
        return format!("{minutes}m {remaining_seconds:02}s");
    }
    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    format!("{hours}h {remaining_minutes:02}m {remaining_seconds:02}s")
}

fn assistant_display(message: &StoredChatMessage) -> (StoredMessageStatus, &str) {
    if message.status != StoredMessageStatus::Normal {
        return (message.status, message.content.as_str());
    }
    if let Some(content) = strip_legacy_status_prefix(&message.content, '❌') {
        return (StoredMessageStatus::Error, content);
    }
    if let Some(content) = strip_legacy_status_prefix(&message.content, '✅') {
        return (StoredMessageStatus::Success, content);
    }
    (StoredMessageStatus::Normal, message.content.as_str())
}

fn strip_legacy_status_prefix(content: &str, marker: char) -> Option<&str> {
    content
        .strip_prefix(marker)
        .map(|remaining| remaining.strip_prefix(' ').unwrap_or(remaining))
}

fn message_status_for_ui(status: StoredMessageStatus) -> MessageStatus {
    match status {
        StoredMessageStatus::Normal => MessageStatus::Normal,
        StoredMessageStatus::Error => MessageStatus::Error,
        StoredMessageStatus::Success => MessageStatus::Success,
    }
}

#[derive(Debug, Clone)]
pub(super) struct ModelMutationDiagnostic {
    pub(super) revision: u64,
    pub(super) duration: Duration,
    pub(super) completed_at: Instant,
    pub(super) inserted_rows: usize,
    pub(super) removed_rows: usize,
    pub(super) incremental: bool,
}

thread_local! {
    static RENDERER: RefCell<ChatMessageRenderer> = RefCell::new(ChatMessageRenderer::new());
    static MODEL_COMMIT_COUNT: Cell<u64> = const { Cell::new(0) };
    static MARKDOWN_PROJECTION_COUNT: Cell<usize> = const { Cell::new(0) };
    static LATEST_MODEL_MUTATION: RefCell<Option<ModelMutationDiagnostic>> = const { RefCell::new(None) };
}

pub fn render_for_ui(
    source: &[StoredChatMessage],
    include_loading_placeholder: bool,
) -> Vec<ChatMessage> {
    RENDERER.with(|renderer| {
        renderer
            .borrow_mut()
            .render_messages(source, include_loading_placeholder)
    })
}

pub(super) fn projection_for_layout(source: &StoredChatMessage) -> (Arc<str>, bool) {
    match source.role {
        StoredMessageRole::User => (Arc::<str>::from(source.content.as_str()), false),
        StoredMessageRole::Assistant => RENDERER.with(|renderer| {
            let rendered = renderer.borrow_mut().assistant_projection(source);
            (
                rendered.rendered_content.clone(),
                rendered.preview_available,
            )
        }),
    }
}

pub fn clear_render_cache() {
    RENDERER.with(|renderer| *renderer.borrow_mut() = ChatMessageRenderer::new());
}

pub fn record_model_commit(
    duration: Duration,
    inserted_rows: usize,
    removed_rows: usize,
    incremental: bool,
) {
    let revision = MODEL_COMMIT_COUNT.with(|count| {
        let revision = count.get().wrapping_add(1);
        count.set(revision);
        revision
    });
    LATEST_MODEL_MUTATION.with(|latest| {
        *latest.borrow_mut() = Some(ModelMutationDiagnostic {
            revision,
            duration,
            completed_at: Instant::now(),
            inserted_rows,
            removed_rows,
            incremental,
        });
    });
}

pub(crate) fn diagnostic_counts() -> (u64, usize) {
    (
        MODEL_COMMIT_COUNT.with(Cell::get),
        MARKDOWN_PROJECTION_COUNT.with(Cell::get),
    )
}

pub(super) fn latest_model_mutation() -> Option<ModelMutationDiagnostic> {
    LATEST_MODEL_MUTATION.with(|latest| latest.borrow().clone())
}

fn i64_to_i32(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::markdown::MAX_INLINE_PROJECTION_CHARS;

    fn message(content: impl Into<String>) -> StoredChatMessage {
        StoredChatMessage {
            id: 1,
            role: StoredMessageRole::Assistant,
            status: StoredMessageStatus::Normal,
            content: content.into(),
            timestamp: "12:00".to_string(),
            elapsed_ms: None,
        }
    }

    #[test]
    fn legacy_status_emoji_is_removed_from_display_text() {
        let message = message("❌ connection failed");
        let (status, content) = assistant_display(&message);
        assert_eq!(status, StoredMessageStatus::Error);
        assert_eq!(content, "connection failed");
    }

    #[test]
    fn assistant_projection_is_bounded_and_retains_full_preview_source() {
        let source = message("answer ".repeat(300));
        let row = ChatMessageRenderer::new().render_single(&source);
        assert!(row.rendered_content.chars().count() <= MAX_INLINE_PROJECTION_CHARS);
        assert!(row.rendered_content.ends_with("..."));
        assert!(row.preview_available);
        assert_eq!(row.content.as_str(), source.content);
    }

    #[test]
    fn markdown_message_exposes_formatted_preview_without_inline_blocks() {
        let source = message("# Result\n\n```rust\nfn main() {}\n```");
        let row = ChatMessageRenderer::new().render_single(&source);
        assert!(row.preview_available);
        assert_eq!(row.preview_title.as_str(), "Open formatted response");
        assert!(row.rendered_content.contains("fn main() {}"));
    }

    #[test]
    fn elapsed_time_uses_compact_metadata_labels() {
        assert_eq!(format_elapsed(842), "842ms");
        assert_eq!(format_elapsed(42_900), "42s");
        assert_eq!(format_elapsed(125_000), "2m 05s");
        assert_eq!(format_elapsed(3_725_000), "1h 02m 05s");
    }
}
