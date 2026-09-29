use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::super::prefix_cache::PrefixCache;
use super::super::{PullRequestFilters, PullRequestItem, PullRequestState, RepoLocator};

const CACHE_CAPACITY: usize = 32;
const CACHE_TTL: Duration = Duration::from_secs(30);

pub(super) const SCAN_PAGE_SIZE: u32 = 100;
pub(super) const MAX_SCAN_PAGES: u32 = 30;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    authority: String,
    namespace: String,
    repository: String,
    server_state: String,
    sort: String,
    page_size: u32,
}

#[derive(Clone, Default)]
pub(super) struct Entry {
    pub(super) items: Vec<PullRequestItem>,
    pub(super) exhausted: bool,
    pub(super) next_page: u32,
}

fn cache() -> &'static Mutex<PrefixCache<CacheKey, Entry>> {
    static CACHE: OnceLock<Mutex<PrefixCache<CacheKey, Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(PrefixCache::new(CACHE_CAPACITY, CACHE_TTL)))
}

pub(super) fn server_state(state: Option<PullRequestState>) -> Option<PullRequestState> {
    if matches!(
        state,
        Some(PullRequestState::Merged | PullRequestState::ClosedUnmerged)
    ) {
        Some(PullRequestState::Closed)
    } else {
        state
    }
}

fn cache_key(
    locator: &RepoLocator,
    server_state: Option<PullRequestState>,
    filters: &PullRequestFilters,
) -> CacheKey {
    CacheKey {
        authority: locator.authority.clone(),
        namespace: locator.namespace.clone(),
        repository: locator.repo.clone(),
        server_state: server_state.map_or_else(|| "all".to_string(), |state| state.as_str().into()),
        sort: filters.sort.clone(),
        page_size: SCAN_PAGE_SIZE,
    }
}

pub(super) fn get(
    locator: &RepoLocator,
    server_state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    refresh: bool,
) -> Option<Arc<Entry>> {
    let key = cache_key(locator, server_state, filters);
    let mut cache = cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if refresh {
        cache.remove(&key);
        None
    } else {
        cache.get(&key)
    }
}

pub(super) fn insert(
    locator: &RepoLocator,
    server_state: Option<PullRequestState>,
    filters: &PullRequestFilters,
    entry: Entry,
) {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(cache_key(locator, server_state, filters), entry);
}

pub(super) fn invalidate(locator: &RepoLocator) {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .invalidate_where(|key| {
            key.authority == locator.authority
                && key.namespace == locator.namespace
                && key.repository == locator.repo
        });
}

pub(super) fn filter(
    items: &[PullRequestItem],
    state: Option<PullRequestState>,
    query: &str,
) -> Vec<PullRequestItem> {
    items
        .iter()
        .filter(|item| {
            (state != Some(PullRequestState::Merged) || item.state == "merged")
                && (state != Some(PullRequestState::ClosedUnmerged) || item.state == "closed")
                && matches_query(item, query)
        })
        .cloned()
        .collect()
}

pub(super) fn merge(existing: &mut Vec<PullRequestItem>, incoming: Vec<PullRequestItem>) {
    for item in incoming {
        if item.number != "—"
            && let Some(index) = existing
                .iter()
                .position(|candidate| candidate.number == item.number)
        {
            existing[index] = item;
        } else {
            existing.push(item);
        }
    }
}

fn matches_query(item: &PullRequestItem, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        item.title, item.author, item.source_branch, item.target_branch
    )
    .to_ascii_lowercase();
    query
        .split_whitespace()
        .map(|term| term.trim_matches(['\'', '"']).to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .all(|term| haystack.contains(term.as_str()))
}

#[cfg(test)]
mod tests {
    use super::{cache_key, filter, merge, server_state};
    use crate::git::{
        PullRequestFilters, PullRequestItem, PullRequestReviewStatus, PullRequestState,
        parse_repo_locator,
    };

    fn public_list_item(
        number: &str,
        title: &str,
        state: &str,
        author: &str,
        source_branch: &str,
        target_branch: &str,
        updated_at: &str,
    ) -> PullRequestItem {
        PullRequestItem {
            number: number.to_string(),
            title: title.to_string(),
            state: state.to_string(),
            author: author.to_string(),
            source_branch: source_branch.to_string(),
            target_branch: target_branch.to_string(),
            review_status: PullRequestReviewStatus::None,
            conflict_status: Default::default(),
            conversation_count: None,
            created_at: String::new(),
            updated_at: updated_at.to_string(),
            web_url: String::new(),
        }
    }

    #[test]
    fn merge_replaces_duplicate_iids_without_reordering() {
        let mut items = vec![
            public_list_item("3", "third", "closed", "alice", "three", "main", "old"),
            public_list_item("2", "second", "merged", "bob", "two", "main", "old"),
        ];

        merge(
            &mut items,
            vec![
                public_list_item("2", "second", "merged", "bob", "two", "main", "new"),
                public_list_item("1", "first", "closed", "carol", "one", "main", "new"),
            ],
        );

        assert_eq!(
            items
                .iter()
                .map(|item| item.number.as_str())
                .collect::<Vec<_>>(),
            ["3", "2", "1"]
        );
        assert_eq!(items[1].updated_at, "new");
    }

    #[test]
    fn merged_and_closed_unmerged_share_the_closed_raw_prefix() {
        let locator = parse_repo_locator("https://gitee.com/platform/usher.git").unwrap();
        let merged_filters = PullRequestFilters {
            query: "release".to_string(),
            sort: "updated-desc".to_string(),
            ..PullRequestFilters::default()
        };
        let closed_filters = PullRequestFilters {
            query: "bugfix".to_string(),
            ..merged_filters.clone()
        };

        let merged_key = cache_key(
            &locator,
            server_state(Some(PullRequestState::Merged)),
            &merged_filters,
        );
        let closed_key = cache_key(
            &locator,
            server_state(Some(PullRequestState::ClosedUnmerged)),
            &closed_filters,
        );

        assert_eq!(merged_key, closed_key);
        assert_eq!(merged_key.server_state, "closed");
    }

    #[test]
    fn filters_merged_and_closed_unmerged_locally() {
        let items = vec![
            public_list_item("3", "merged", "merged", "alice", "one", "main", ""),
            public_list_item("2", "closed", "closed", "bob", "two", "main", ""),
            public_list_item("1", "open", "open", "carol", "three", "main", ""),
        ];

        assert_eq!(
            filter(&items, Some(PullRequestState::Merged), "")
                .iter()
                .map(|item| item.number.as_str())
                .collect::<Vec<_>>(),
            ["3"]
        );
        assert_eq!(
            filter(&items, Some(PullRequestState::ClosedUnmerged), "")
                .iter()
                .map(|item| item.number.as_str())
                .collect::<Vec<_>>(),
            ["2"]
        );
    }

    #[test]
    fn query_matches_all_terms_across_searchable_fields() {
        let items = vec![
            public_list_item(
                "2",
                "Fix release pipeline",
                "closed",
                "Alice",
                "bugfix/runner",
                "main",
                "",
            ),
            public_list_item(
                "1",
                "Document pipeline",
                "closed",
                "Bob",
                "docs",
                "main",
                "",
            ),
        ];

        let matches = filter(
            &items,
            Some(PullRequestState::ClosedUnmerged),
            "\"release\" ALICE bugfix",
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].number, "2");
    }
}
