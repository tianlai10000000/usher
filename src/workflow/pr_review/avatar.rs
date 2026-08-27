//! Bounded avatar download and decode for PR Review timelines.

use std::collections::{HashMap, HashSet};

use futures_util::{StreamExt, stream};

use crate::git::{PullRequestCommit, PullRequestConversationItem};
use crate::http::{self, RequestOptions};

pub(super) struct ConversationAvatarPixels {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

pub(super) async fn load_conversation_avatars(
    items: &[PullRequestConversationItem],
) -> HashMap<String, ConversationAvatarPixels> {
    load_avatar_urls(
        items
            .iter()
            .filter_map(|item| {
                let url = item.avatar_url.trim();
                (!url.is_empty()).then(|| url.to_string())
            })
            .collect::<HashSet<_>>()
            .into_iter(),
    )
    .await
}

pub(super) async fn load_commit_avatars(
    commits: &[PullRequestCommit],
) -> HashMap<String, ConversationAvatarPixels> {
    load_avatar_urls(
        commits
            .iter()
            .flat_map(|commit| [commit.avatar_url.trim(), commit.committer_avatar_url.trim()])
            .filter(|url| !url.is_empty())
            .map(str::to_string)
            .collect::<HashSet<_>>()
            .into_iter(),
    )
    .await
}

async fn load_avatar_urls(
    urls: impl Iterator<Item = String>,
) -> HashMap<String, ConversationAvatarPixels> {
    let urls = urls.take(32);
    stream::iter(urls)
        .map(|url| async move {
            let image = load_conversation_avatar(url.as_str()).await?;
            Some((url, image))
        })
        .buffer_unordered(6)
        .filter_map(async move |item| item)
        .collect()
        .await
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
