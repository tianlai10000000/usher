//! Conversation and commit timeline presentation.
//!
//! This module converts provider-neutral timeline records into Slint models and
//! owns deterministic row sizing, connection, grouping, and commit-range math.
//! Avatar download and `MainWindow` mutation remain in the controller façade.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use super::avatar::ConversationAvatarPixels;
use super::cache::BoundedCache;
use crate::git::{PullRequestCommit, PullRequestConversationItem, split_commit_message};
use crate::{WorkflowPrCommit, WorkflowPrConversationItem};

pub(super) fn empty_conversation_model() -> ModelRc<WorkflowPrConversationItem> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrConversationItem>::new()))
}

pub(super) fn empty_commit_model() -> ModelRc<WorkflowPrCommit> {
    ModelRc::new(VecModel::from(Vec::<WorkflowPrCommit>::new()))
}

fn conversation_row_height(item: &PullRequestConversationItem) -> i32 {
    match item.kind.as_str() {
        "commit" => 36,
        "commits" | "event" | "merged" | "review_requested" | "review_summary" => 48,
        _ => conversation_card_row_height(item.body.as_str()),
    }
}

pub(super) fn conversation_content_height(items: &[PullRequestConversationItem]) -> i32 {
    items.iter().map(conversation_row_height).sum()
}

/// GitHub commit-message body under the title (kebab expansion).
/// Prefer a slight under-estimate: over-estimating leaves a blank gap under the
/// body before the author footer. Slint still sizes the visible Text with
/// `preferred-height` so wrapped text is not clipped when the estimate is low.
fn commit_message_body_height(body: &str) -> i32 {
    const BOTTOM_PADDING: i32 = 4;
    // Matches the 12px monospace preformatted body closely enough for ListView
    // row reservation; the visible body uses preferred-height.
    const LINE_HEIGHT: i32 = 17;
    // Commits body ≈ list(≤980) − 250 ≈ 730px ≈ 100 Latin cols at 12px.
    // Conversation body is similar. Wide columns avoid the blank-gap bug.
    const WRAP_COLUMNS: usize = 100;
    const MAX_LINES: i32 = 20;

    if body.trim().is_empty() {
        return 0;
    }
    let mut lines = 0i32;
    for paragraph in body.lines() {
        if paragraph.is_empty() {
            lines += 1;
            continue;
        }
        // Treat full-width / CJK as ~2 columns so wraps are not under-counted.
        let cols = paragraph
            .chars()
            .map(|character| if character > '\u{2E7F}' { 2usize } else { 1 })
            .sum::<usize>()
            .max(1);
        lines += cols.div_ceil(WRAP_COLUMNS) as i32;
    }
    BOTTOM_PADDING + lines.clamp(1, MAX_LINES) * LINE_HEIGHT
}

/// Commits-tab expand layout: title band + body + author band − collapsed row.
fn commit_tab_expand_layout_extra(body_height: i32) -> i32 {
    const TITLE_BAND: i32 = 30;
    const AUTHOR_BAND: i32 = 28;
    const ROW_HEIGHT: i32 = 52;
    body_height + TITLE_BAND + AUTHOR_BAND - ROW_HEIGHT
}

pub(super) fn refresh_commit_group_expand_extras(commits: &mut [WorkflowPrCommit]) {
    let mut index = 0;
    while index < commits.len() {
        if !commits[index].group_start {
            commits[index].group_expand_extra = 0;
            index += 1;
            continue;
        }
        let group_size = commits[index].group_size.max(0) as usize;
        let end = (index + group_size).min(commits.len());
        let extra = commits[index..end]
            .iter()
            .filter(|commit| commit.message_expanded)
            .map(|commit| commit_tab_expand_layout_extra(commit.message_body_height))
            .sum::<i32>();
        commits[index].group_expand_extra = extra;
        for commit in &mut commits[index + 1..end] {
            commit.group_expand_extra = 0;
        }
        index = end.max(index + 1);
    }
}

