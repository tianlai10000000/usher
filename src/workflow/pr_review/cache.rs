//! PR Review cache ownership and memory accounting.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::controller::ReviewRequest;
use crate::git::{PullRequestDiffDetail, PullRequestDiffFile, PullRequestDiffLine};
use crate::project::ProjectRepositoryMetadata;
use crate::workflow::syntax_highlight::{HighlightedLine, HighlightedToken};

pub(super) const HIGHLIGHT_CACHE_CAPACITY: usize = 4;
pub(super) const SOURCE_CACHE_CAPACITY: usize = 3;
pub(super) const RANGE_DIFF_CACHE_CAPACITY: usize = 4;

#[derive(Debug)]
pub(super) struct BoundedCache<T> {
    capacity: usize,
    pub(super) entries: HashMap<String, T>,
    order: VecDeque<String>,
}

impl<T> BoundedCache<T> {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    pub(super) fn remove(&mut self, key: &str) {
        self.entries.remove(key);
        self.order.retain(|candidate| candidate != key);
    }

    pub(super) fn insert(&mut self, key: String, value: T) {
        self.remove(key.as_str());
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);
        while self.entries.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(evicted.as_str());
            }
        }
    }
}

impl<T: Clone> BoundedCache<T> {
    pub(super) fn get_cloned(&mut self, key: &str) -> Option<T> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|candidate| candidate != key);
        self.order.push_back(key.to_string());
        Some(value)
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct OwnedMemoryStats {
    pub(super) items: usize,
    pub(super) children: usize,
    pub(super) text_bytes: usize,
    pub(super) estimated_heap_bytes: usize,
}

#[derive(Debug)]
pub(super) struct ReviewCache {
    pub(super) project_alias: String,
    pub(super) request: ReviewRequest,
    pub(super) metadata: ProjectRepositoryMetadata,
    pub(super) all_detail: Arc<SharedDiffDetail>,
    pub(super) base_detail: Arc<SharedDiffDetail>,
    pub(super) range_diffs: BoundedCache<Arc<SharedDiffDetail>>,
    pub(super) expanded_files: HashMap<usize, Arc<PullRequestDiffFile>>,
    pub(super) loaded_large_diffs: HashSet<usize>,
}

#[derive(Debug)]
pub(super) struct SharedDiffDetail {
    pub(super) files: Vec<Arc<PullRequestDiffFile>>,
    pub(super) added_lines: u32,
    pub(super) removed_lines: u32,
    pub(super) commit_count: u32,
    pub(super) conversation_count: Option<u32>,
    pub(super) base_sha: String,
    pub(super) head_sha: String,
    pub(super) too_many_commits: bool,
    pub(super) files_truncated: bool,
}

impl From<PullRequestDiffDetail> for SharedDiffDetail {
    fn from(detail: PullRequestDiffDetail) -> Self {
        Self {
            files: detail.files.into_iter().map(Arc::new).collect(),
            added_lines: detail.added_lines,
            removed_lines: detail.removed_lines,
            commit_count: detail.commit_count,
            conversation_count: detail.conversation_count,
            base_sha: detail.base_sha,
            head_sha: detail.head_sha,
            too_many_commits: detail.too_many_commits,
            files_truncated: detail.files_truncated,
        }
    }
}

pub(super) type SharedDiffFile = Arc<PullRequestDiffFile>;

impl ReviewCache {
    pub(super) fn file(&self, index: usize) -> Option<&PullRequestDiffFile> {
        self.expanded_files
            .get(&index)
            .map(AsRef::as_ref)
            .or_else(|| self.base_detail.files.get(index).map(AsRef::as_ref))
    }

    pub(super) fn shared_file(&self, index: usize) -> Option<SharedDiffFile> {
        if let Some(file) = self.expanded_files.get(&index) {
            return Some(file.clone());
        }
        self.base_detail.files.get(index).cloned()
    }

    pub(super) fn file_mut_cow(&mut self, index: usize) -> Option<&mut PullRequestDiffFile> {
        if !self.expanded_files.contains_key(&index) {
            let original = self.base_detail.files.get(index)?.as_ref().clone();
            self.expanded_files.insert(index, Arc::new(original));
        }
        self.expanded_files.get_mut(&index).map(Arc::make_mut)
    }

    pub(super) fn collapse_file(&mut self, index: usize) -> Option<SharedDiffFile> {
        self.expanded_files.remove(&index)?;
        self.shared_file(index)
    }
}

