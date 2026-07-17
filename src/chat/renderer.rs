use super::markdown::{
    build_assistant_render_payload, build_assistant_render_payload_with_wrap_width,
};
use super::persistence::{StoredChatMessage, StoredMessageRole, StoredMessageStatus};
use crate::{ChatMessage, MessageRole, MessageStatus};
use slint::{Model, ModelRc};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

const DEFAULT_RENDER_CACHE_CAPACITY: usize = 96;
const DEFAULT_RENDER_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;
const LOADING_PLACEHOLDER_ID: i32 = -1;

struct CachedRender {
    rendered_content: Arc<str>,
    is_markdown: bool,
    markdown_blocks: Arc<[crate::MarkdownBlock]>,
    approx_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    message_id: i64,
    wrap_width_px: i32,
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
        if let Some(value) = self.values.get(&key).cloned() {
            self.touch(key);
            Some(value)
        } else {
            None
        }
    }

    fn insert(&mut self, key: CacheKey, value: Arc<CachedRender>) {
        if let Some(old) = self.values.insert(key, value.clone()) {
            self.current_bytes = self.current_bytes.saturating_sub(old.approx_bytes);
        }
        self.current_bytes = self.current_bytes.saturating_add(value.approx_bytes);
        self.touch(key);
        while self.values.len() > self.capacity || self.current_bytes > self.max_bytes {
            if let Some(evict) = self.order.pop_front() {
                if let Some(old) = self.values.remove(&evict) {
                    self.current_bytes = self.current_bytes.saturating_sub(old.approx_bytes);
                }
            }
        }
    }

    fn touch(&mut self, key: CacheKey) {
        self.order.retain(|k| *k != key);
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
        wrap_width_override_px: Option<i32>,
        include_loading_placeholder: bool,
    ) -> Vec<ChatMessage> {
        let mut result =
            Vec::with_capacity(source.len() + usize::from(include_loading_placeholder));

        for message in source {
            result.push(self.render_single(message, wrap_width_override_px));
        }

        if include_loading_placeholder {
            result.push(Self::loading_placeholder());
        }

        result
    }

    fn render_single(
        &mut self,
        source: &StoredChatMessage,
        wrap_width_override_px: Option<i32>,
    ) -> ChatMessage {
        let id = i64_to_i32(source.id);
        let timestamp = source.timestamp.clone();

        match source.role {
            StoredMessageRole::User => ChatMessage {
                id,
                role: MessageRole::User,
                status: MessageStatus::Normal,
                content: source.content.clone().into(),
                rendered_content: "".into(),
                is_markdown: false,
                markdown_blocks: ModelRc::default(),
                timestamp: timestamp.into(),
            },
            StoredMessageRole::Assistant => {
                let (stored_status, display_content) = assistant_display(source);
                let status = message_status_for_ui(stored_status);

                // 非 markdown 内容渲染成本低，且缓存会额外持有整段文本副本，直接跳过缓存以降低内存。
                if !super::markdown::looks_like_markdown(display_content) {
                    return ChatMessage {
                        id,
                        role: MessageRole::Assistant,
                        status,
                        content: display_content.into(),
                        rendered_content: display_content.into(),
                        is_markdown: false,
                        markdown_blocks: ModelRc::default(),
                        timestamp: timestamp.into(),
                    };
                }

                let wrap_key = wrap_width_override_px.unwrap_or(-1);
                let cache_key = CacheKey {
                    message_id: source.id,
                    wrap_width_px: wrap_key,
                };

                let rendered = if let Some(hit) = self.cache.get(cache_key) {
                    hit
                } else {
                    let payload = if let Some(width) = wrap_width_override_px {
                        build_assistant_render_payload_with_wrap_width(
                            display_content,
                            Some(width as f32),
                        )
                    } else {
                        build_assistant_render_payload(display_content)
                    };
                    let rendered_text = if payload.is_markdown {
                        // markdown 块渲染时不会用到 rendered_content，避免重复占用大字符串。
                        Arc::<str>::from("")
                    } else {
                        Arc::<str>::from(payload.rendered_text)
                    };
                    let blocks: Arc<[crate::MarkdownBlock]> = payload.blocks.into();
                    let approx_bytes =
                        rendered_text.len() + estimate_markdown_blocks_bytes(&blocks);
                    let computed = Arc::new(CachedRender {
                        rendered_content: rendered_text,
                        is_markdown: payload.is_markdown,
                        markdown_blocks: blocks,
                        approx_bytes,
                    });
                    self.cache.insert(cache_key, computed.clone());
                    computed
                };

                ChatMessage {
                    id,
                    role: MessageRole::Assistant,
                    status,
                    content: display_content.into(),
                    rendered_content: rendered.rendered_content.as_ref().into(),
                    is_markdown: rendered.is_markdown,
                    markdown_blocks: rendered.markdown_blocks.as_ref().into(),
                    timestamp: timestamp.into(),
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
            is_markdown: false,
            markdown_blocks: ModelRc::default(),
            timestamp: "".into(),
        }
    }
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

fn estimate_markdown_blocks_bytes(blocks: &[crate::MarkdownBlock]) -> usize {
    let mut sum = 0usize;
    for block in blocks {
        sum = sum.saturating_add(block.text.len());
        for line in block.inline_lines.iter() {
            for span in line.spans.iter() {
                sum = sum
                    .saturating_add(span.text.len())
                    .saturating_add(span.href.len())
                    .saturating_add(16);
            }
        }
        sum = sum.saturating_add(32);
    }
    sum
}

thread_local! {
    static RENDERER: RefCell<ChatMessageRenderer> = RefCell::new(ChatMessageRenderer::new());
}

pub fn render_for_ui(
    source: &[StoredChatMessage],
    wrap_width_override_px: Option<i32>,
    include_loading_placeholder: bool,
) -> Vec<ChatMessage> {
    RENDERER.with(|renderer| {
        renderer.borrow_mut().render_messages(
            source,
            wrap_width_override_px,
            include_loading_placeholder,
        )
    })
}

pub fn clear_render_cache() {
    RENDERER.with(|renderer| {
        *renderer.borrow_mut() = ChatMessageRenderer::new();
    });
}

fn i64_to_i32(value: i64) -> i32 {
    if value > i32::MAX as i64 {
        i32::MAX
    } else if value < i32::MIN as i64 {
        i32::MIN
    } else {
        value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::assistant_display;
    use crate::chat::persistence::{StoredChatMessage, StoredMessageRole, StoredMessageStatus};

    #[test]
    fn legacy_status_emoji_is_removed_from_display_text() {
        let message = StoredChatMessage {
            id: 1,
            role: StoredMessageRole::Assistant,
            status: StoredMessageStatus::Normal,
            content: "❌ connection failed".to_string(),
            timestamp: "12:00".to_string(),
        };

        let (status, content) = assistant_display(&message);
        assert_eq!(status, StoredMessageStatus::Error);
        assert_eq!(content, "connection failed");
    }
}