/// GitHub Conversation cards hug their text: header + padded body lines, not a
/// fixed tall viewport. Long bodies are capped so ListView row heights stay bounded.
fn conversation_card_row_height(body: &str) -> i32 {
    const OUTER_MARGIN: i32 = 12;
    const HEADER: i32 = 36;
    const EMPTY_BODY: i32 = 40;
    const BODY_PADDING: i32 = 24;
    const LINE_HEIGHT: i32 = 20;
    const WRAP_COLUMNS: usize = 72;
    const MAX_LINES: i32 = 12;

    if body.trim().is_empty() {
        return OUTER_MARGIN + HEADER + EMPTY_BODY;
    }

    let mut lines = 0i32;
    for paragraph in body.lines() {
        let chars = paragraph.chars().count().max(1);
        lines += chars.div_ceil(WRAP_COLUMNS) as i32;
    }
    let lines = lines.clamp(1, MAX_LINES);
    OUTER_MARGIN + HEADER + BODY_PADDING + lines * LINE_HEIGHT
}

pub(super) fn conversation_model(
    items: &[PullRequestConversationItem],
    avatars: &HashMap<String, Arc<ConversationAvatarPixels>>,
) -> ModelRc<WorkflowPrConversationItem> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (body, message_body) = if item.kind == "commit" {
                    let (title, body) = split_commit_message(item.body.as_str());
                    let title = if title.is_empty() {
                        item.body.lines().next().unwrap_or("").to_string()
                    } else {
                        title
                    };
                    (title, body)
                } else {
                    (item.body.clone(), String::new())
                };
                let message_body_height = commit_message_body_height(message_body.as_str());
                WorkflowPrConversationItem {
                    id: item.id.clone().into(),
                    kind: item.kind.clone().into(),
                    author: item.author.clone().into(),
                    avatar: cached_avatar_image(avatars, item.avatar_url.as_str()),
                    avatar_label: avatar_label(item.author.as_str()).into(),
                    author_association: item.author_association.clone().into(),
                    body: body.into(),
                    message_body: message_body.into(),
                    message_body_height,
                    message_expanded: false,
                    created_at: super::super::pr_list::format_github_relative_time(
                        item.created_at.as_str(),
                    )
                    .into(),
                    created_at_tooltip: super::super::pr_list::format_github_absolute_tooltip(
                        item.created_at.as_str(),
                    )
                    .into(),
                    state: item.state.clone().into(),
                    review_id: item.review_id.clone().into(),
                    can_dismiss: item.can_dismiss,
                    connect_before: conversation_items_connect(
                        index
                            .checked_sub(1)
                            .and_then(|previous| items.get(previous)),
                        Some(item),
                    ),
                    connect_after: conversation_items_connect(Some(item), items.get(index + 1)),
                    row_height: conversation_row_height(item),
                }
            })
            .collect::<Vec<_>>(),
    ))
}

pub(super) fn patch_conversation_avatars(
    model: &ModelRc<WorkflowPrConversationItem>,
    items: &[PullRequestConversationItem],
    avatars: &HashMap<String, Arc<ConversationAvatarPixels>>,
) {
    for (index, item) in items.iter().enumerate() {
        let Some(mut row) = model.row_data(index) else {
            continue;
        };
        let avatar = cached_avatar_image(avatars, item.avatar_url.as_str());
        if row.avatar != avatar {
            row.avatar = avatar;
            model.set_row_data(index, row);
        }
    }
}

fn conversation_items_connect(
    before: Option<&PullRequestConversationItem>,
    after: Option<&PullRequestConversationItem>,
) -> bool {
    let (Some(before), Some(after)) = (before, after) else {
        return false;
    };
    if before.kind == "merged" {
        return false;
    }
    if after.kind == "review_summary" {
        // Adjacent GitHub reviews are separate summary/card pairs, but their
        // visual timeline remains continuous between the preceding card and
        // the next summary marker.
        return before.kind == "review";
    }
    before.kind == "review_summary" && after.kind == "review" || before.kind != "review_summary"
}

