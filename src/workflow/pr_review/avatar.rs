//! Bounded avatar download and decode for PR Review timelines.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::{StreamExt, stream};

use super::cache::BoundedCache;
use crate::git::{PullRequestCommit, PullRequestConversationItem};
use crate::http::{self, RequestOptions};

/// Newest downloads per refresh; conversation and commits tabs rarely exceed
/// this many distinct authors, and cache hits do not count against it.
const MAX_AVATAR_DOWNLOADS: usize = 32;
/// Per-URL budget. A slow or stalled avatar host no longer discards the
/// already-decoded results of its siblings.
const AVATAR_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3);
/// Process-wide decoded-avatar budget. 128 URLs × 80×80 RGBA (≈25 KiB each)
/// caps the cache near 3 MiB while covering the conversation and commit author
/// sets of the last handful of reviewed pull requests.
const AVATAR_CACHE_CAPACITY: usize = 128;

pub(super) struct ConversationAvatarPixels {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

/// Decoded avatars shared across PRs and across the conversation and commit
/// loaders. Pixel data (not `slint::Image`) is cached so entries stay `Send`.
fn avatar_pixel_cache() -> &'static Mutex<BoundedCache<Arc<ConversationAvatarPixels>>> {
    static CACHE: OnceLock<Mutex<BoundedCache<Arc<ConversationAvatarPixels>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BoundedCache::new(AVATAR_CACHE_CAPACITY)))
}

fn cached_avatar(url: &str) -> Option<Arc<ConversationAvatarPixels>> {
    avatar_pixel_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_cloned(url)
}

fn store_avatar(url: &str, pixels: &Arc<ConversationAvatarPixels>) {
    avatar_pixel_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(url.to_string(), pixels.clone());
}

pub(super) async fn load_conversation_avatars(
    items: &[PullRequestConversationItem],
) -> HashMap<String, Arc<ConversationAvatarPixels>> {
    load_avatar_urls(items.iter().filter_map(|item| {
        let url = item.avatar_url.trim();
        (!url.is_empty()).then(|| url.to_string())
    }))
    .await
}

pub(super) async fn load_commit_avatars(
    commits: &[PullRequestCommit],
) -> HashMap<String, Arc<ConversationAvatarPixels>> {
    load_avatar_urls(
        commits
            .iter()
            .flat_map(|commit| [commit.avatar_url.trim(), commit.committer_avatar_url.trim()])
            .filter(|url| !url.is_empty())
            .map(str::to_string),
    )
    .await
}

async fn load_avatar_urls(
    urls: impl Iterator<Item = String>,
) -> HashMap<String, Arc<ConversationAvatarPixels>> {
    // Deduplicate in first-occurrence order so the bounded download subset is
    // deterministic across refreshes instead of following HashSet iteration.
    let mut seen = HashSet::new();
    let mut resolved = HashMap::new();
    let mut pending = Vec::new();
    for url in urls {
        if !seen.insert(url.clone()) {
            continue;
        }
        if let Some(pixels) = cached_avatar(url.as_str()) {
            resolved.insert(url, pixels);
        } else if pending.len() < MAX_AVATAR_DOWNLOADS {
            pending.push(url);
        }
    }

    let downloaded = stream::iter(pending)
        .map(|url| async move {
            let pixels = tokio::time::timeout(
                AVATAR_DOWNLOAD_TIMEOUT,
                load_conversation_avatar(url.as_str()),
            )
            .await
            .ok()
            .flatten()?;
            Some((url, Arc::new(pixels)))
        })
        .buffer_unordered(6)
        .filter_map(async move |item| item)
        .collect::<Vec<_>>()
        .await;
    for (url, pixels) in downloaded {
        store_avatar(url.as_str(), &pixels);
        resolved.insert(url, pixels);
    }
    resolved
}

async fn load_conversation_avatar(url: &str) -> Option<ConversationAvatarPixels> {
    const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

    let mut response = http::send(
        http::get(url),
        RequestOptions::new("Load pull request conversation avatar"),
    )
    .await
    .ok()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_AVATAR_BYTES as u64)
    {
        return None;
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_AVATAR_BYTES {
            return None;
        }
        bytes.extend_from_slice(chunk.as_ref());
    }

    let decoded = image::load_from_memory(bytes.as_slice()).ok()?;
    let resized = decoded
        .resize_to_fill(80, 80, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    Some(ConversationAvatarPixels {
        width: resized.width(),
        height: resized.height(),
        rgba: resized.into_raw(),
    })
}
