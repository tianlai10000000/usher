//! Width-independent, bounded projections for agent-authored Markdown.

use std::collections::{HashMap, VecDeque};
use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

use usher_preview::{
    CancellationToken, MarkdownBlock, MarkdownBlockKind, MarkdownInlineKind, PreviewError,
    parse_markdown_structure_with_cancellation,
};

pub const REPOSITORY_FILE_LINK_PREFIX: &str = "usher-repo-file:";
pub const DEFAULT_PROJECTION_MAX_UTF8_BYTES: usize = 16 * 1024;
pub const DEFAULT_PROJECTION_MAX_BLOCKS: usize = 48;
pub const DEFAULT_CACHE_MAX_ENTRIES: usize = 128;
pub const DEFAULT_CACHE_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionLimits {
    pub max_utf8_bytes: usize,
    pub max_blocks: usize,
}

impl Default for ProjectionLimits {
    fn default() -> Self {
        Self {
            max_utf8_bytes: DEFAULT_PROJECTION_MAX_UTF8_BYTES,
            max_blocks: DEFAULT_PROJECTION_MAX_BLOCKS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionCacheLimits {
    pub max_entries: usize,
    pub max_bytes: usize,
}

impl Default for ProjectionCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_CACHE_MAX_ENTRIES,
            max_bytes: DEFAULT_CACHE_MAX_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentIdentity {
    first: u64,
    second: u64,
    utf8_bytes: usize,
}

impl ContentIdentity {
    pub const fn utf8_bytes(self) -> usize {
        self.utf8_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryFileAction {
    pub token: String,
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResponseProjection {
    pub title: String,
    pub plain_text: String,
    pub blocks: Vec<MarkdownBlock>,
    pub repository_file_actions: Vec<RepositoryFileAction>,
    pub truncated: bool,
    pub approximate_bytes: usize,
}

impl AgentResponseProjection {
    /// Whether this response benefits from a full formatted Preview action.
    pub fn has_rich_markdown(&self) -> bool {
        self.truncated
            || !self.repository_file_actions.is_empty()
            || self.blocks.iter().any(|block| {
                block.kind != MarkdownBlockKind::Paragraph
                    || block
                        .spans
                        .iter()
                        .any(|span| span.kind != MarkdownInlineKind::Text)
            })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProjectionCacheKey {
    message_id: String,
    content: ContentIdentity,
    fallback_title: ContentIdentity,
}

struct CachedProjection {
    projection: Arc<AgentResponseProjection>,
    approximate_bytes: usize,
}

pub struct AgentResponseProjector {
    projection_limits: ProjectionLimits,
    cache_limits: ProjectionCacheLimits,
    cache: HashMap<ProjectionCacheKey, CachedProjection>,
    lru: VecDeque<ProjectionCacheKey>,
    cache_bytes: usize,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl Default for AgentResponseProjector {
    fn default() -> Self {
        Self::new(
            ProjectionLimits::default(),
            ProjectionCacheLimits::default(),
        )
    }
}

impl AgentResponseProjector {
    pub fn new(projection_limits: ProjectionLimits, cache_limits: ProjectionCacheLimits) -> Self {
        Self {
            projection_limits,
            cache_limits,
            cache: HashMap::new(),
            lru: VecDeque::new(),
            cache_bytes: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn project(
        &mut self,
        message_id: &str,
        content: &str,
        fallback_title: &str,
    ) -> Arc<AgentResponseProjection> {
        self.project_with_cancellation(
            message_id,
            content,
            fallback_title,
            &CancellationToken::default(),
        )
        .expect("non-cancellable agent response projection cannot be cancelled")
    }

    pub fn project_with_cancellation(
        &mut self,
        message_id: &str,
        content: &str,
        fallback_title: &str,
        cancellation: &CancellationToken,
    ) -> Result<Arc<AgentResponseProjection>, PreviewError> {
        if cancellation.is_cancelled() {
            return Err(PreviewError::Cancelled);
        }
        let content_id = content_identity(content);
        let key = ProjectionCacheKey {
            message_id: message_id.to_owned(),
            content: content_id,
            fallback_title: content_identity(fallback_title),
        };
        if let Some(projection) = self.cache_get(&key) {
            self.hits = self.hits.saturating_add(1);
            return Ok(projection);
        }

        self.misses = self.misses.saturating_add(1);
        let projection = Arc::new(project_agent_response_with_cancellation(
            message_id,
            content,
            fallback_title,
            self.projection_limits,
            cancellation,
        )?);
        self.cache_insert(key, projection.clone());
        Ok(projection)
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.lru.clear();
        self.cache_bytes = 0;
    }

    pub fn stats(&self) -> ProjectionCacheStats {
        ProjectionCacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            entries: self.cache.len(),
            bytes: self.cache_bytes,
        }
    }

    fn cache_get(&mut self, key: &ProjectionCacheKey) -> Option<Arc<AgentResponseProjection>> {
        let projection = self.cache.get(key)?.projection.clone();
        self.touch(key);
        Some(projection)
    }

    fn cache_insert(&mut self, key: ProjectionCacheKey, projection: Arc<AgentResponseProjection>) {
        if self.cache_limits.max_entries == 0 || self.cache_limits.max_bytes == 0 {
            return;
        }
        let approximate_bytes = projection
            .approximate_bytes
            .saturating_add(key.message_id.len())
            .saturating_add(mem::size_of::<ProjectionCacheKey>())
            .saturating_add(mem::size_of::<CachedProjection>());
        if approximate_bytes > self.cache_limits.max_bytes {
            return;
        }

        if let Some(previous) = self.cache.remove(&key) {
            self.cache_bytes = self.cache_bytes.saturating_sub(previous.approximate_bytes);
            self.lru.retain(|candidate| candidate != &key);
        }
        self.cache_bytes = self.cache_bytes.saturating_add(approximate_bytes);
        self.cache.insert(
            key.clone(),
            CachedProjection {
                projection,
                approximate_bytes,
            },
        );
        self.lru.push_back(key);

        while self.cache.len() > self.cache_limits.max_entries
            || self.cache_bytes > self.cache_limits.max_bytes
        {
            let Some(evicted_key) = self.lru.pop_front() else {
                break;
            };
            if let Some(evicted) = self.cache.remove(&evicted_key) {
                self.cache_bytes = self.cache_bytes.saturating_sub(evicted.approximate_bytes);
                self.evictions = self.evictions.saturating_add(1);
            }
        }
    }

    fn touch(&mut self, key: &ProjectionCacheKey) {
        self.lru.retain(|candidate| candidate != key);
        self.lru.push_back(key.clone());
    }
}

#[cfg(test)]
pub fn project_agent_response(
    message_id: &str,
    content: &str,
    fallback_title: &str,
    limits: ProjectionLimits,
) -> AgentResponseProjection {
    project_agent_response_with_cancellation(
        message_id,
        content,
        fallback_title,
        limits,
        &CancellationToken::default(),
    )
    .expect("non-cancellable agent response projection cannot be cancelled")
}

pub fn project_agent_response_with_cancellation(
    message_id: &str,
    content: &str,
    fallback_title: &str,
    limits: ProjectionLimits,
    cancellation: &CancellationToken,
) -> Result<AgentResponseProjection, PreviewError> {
    let content_identity = content_identity(content);
    let bounded_source = take_utf8_prefix(content, limits.max_utf8_bytes);
    let source_truncated = bounded_source.len() < content.len();
    let document =
        parse_markdown_structure_with_cancellation(bounded_source, fallback_title, cancellation)?;
    let (mut blocks, blocks_truncated) = bound_blocks(document.blocks, limits);
    let repository_file_actions =
        rewrite_repository_file_links(message_id, content_identity, blocks.as_mut_slice());
    let plain_text = plain_text_for(blocks.as_slice());
    let approximate_bytes = projection_bytes(
        document.title.as_str(),
        plain_text.as_str(),
        blocks.as_slice(),
        repository_file_actions.as_slice(),
    );
    Ok(AgentResponseProjection {
        title: document.title,
        plain_text,
        blocks,
        repository_file_actions,
        truncated: source_truncated || blocks_truncated,
        approximate_bytes,
    })
}

pub fn content_identity(content: &str) -> ContentIdentity {
    ContentIdentity {
        first: stable_hash(0xcbf2_9ce4_8422_2325, &[content.as_bytes()]),
        second: stable_hash(0x8422_2325_cbf2_9ce4, &[content.as_bytes()]),
        utf8_bytes: content.len(),
    }
}

fn bound_blocks(
    blocks: Vec<MarkdownBlock>,
    limits: ProjectionLimits,
) -> (Vec<MarkdownBlock>, bool) {
    if blocks.is_empty() {
        return (Vec::new(), false);
    }
    if limits.max_blocks == 0 || limits.max_utf8_bytes == 0 {
        return (Vec::new(), true);
    }

    let source_block_count = blocks.len();
    let mut projected = Vec::with_capacity(source_block_count.min(limits.max_blocks));
    let mut remaining_bytes = limits.max_utf8_bytes;
    let mut truncated = false;

    for mut block in blocks {
        if projected.len() == limits.max_blocks {
            truncated = true;
            break;
        }
        block.code_spans.clear();
        if block.text.len() <= remaining_bytes {
            remaining_bytes -= block.text.len();
            projected.push(block);
            continue;
        }
        if remaining_bytes > 0 {
            projected.push(truncate_block(block, remaining_bytes));
        }
        truncated = true;
        break;
    }
    if projected.len() < source_block_count && !truncated {
        truncated = true;
    }
    (projected, truncated)
}

fn truncate_block(mut block: MarkdownBlock, max_bytes: usize) -> MarkdownBlock {
    let ellipsis_bytes = usize::from(max_bytes >= 3) * 3;
    let content_bytes = max_bytes.saturating_sub(ellipsis_bytes);
    if block.spans.is_empty() {
        block.text = take_utf8_prefix(block.text.as_str(), content_bytes).to_owned();
        if ellipsis_bytes > 0 {
            block.text.push_str("...");
        }
        return block;
    }

    let mut remaining = content_bytes;
    let mut spans = Vec::new();
    for mut span in block.spans {
        if remaining == 0 {
            break;
        }
        if span.text.len() > remaining {
            span.text = take_utf8_prefix(span.text.as_str(), remaining).to_owned();
        }
        remaining = remaining.saturating_sub(span.text.len());
        if !span.text.is_empty() {
            spans.push(span);
        }
    }
    if ellipsis_bytes > 0 {
        spans.push(usher_preview::MarkdownInlineSpan {
            kind: MarkdownInlineKind::Text,
            text: "...".to_owned(),
            href: String::new(),
        });
    }
    block.text = spans.iter().map(|span| span.text.as_str()).collect();
    block.spans = spans;
    block
}

fn take_utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn rewrite_repository_file_links(
    message_id: &str,
    content_identity: ContentIdentity,
    blocks: &mut [MarkdownBlock],
) -> Vec<RepositoryFileAction> {
    let mut actions = Vec::new();
    let mut action_indexes = HashMap::<String, usize>::new();

    for block in blocks {
        for span in &mut block.spans {
            let candidate = match span.kind {
                MarkdownInlineKind::Link if is_http_link(span.href.as_str()) => continue,
                MarkdownInlineKind::Link => span.href.as_str(),
                MarkdownInlineKind::Code => span.text.as_str(),
                _ => continue,
            };
            let Some(relative_path) = normalize_repository_markdown_path(candidate) else {
                if span.kind == MarkdownInlineKind::Link {
                    span.kind = MarkdownInlineKind::Text;
                    span.href.clear();
                }
                continue;
            };
            let token = repository_action_token(message_id, content_identity, &relative_path);
            if !action_indexes.contains_key(&token) {
                action_indexes.insert(token.clone(), actions.len());
                actions.push(RepositoryFileAction {
                    token: token.clone(),
                    relative_path,
                });
            }
            span.kind = MarkdownInlineKind::Link;
            span.href = format!("{REPOSITORY_FILE_LINK_PREFIX}{token}");
        }
    }
    actions
}

fn is_http_link(value: &str) -> bool {
    let value = value.trim();
    value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn normalize_repository_markdown_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with(['/', '\\', '~'])
        || value.contains(['\0', '?', '%'])
        || value.chars().any(char::is_control)
    {
        return None;
    }
    let without_fragment = value.split_once('#').map_or(value, |(path, _)| path);
    if without_fragment.is_empty() {
        return None;
    }
    let normalized = without_fragment.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
    {
        return None;
    }

    let mut relative = PathBuf::new();
    let mut final_component = None;
    for component in normalized.split('/') {
        match component {
            "" | "." => continue,
            ".." => return None,
            _ if component.contains(':') => return None,
            _ => {
                final_component = Some(component);
                relative.push(component);
            }
        }
    }
    let extension = final_component?
        .rsplit_once('.')
        .map(|(_, extension)| extension)?;
    if !matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown") {
        return None;
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

fn repository_action_token(
    message_id: &str,
    content_identity: ContentIdentity,
    relative_path: &PathBuf,
) -> String {
    let content_first = content_identity.first.to_le_bytes();
    let content_second = content_identity.second.to_le_bytes();
    let content_len = content_identity.utf8_bytes().to_le_bytes();
    let path = relative_path.to_string_lossy();
    let parts = [
        message_id.as_bytes(),
        content_first.as_slice(),
        content_second.as_slice(),
        content_len.as_slice(),
        path.as_bytes(),
    ];
    let first = stable_hash(0x6c62_272e_07bb_0142, &parts);
    let second = stable_hash(0x9e37_79b9_7f4a_7c15, &parts);
    format!("{first:016x}{second:016x}")
}

fn stable_hash(seed: u64, parts: &[&[u8]]) -> u64 {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = seed;
    for part in parts {
        for byte in part.len().to_le_bytes().iter().chain(part.iter()) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

fn plain_text_for(blocks: &[MarkdownBlock]) -> String {
    blocks
        .iter()
        .filter(|block| block.kind != MarkdownBlockKind::Rule)
        .map(|block| block.text.trim_end())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn projection_bytes(
    title: &str,
    plain_text: &str,
    blocks: &[MarkdownBlock],
    actions: &[RepositoryFileAction],
) -> usize {
    let block_bytes = blocks
        .iter()
        .map(|block| {
            mem::size_of::<MarkdownBlock>()
                .saturating_add(block.text.len())
                .saturating_add(block.code_language.len())
                .saturating_add(
                    block
                        .spans
                        .iter()
                        .map(|span| {
                            mem::size_of_val(span)
                                .saturating_add(span.text.len())
                                .saturating_add(span.href.len())
                        })
                        .sum::<usize>(),
                )
        })
        .sum::<usize>();
    let action_bytes = actions
        .iter()
        .map(|action| {
            mem::size_of::<RepositoryFileAction>()
                .saturating_add(action.token.len())
                .saturating_add(action.relative_path.as_os_str().len())
        })
        .sum::<usize>();
    mem::size_of::<AgentResponseProjection>()
        .saturating_add(title.len())
        .saturating_add(plain_text.len())
        .saturating_add(block_bytes)
        .saturating_add(action_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_projection(message_id: &str, content: &str) -> AgentResponseProjection {
        project_agent_response(
            message_id,
            content,
            "Agent response",
            ProjectionLimits::default(),
        )
    }

    #[test]
    fn projection_preserves_markdown_semantics_without_code_highlighting() {
        let projection = default_projection(
            "message-1",
            "# Result\n\nUse **cargo check** and `Cargo.toml`.\n\n- one\n- two\n\n> Note\n\n```rust\nfn main() {}\n```",
        );

        assert_eq!(projection.title, "Result");
        assert!(
            projection.blocks.iter().any(|block| {
                block.kind == MarkdownBlockKind::Heading1 && block.text == "Result"
            })
        );
        assert!(projection.blocks.iter().any(|block| {
            block.kind == MarkdownBlockKind::ListItem && block.text.contains("one")
        }));
        assert!(
            projection
                .blocks
                .iter()
                .any(|block| block.kind == MarkdownBlockKind::Quote)
        );
        let code = projection
            .blocks
            .iter()
            .find(|block| block.kind == MarkdownBlockKind::CodeBlock)
            .expect("code block");
        assert_eq!(code.code_language, "rust");
        assert!(code.code_spans.is_empty());
    }

    #[test]
    fn fallback_title_is_used_when_response_has_no_heading() {
        let projection = project_agent_response(
            "message-1",
            "Plain response",
            "Review answer",
            ProjectionLimits::default(),
        );
        assert_eq!(projection.title, "Review answer");
    }

    #[test]
    fn preview_action_is_limited_to_rich_or_truncated_responses() {
        for content in [
            "Plain response",
            "First plain paragraph.\n\nSecond plain paragraph.",
        ] {
            assert!(!default_projection("plain", content).has_rich_markdown());
        }

        for content in [
            "# Heading",
            "- list item",
            "> quote",
            "```rust\nfn main() {}\n```",
            "---",
            "| Column |\n| --- |\n| value |",
            "Use **strong text**.",
            "Open [documentation](https://example.test).",
            "Read `AGENTS.md`.",
        ] {
            assert!(
                default_projection("rich", content).has_rich_markdown(),
                "expected rich Markdown for {content:?}"
            );
        }

        let truncated = project_agent_response(
            "truncated",
            "A plain response that exceeds its projection budget.",
            "Answer",
            ProjectionLimits {
                max_utf8_bytes: 12,
                max_blocks: 80,
            },
        );
        assert!(truncated.truncated);
        assert!(truncated.has_rich_markdown());
    }

    #[test]
    fn projection_limits_are_unicode_safe_and_stop_at_block_limit() {
        let projection = project_agent_response(
            "message-1",
            "第一段中文\n\nSecond paragraph\n\nThird paragraph",
            "Answer",
            ProjectionLimits {
                max_utf8_bytes: 17,
                max_blocks: 2,
            },
        );

        assert!(projection.truncated);
        assert!(projection.blocks.len() <= 2);
        assert!(
            projection
                .blocks
                .iter()
                .all(|block| block.text.is_char_boundary(block.text.len()))
        );
        assert!(
            projection
                .blocks
                .iter()
                .map(|block| block.text.len())
                .sum::<usize>()
                <= 17
        );
    }

    #[test]
    fn default_projection_caps_compact_blocks_and_keeps_full_preview_available() {
        let content = (0..DEFAULT_PROJECTION_MAX_BLOCKS + 12)
            .map(|index| format!("Paragraph {index}: 中文 🧪"))
            .collect::<Vec<_>>()
            .join("\n\n");

        let projection = default_projection("bounded-default", content.as_str());

        assert_eq!(projection.blocks.len(), DEFAULT_PROJECTION_MAX_BLOCKS);
        assert!(
            projection
                .blocks
                .iter()
                .all(|block| block.text.is_char_boundary(block.text.len()))
        );
        assert!(projection.truncated);
        assert!(projection.has_rich_markdown());
    }

    #[test]
    fn source_budget_bounds_oversized_link_destinations_before_parsing() {
        let oversized = format!(
            "[report](https://example.test/{})",
            "segment/".repeat(32 * 1024)
        );
        let projection = project_agent_response(
            "oversized-link",
            oversized.as_str(),
            "Bounded response",
            ProjectionLimits {
                max_utf8_bytes: 1_024,
                max_blocks: 80,
            },
        );

        assert!(projection.truncated);
        assert!(projection.approximate_bytes < 16 * 1_024);
        assert!(projection.blocks.len() <= 80);
    }

    #[test]
    fn zero_limits_return_an_empty_truncated_projection() {
        let projection = project_agent_response(
            "message-1",
            "# Result",
            "Answer",
            ProjectionLimits {
                max_utf8_bytes: 0,
                max_blocks: 0,
            },
        );
        assert!(projection.blocks.is_empty());
        assert!(projection.truncated);
    }

    #[test]
    fn repository_markdown_links_and_inline_code_become_opaque_actions() {
        let projection = default_projection(
            "message-1",
            "Read [the guide](docs/guide.md#setup), `AGENTS.md`, and `docs/中文.MARKDOWN`.",
        );

        let paths = projection
            .repository_file_actions
            .iter()
            .map(|action| action.relative_path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("docs/guide.md"),
                PathBuf::from("AGENTS.md"),
                PathBuf::from("docs/中文.MARKDOWN"),
            ]
        );
        for action in &projection.repository_file_actions {
            assert_eq!(action.token.len(), 32);
            assert!(
                action
                    .token
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            );
            assert!(!action.token.contains("guide"));
        }
        let hrefs = projection
            .blocks
            .iter()
            .flat_map(|block| block.spans.iter())
            .filter(|span| span.kind == MarkdownInlineKind::Link)
            .map(|span| span.href.as_str())
            .collect::<Vec<_>>();
        assert_eq!(hrefs.len(), 3);
        assert!(
            hrefs
                .iter()
                .all(|href| href.starts_with(REPOSITORY_FILE_LINK_PREFIX))
        );
    }

    #[test]
    fn unsafe_or_non_markdown_paths_do_not_create_repository_actions() {
        let projection = default_projection(
            "message-1",
            "[parent](../secret.md) [absolute](/tmp/a.md) [drive](C:/a.md) \
             [file](file:///tmp/a.md) [mail](mailto:a@example.com) [rust](src/lib.rs) \
             `../inline.md` `C:\\inline.md` `src/lib.rs`",
        );

        assert!(projection.repository_file_actions.is_empty());
        assert!(
            projection
                .blocks
                .iter()
                .flat_map(|block| block.spans.iter())
                .all(|span| !span.href.starts_with(REPOSITORY_FILE_LINK_PREFIX))
        );
    }

    #[test]
    fn external_http_links_are_preserved_without_repository_actions() {
        let projection = default_projection(
            "message-1",
            "Read [docs](https://example.test/docs.md) and [HTTP](HTTP://example.test/a.md).",
        );

        assert!(projection.repository_file_actions.is_empty());
        let hrefs = projection
            .blocks
            .iter()
            .flat_map(|block| block.spans.iter())
            .filter(|span| span.kind == MarkdownInlineKind::Link)
            .map(|span| span.href.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            hrefs,
            vec!["https://example.test/docs.md", "HTTP://example.test/a.md"]
        );
    }

    #[test]
    fn action_tokens_are_stable_and_isolated_by_message() {
        let content = "Read `docs/architecture.md`.";
        let first = default_projection("message-1", content);
        let repeated = default_projection("message-1", content);
        let other_message = default_projection("message-2", content);

        assert_eq!(
            first.repository_file_actions[0].token,
            repeated.repository_file_actions[0].token
        );
        assert_ne!(
            first.repository_file_actions[0].token,
            other_message.repository_file_actions[0].token
        );
    }

    #[test]
    fn cache_uses_message_and_content_identity() {
        let mut projector = AgentResponseProjector::default();
        let first = projector.project("message-1", "# Result", "Answer");
        let repeated = projector.project("message-1", "# Result", "Answer");
        let changed = projector.project("message-1", "# Changed", "Answer");

        assert!(Arc::ptr_eq(&first, &repeated));
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_eq!(
            projector.stats(),
            ProjectionCacheStats {
                hits: 1,
                misses: 2,
                evictions: 0,
                entries: 2,
                bytes: projector.stats().bytes,
            }
        );
    }

    #[test]
    fn cache_evicts_the_least_recently_used_entry_by_count() {
        let mut projector = AgentResponseProjector::new(
            ProjectionLimits::default(),
            ProjectionCacheLimits {
                max_entries: 2,
                max_bytes: DEFAULT_CACHE_MAX_BYTES,
            },
        );
        projector.project("one", "First", "Answer");
        projector.project("two", "Second", "Answer");
        projector.project("one", "First", "Answer");
        projector.project("three", "Third", "Answer");

        let stats = projector.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 3);
        assert_eq!(stats.evictions, 1);
        assert_eq!(stats.entries, 2);

        projector.project("two", "Second", "Answer");
        assert_eq!(projector.stats().misses, 4);
    }

    #[test]
    fn cache_evicts_by_approximate_byte_budget() {
        let sample = default_projection("one", "A compact response");
        let one_entry_budget = sample.approximate_bytes
            + mem::size_of::<ProjectionCacheKey>()
            + mem::size_of::<CachedProjection>()
            + "one".len()
            + 16;
        let mut projector = AgentResponseProjector::new(
            ProjectionLimits::default(),
            ProjectionCacheLimits {
                max_entries: 8,
                max_bytes: one_entry_budget,
            },
        );
        projector.project("one", "A compact response", "Answer");
        projector.project("two", "A compact response", "Answer");

        let stats = projector.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.evictions, 1);
        assert!(stats.bytes <= one_entry_budget);
    }

    #[test]
    fn cancelled_projection_does_not_enter_the_cache() {
        let mut projector = AgentResponseProjector::default();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let result =
            projector.project_with_cancellation("message-1", "# Result", "Answer", &cancellation);

        assert_eq!(result.unwrap_err(), PreviewError::Cancelled);
        assert_eq!(projector.stats(), ProjectionCacheStats::default());
    }
}