pub(super) fn commit_model(
    commits: &[PullRequestCommit],
    avatars: &HashMap<String, Arc<ConversationAvatarPixels>>,
) -> ModelRc<WorkflowPrCommit> {
    // Provider lists stay oldest→newest for range math; the Commits tab and
    // commit-range picker show newest→oldest.
    let display = commits.iter().rev().collect::<Vec<_>>();
    let date_labels = display
        .iter()
        .map(|commit| format_commit_date(commit_display_timestamp(commit)))
        .collect::<Vec<_>>();
    ModelRc::new(VecModel::from(
        display
            .iter()
            .enumerate()
            .map(|(index, commit)| {
                let date_label = &date_labels[index];
                let group_start = index == 0 || date_labels[index - 1] != *date_label;
                let group_end =
                    index + 1 == date_labels.len() || date_labels[index + 1] != *date_label;
                let group_size = if group_start {
                    date_labels[index..]
                        .iter()
                        .take_while(|candidate| *candidate == date_label)
                        .count()
                } else {
                    0
                };
                let (_, message_body) = split_commit_message(commit.message.as_str());
                let show_committer = commit_has_distinct_committer(commit);
                let identity_action = commit_identity_action(commit, show_committer);
                let display_timestamp = commit_display_timestamp(commit);
                WorkflowPrCommit {
                    sha: commit.sha.clone().into(),
                    short_sha: commit.sha.chars().take(7).collect::<String>().into(),
                    title: commit.title.clone().into(),
                    message: commit.message.clone().into(),
                    message_body_height: commit_message_body_height(message_body.as_str()),
                    message_body: message_body.into(),
                    message_expanded: false,
                    group_expand_extra: 0,
                    author: commit.author.clone().into(),
                    avatar: cached_avatar_image(avatars, commit.avatar_url.as_str()),
                    avatar_label: avatar_label(commit.author.as_str()).into(),
                    // Absolute local datetime for the relative-time tooltip.
                    authored_at: super::super::pr_list::format_github_absolute_tooltip(
                        display_timestamp,
                    )
                    .into(),
                    committer: commit.committer.clone().into(),
                    committer_avatar: cached_avatar_image(
                        avatars,
                        commit.committer_avatar_url.as_str(),
                    ),
                    committer_avatar_label: avatar_label(commit.committer.as_str()).into(),
                    committed_at: super::super::pr_list::format_github_absolute_tooltip(
                        commit.committed_at.as_str(),
                    )
                    .into(),
                    show_committer,
                    identity_action: identity_action.into(),
                    date_label: date_label.clone().into(),
                    // GitHub Commits rows: Primer relative-time (P30D → `on Mar 19`).
                    short_date_label: super::super::pr_list::format_github_relative_time(
                        display_timestamp,
                    )
                    .into(),
                    group_start,
                    group_end,
                    group_size: group_size as i32,
                    verified: commit.verified,
                }
            })
            .collect::<Vec<_>>(),
    ))
}

pub(super) fn patch_commit_avatars(
    model: &ModelRc<WorkflowPrCommit>,
    commits: &[PullRequestCommit],
    avatars: &HashMap<String, Arc<ConversationAvatarPixels>>,
) {
    for (index, commit) in commits.iter().rev().enumerate() {
        let Some(mut row) = model.row_data(index) else {
            continue;
        };
        let avatar = cached_avatar_image(avatars, commit.avatar_url.as_str());
        let committer_avatar = cached_avatar_image(avatars, commit.committer_avatar_url.as_str());
        if row.avatar != avatar || row.committer_avatar != committer_avatar {
            row.avatar = avatar;
            row.committer_avatar = committer_avatar;
            model.set_row_data(index, row);
        }
    }
}

fn commit_has_distinct_committer(commit: &PullRequestCommit) -> bool {
    let author = commit.author.trim();
    let committer = commit.committer.trim();
    let same_avatar = !commit.avatar_url.trim().is_empty()
        && commit
            .avatar_url
            .eq_ignore_ascii_case(commit.committer_avatar_url.trim());
    !author.is_empty()
        && !committer.is_empty()
        && !author.eq_ignore_ascii_case(committer)
        && !same_avatar
}