pub(super) fn diff_storage_stats<T>(files: &[T]) -> OwnedMemoryStats
where
    T: Borrow<PullRequestDiffFile>,
{
    let mut stats = OwnedMemoryStats {
        items: files.len(),
        estimated_heap_bytes: std::mem::size_of_val(files),
        ..OwnedMemoryStats::default()
    };
    for file in files {
        let file = file.borrow();
        stats.text_bytes = stats
            .text_bytes
            .saturating_add(file.path.len())
            .saturating_add(file.old_path.len())
            .saturating_add(file.change_type.as_str().len());
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(file.path.capacity())
            .saturating_add(file.old_path.capacity())
            .saturating_add(
                file.lines
                    .capacity()
                    .saturating_mul(std::mem::size_of::<PullRequestDiffLine>()),
            );
        stats.children = stats.children.saturating_add(file.lines.len());
        for line in &file.lines {
            stats.text_bytes = stats
                .text_bytes
                .saturating_add(line.kind.as_str().len())
                .saturating_add(line.content.len());
            stats.estimated_heap_bytes = stats
                .estimated_heap_bytes
                .saturating_add(line.content.capacity());
        }
    }
    stats
}

pub(super) fn diff_override_storage_stats(
    files: &HashMap<usize, Arc<PullRequestDiffFile>>,
) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats {
        estimated_heap_bytes: files.capacity().saturating_mul(
            std::mem::size_of::<usize>() + std::mem::size_of::<Arc<PullRequestDiffFile>>(),
        ),
        ..OwnedMemoryStats::default()
    };
    for file in files.values() {
        let file_stats = diff_storage_stats(std::slice::from_ref(file));
        stats.items = stats.items.saturating_add(file_stats.items);
        stats.children = stats.children.saturating_add(file_stats.children);
        stats.text_bytes = stats.text_bytes.saturating_add(file_stats.text_bytes);
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(file_stats.estimated_heap_bytes);
    }
    stats
}

pub(super) fn owned_diff_storage_stats(cache: &ReviewCache) -> (usize, OwnedMemoryStats) {
    let mut seen = HashSet::new();
    let mut stats = OwnedMemoryStats::default();
    let details = std::iter::once(&cache.all_detail)
        .chain(std::iter::once(&cache.base_detail))
        .chain(cache.range_diffs.entries.values());
    for detail in details {
        if !seen.insert(Arc::as_ptr(detail) as usize) {
            continue;
        }
        let detail_stats = diff_storage_stats(&detail.files);
        stats.items = stats.items.saturating_add(detail_stats.items);
        stats.children = stats.children.saturating_add(detail_stats.children);
        stats.text_bytes = stats
            .text_bytes
            .saturating_add(detail_stats.text_bytes)
            .saturating_add(detail.base_sha.len())
            .saturating_add(detail.head_sha.len());
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(detail_stats.estimated_heap_bytes)
            .saturating_add(detail.base_sha.capacity())
            .saturating_add(detail.head_sha.capacity());
    }
    (seen.len(), stats)
}

pub(super) fn highlighted_cache_stats(
    cache: &BoundedCache<Arc<Vec<HighlightedLine>>>,
) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats::default();
    for (key, lines) in &cache.entries {
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(key.capacity())
            .saturating_add(
                lines
                    .capacity()
                    .saturating_mul(std::mem::size_of::<HighlightedLine>()),
            );
        stats.items = stats.items.saturating_add(lines.len());
        for line in lines.iter() {
            stats.children = stats.children.saturating_add(line.tokens.len());
            stats.estimated_heap_bytes = stats.estimated_heap_bytes.saturating_add(
                line.tokens
                    .capacity()
                    .saturating_mul(std::mem::size_of::<HighlightedToken>()),
            );
            for token in &line.tokens {
                stats.text_bytes = stats.text_bytes.saturating_add(token.text.len());
                stats.estimated_heap_bytes = stats
                    .estimated_heap_bytes
                    .saturating_add(token.text.capacity());
            }
        }
    }
    stats
}

pub(super) fn source_cache_stats(cache: &BoundedCache<String>) -> OwnedMemoryStats {
    let mut stats = OwnedMemoryStats::default();
    for (key, source) in &cache.entries {
        stats.items = stats.items.saturating_add(1);
        stats.text_bytes = stats.text_bytes.saturating_add(source.len());
        stats.estimated_heap_bytes = stats
            .estimated_heap_bytes
            .saturating_add(key.capacity())
            .saturating_add(source.capacity());
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::BoundedCache;

    #[test]
    fn evicts_the_least_recently_used_entry() {
        let mut cache = BoundedCache::new(2);
        cache.insert("first".to_string(), 1);
        cache.insert("second".to_string(), 2);
        assert_eq!(cache.get_cloned("first"), Some(1));
        cache.insert("third".to_string(), 3);
        assert_eq!(cache.get_cloned("second"), None);
        assert_eq!(cache.get_cloned("first"), Some(1));
        assert_eq!(cache.get_cloned("third"), Some(3));
    }
}