fn commit_display_timestamp(commit: &PullRequestCommit) -> &str {
    if !commit.committer.trim().is_empty() && !commit.committed_at.trim().is_empty() {
        commit.committed_at.as_str()
    } else {
        commit.authored_at.as_str()
    }
}

fn commit_identity_action(commit: &PullRequestCommit, show_committer: bool) -> String {
    if show_committer {
        format!(
            "{} authored and {} committed",
            commit.author, commit.committer
        )
    } else if !commit.author.trim().is_empty() {
        format!("{} authored", commit.author)
    } else if !commit.committer.trim().is_empty() {
        format!("{} committed", commit.committer)
    } else {
        "Unknown author committed".to_string()
    }
}

const AVATAR_IMAGE_CACHE_CAPACITY: usize = 128;

thread_local! {
    /// `slint::Image` is event-loop-owned rather than `Send`, so keep the
    /// process-lifetime texture cache on the Slint thread. Conversation and
    /// Commits model rebuilds reuse the same refcounted image for one URL.
    static AVATAR_IMAGE_CACHE: RefCell<BoundedCache<Image>> =
        RefCell::new(BoundedCache::new(AVATAR_IMAGE_CACHE_CAPACITY));
}

fn cached_avatar_image(
    avatars: &HashMap<String, Arc<ConversationAvatarPixels>>,
    url: &str,
) -> Image {
    let url = url.trim();
    if url.is_empty() {
        return Image::default();
    }
    AVATAR_IMAGE_CACHE.with(|cache| {
        if let Some(image) = cache.borrow_mut().get_cloned(url) {
            return image;
        }
        let Some(avatar) = avatars.get(url) else {
            return Image::default();
        };
        let image = Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            avatar.rgba.as_slice(),
            avatar.width,
            avatar.height,
        ));
        cache.borrow_mut().insert(url.to_string(), image.clone());
        image
    })
}

fn avatar_label(author: &str) -> String {
    author
        .chars()
        .next()
        .map(|character| character.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_string())
}

/// Map a newest-first display index range onto the oldest→newest provider list.
pub(super) fn display_commit_range_to_chrono(
    display_start: usize,
    display_end: usize,
    len: usize,
) -> Option<(usize, usize)> {
    if len == 0 || display_start >= len || display_end >= len {
        return None;
    }
    let (display_lo, display_hi) = if display_start <= display_end {
        (display_start, display_end)
    } else {
        (display_end, display_start)
    };
    Some((len - 1 - display_hi, len - 1 - display_lo))
}

pub(super) fn chrono_commit_index_to_display(chrono_index: usize, len: usize) -> Option<usize> {
    if len == 0 || chrono_index >= len {
        return None;
    }
    Some(len - 1 - chrono_index)
}

pub(super) fn commit_range_label(
    commits: &[PullRequestCommit],
    start: usize,
    end: usize,
) -> String {
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    if start == end {
        format!("Commit {}", short(commits[start].sha.as_str()))
    } else {
        format!(
            "{} … {}",
            short(commits[start].sha.as_str()),
            short(commits[end].sha.as_str())
        )
    }
}

pub(super) fn commit_index_by_sha(commits: &[PullRequestCommit], sha: &str) -> Option<usize> {
    commits.iter().position(|commit| commit.sha == sha)
}

pub(super) fn range_covers_all_commits(commit_count: usize, start: usize, end: usize) -> bool {
    commit_count > 0 && start == 0 && end == commit_count - 1
}

fn format_commit_date(value: &str) -> String {
    let utc_plus_eight =
        chrono::FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 must be a valid fixed offset");
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| {
            timestamp
                .with_timezone(&utc_plus_eight)
                .format("%b %-d, %Y")
                .to_string()
        })
        .unwrap_or_else(|_| value.split('T').next().unwrap_or(value).to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        chrono_commit_index_to_display, commit_has_distinct_committer, commit_identity_action,
        commit_index_by_sha, commit_message_body_height, commit_model, commit_range_label,
        commit_tab_expand_layout_extra, conversation_card_row_height, conversation_content_height,
        conversation_items_connect, conversation_model, display_commit_range_to_chrono,
        patch_commit_avatars, patch_conversation_avatars, range_covers_all_commits,
        refresh_commit_group_expand_extras,
    };
    use crate::WorkflowPrCommit;
    use crate::git::{PullRequestCommit, PullRequestConversationItem};
    use slint::Model;
    use std::collections::HashMap;

    fn conversation_item(kind: &str, body: &str) -> PullRequestConversationItem {
        PullRequestConversationItem {
            id: kind.to_string(),
            kind: kind.to_string(),
            author: "alice".to_string(),
            avatar_url: String::new(),
            author_association: String::new(),
            body: body.to_string(),
            created_at: String::new(),
            state: String::new(),
            review_id: String::new(),
            can_dismiss: false,
        }
    }

    fn commit(sha: &str) -> PullRequestCommit {
        PullRequestCommit {
            sha: sha.to_string(),
            title: String::new(),
            message: String::new(),
            author: String::new(),
            avatar_url: String::new(),
            authored_at: String::new(),
            committer: String::new(),
            committer_avatar_url: String::new(),
            committed_at: String::new(),
            verified: false,
        }
    }

    #[test]
    fn commit_body_height_stays_compact_for_short_paragraphs() {
        let body = "Prevent pointer events on the loading overlay so the review header stays clickable.\n\nAlign the file diff statistics with the Files changed tab badge.";
        let height = commit_message_body_height(body);
        assert!(height > 0);
        assert!(
            height <= 90,
            "short body reserved {height}px; expected a compact GitHub-like band"
        );
    }

    #[test]
    fn uses_github_author_and_committer_identity_copy_only_when_distinct() {
        let mut commit = commit("abc");
        commit.author = "Alice".to_string();
        commit.committer = "Bob".to_string();
        assert!(commit_has_distinct_committer(&commit));
        assert_eq!(
            commit_identity_action(&commit, true),
            "Alice authored and Bob committed"
        );

        commit.committer = "alice".to_string();
        assert!(!commit_has_distinct_committer(&commit));
        assert_eq!(commit_identity_action(&commit, false), "Alice authored");
    }

    #[test]
    fn avatar_patches_preserve_existing_row_interaction_state() {
        let conversation = vec![conversation_item("commit", "title\nbody")];
        let conversation_model = conversation_model(&conversation, &HashMap::new());
        let mut conversation_row = conversation_model.row_data(0).unwrap();
        conversation_row.message_expanded = true;
        conversation_model.set_row_data(0, conversation_row);
        patch_conversation_avatars(&conversation_model, &conversation, &HashMap::new());
        assert!(conversation_model.row_data(0).unwrap().message_expanded);

        let commits = vec![commit("abc")];
        let commit_model = commit_model(&commits, &HashMap::new());
        let mut commit_row = commit_model.row_data(0).unwrap();
        commit_row.message_expanded = true;
        commit_row.group_expand_extra = 42;
        commit_model.set_row_data(0, commit_row);
        patch_commit_avatars(&commit_model, &commits, &HashMap::new());
        let patched = commit_model.row_data(0).unwrap();
        assert!(patched.message_expanded);
        assert_eq!(patched.group_expand_extra, 42);
    }

    #[test]
    fn commit_group_expand_extras_sum_every_expanded_row() {
        let make =
            |sha: &str, group_start: bool, group_size: i32, expanded: bool, body_height: i32| {
                WorkflowPrCommit {
                    sha: sha.into(),
                    short_sha: sha.into(),
                    title: "title".into(),
                    message: "title".into(),
                    message_body: "body".into(),
                    message_body_height: body_height,
                    message_expanded: expanded,
                    group_expand_extra: 0,
                    author: "alice".into(),
                    avatar: Default::default(),
                    avatar_label: "A".into(),
                    authored_at: "".into(),
                    committer: "".into(),
                    committer_avatar: Default::default(),
                    committer_avatar_label: "?".into(),
                    committed_at: "".into(),
                    show_committer: false,
                    identity_action: "alice authored".into(),
                    date_label: "Jul 20, 2026".into(),
                    short_date_label: "2 days ago".into(),
                    group_start,
                    group_end: false,
                    group_size,
                    verified: false,
                }
            };
        let mut commits = vec![
            make("aaa", true, 2, true, 40),
            make("bbb", false, 0, true, 20),
        ];
        refresh_commit_group_expand_extras(&mut commits);
        let expected = commit_tab_expand_layout_extra(40) + commit_tab_expand_layout_extra(20);
        assert_eq!(commits[0].group_expand_extra, expected);
        assert_eq!(commits[1].group_expand_extra, 0);
        assert!(commits[0].message_expanded && commits[1].message_expanded);
    }

    #[test]
    fn measures_conversation_rows_without_viewport_filler_space() {
        let items = vec![
            conversation_item("description", ""),
            conversation_item("commits", "added 2 commits"),
            conversation_item("commit", "first"),
            conversation_item("commit", "second"),
            conversation_item("merged", "abcdef0"),
            conversation_item("event", "closed this pull request"),
        ];

        assert_eq!(conversation_content_height(&items), 304);
        assert_eq!(conversation_card_row_height("请先解决冲突"), 92);
        assert_eq!(conversation_card_row_height(""), 88);
    }

    #[test]
    fn connects_adjacent_review_pairs_without_crossing_terminal_merge() {
        let summary = conversation_item("review_summary", "");
        let review = conversation_item("review", "");
        let next_summary = conversation_item("review_summary", "");
        let next_review = conversation_item("review", "");
        let merged = conversation_item("merged", "");

        assert!(conversation_items_connect(Some(&summary), Some(&review)));
        assert!(conversation_items_connect(
            Some(&review),
            Some(&next_summary)
        ));
        assert!(conversation_items_connect(
            Some(&next_summary),
            Some(&next_review)
        ));
        assert!(!conversation_items_connect(Some(&merged), Some(&summary)));
    }

    #[test]
    fn labels_single_and_multi_commit_ranges() {
        let commits = vec![
            commit("1111111aaaaaaaa"),
            commit("2222222bbbbbbbb"),
            commit("3333333cccccccc"),
        ];
        assert_eq!(commit_range_label(&commits, 1, 1), "Commit 2222222");
        assert_eq!(commit_range_label(&commits, 0, 2), "1111111 … 3333333");
    }

    #[test]
    fn resolves_commit_selection_by_full_sha() {
        let commits = vec![
            commit("1111111aaaaaaaa"),
            commit("2222222bbbbbbbb"),
            commit("3333333cccccccc"),
        ];
        assert_eq!(commit_index_by_sha(&commits, "2222222bbbbbbbb"), Some(1));
        assert_eq!(commit_index_by_sha(&commits, "2222222"), None);
    }

    #[test]
    fn maps_newest_first_display_ranges_onto_oldest_first_provider_lists() {
        assert_eq!(display_commit_range_to_chrono(0, 0, 3), Some((2, 2)));
        assert_eq!(display_commit_range_to_chrono(0, 2, 3), Some((0, 2)));
        assert_eq!(display_commit_range_to_chrono(0, 1, 3), Some((1, 2)));
        assert_eq!(display_commit_range_to_chrono(2, 0, 3), Some((0, 2)));
        assert_eq!(chrono_commit_index_to_display(0, 3), Some(2));
        assert_eq!(chrono_commit_index_to_display(2, 3), Some(0));
        assert_eq!(display_commit_range_to_chrono(0, 0, 0), None);
        assert_eq!(chrono_commit_index_to_display(0, 0), None);
    }

    #[test]
    fn identifies_the_full_commit_range() {
        assert!(range_covers_all_commits(1, 0, 0));
        assert!(range_covers_all_commits(3, 0, 2));
        assert!(!range_covers_all_commits(3, 0, 1));
        assert!(!range_covers_all_commits(3, 1, 2));
        assert!(!range_covers_all_commits(0, 0, 0));
    }
}
