use std::fmt;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoType {
    Gitee,
    GitLab,
    GitHub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GiteeApiProfile {
    PublicV5,
    EnterpriseV8,
}

impl RepoType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Gitee => "gitee",
            Self::GitLab => "gitlab",
            Self::GitHub => "github",
        }
    }
}

impl fmt::Display for RepoType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoDetail {
    pub(crate) repo_project_id: String,
    pub(crate) repo_type: RepoType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestItem {
    pub(crate) number: String,
    pub(crate) title: String,
    pub(crate) state: String,
    pub(crate) author: String,
    pub(crate) source_branch: String,
    pub(crate) target_branch: String,
    /// Provider-neutral review decision shown on the pull-request list.
    pub(crate) review_status: PullRequestReviewStatus,
    pub(crate) conflict_status: super::conflicts::ConflictStatus,
    pub(crate) conversation_count: Option<u32>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) web_url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PullRequestReviewStatus {
    #[default]
    None,
    ReviewRequired,
    Approved,
    ChangesRequested,
}

impl PullRequestReviewStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::ReviewRequired => "review_required",
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestPage {
    pub(crate) items: Vec<PullRequestItem>,
    pub(crate) total: u32,
    pub(crate) total_exact: bool,
    /// Confirmed page count, including provider metadata available separately
    /// from the total-item header. Unknown totals must not use a next-page
    /// probe as a last-page number.
    pub(crate) page_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct PullRequestFilters {
    /// Provider-neutral title/search terms entered in the GitHub-style query
    /// field after portable qualifiers have been removed.
    pub(crate) query: String,
    pub(crate) reviews: String,
    pub(crate) assignee: String,
    pub(crate) sort: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffLine {
    pub(crate) old_line: i32,
    pub(crate) new_line: i32,
    pub(crate) kind: DiffLineKind,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    Addition,
    Deletion,
    Context,
    Hunk,
    Meta,
}

impl DiffLineKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Addition => "addition",
            Self::Deletion => "deletion",
            Self::Context => "context",
            Self::Hunk => "hunk",
            Self::Meta => "meta",
        }
    }

    pub(crate) fn from_provider(value: &str) -> Self {
        match value {
            "addition" | "added" | "new" => Self::Addition,
            "deletion" | "removed" | "old" => Self::Deletion,
            "hunk" | "match" => Self::Hunk,
            "meta" => Self::Meta,
            _ => Self::Context,
        }
    }
}

impl From<&str> for DiffLineKind {
    fn from(value: &str) -> Self {
        Self::from_provider(value)
    }
}

impl From<String> for DiffLineKind {
    fn from(value: String) -> Self {
        Self::from_provider(value.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffChangeType {
    New,
    Deleted,
    Renamed,
    Modified,
}

impl DiffChangeType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new_file",
            Self::Deleted => "deleted_file",
            Self::Renamed => "renamed_file",
            Self::Modified => "edit_file",
        }
    }

    pub(crate) fn from_provider(value: &str) -> Self {
        match value {
            "added" | "new" | "new_file" => Self::New,
            "deleted" | "removed" | "deleted_file" => Self::Deleted,
            "renamed" | "renamed_file" => Self::Renamed,
            _ => Self::Modified,
        }
    }
}

impl From<&str> for DiffChangeType {
    fn from(value: &str) -> Self {
        Self::from_provider(value)
    }
}

impl From<String> for DiffChangeType {
    fn from(value: String) -> Self {
        Self::from_provider(value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffFile {
    pub(crate) path: String,
    pub(crate) old_path: String,
    pub(crate) change_type: DiffChangeType,
    pub(crate) added_lines: u32,
    pub(crate) removed_lines: u32,
    pub(crate) viewed: bool,
    pub(crate) lines: Vec<PullRequestDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestDiffDetail {
    pub(crate) author_identity: PullRequestAuthorIdentity,
    pub(crate) files: Vec<PullRequestDiffFile>,
    pub(crate) added_lines: u32,
    pub(crate) removed_lines: u32,
    pub(crate) commit_count: u32,
    /// Provider summary for the Conversation tab badge. `None` on commit-range
    /// compares so applying a range cannot wipe the PR-level badge.
    pub(crate) conversation_count: Option<u32>,
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
    pub(crate) start_sha: String,
    pub(crate) too_many_commits: bool,
    pub(crate) files_truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PullRequestAuthorIdentity {
    pub(crate) name: Option<String>,
    pub(crate) email: Option<String>,
}

impl PullRequestAuthorIdentity {
    pub(crate) fn from_user(user: &serde_json::Value) -> Self {
        let field = |key| {
            user.get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        Self {
            name: field("name"),
            email: field("email").or_else(|| field("public_email")),
        }
    }
}

pub(crate) struct PageCollector<T> {
    values: Vec<T>,
    page: u32,
    per_page: usize,
    max_pages: u32,
    truncated: bool,
}

impl<T> PageCollector<T> {
    pub(crate) fn new(per_page: usize, max_pages: u32) -> Self {
        Self {
            values: Vec::new(),
            page: 1,
            per_page: per_page.max(1),
            max_pages: max_pages.max(1),
            truncated: false,
        }
    }

    pub(crate) fn page(&self) -> u32 {
        self.page
    }

    /// Returns whether another page should be requested.
    pub(crate) fn append(&mut self, batch: Vec<T>) -> bool {
        let page_was_full = batch.len() >= self.per_page;
        self.values.extend(batch);
        if !page_was_full {
            return false;
        }
        if self.page >= self.max_pages {
            self.truncated = true;
            return false;
        }
        self.page += 1;
        true
    }

    pub(crate) fn into_parts(self) -> (Vec<T>, bool) {
        (self.values, self.truncated)
    }
}

/// Read the Conversation-tab badge summary from a provider PR/MR detail payload.
/// Returns `None` when the provider omitted every known count field so callers can
/// fall back to a lightweight probe without treating "missing" as zero.
pub(crate) fn conversation_count_from_detail(detail: &serde_json::Value) -> Option<u32> {
    let root = detail;
    let detail = unwrap_detail_object(root);
    let direct = [
        "comments",
        "comments_count",
        "commentsCount",
        "user_notes_count",
        "userNotesCount",
        "notes_count",
        "notesCount",
        "comment_count",
        "commentCount",
    ]
    .into_iter()
    .find_map(|key| detail.get(key).and_then(json_u32));

    direct.or_else(|| {
        // Gitee Enterprise notes responses carry the exact total beside the
        // paged list rather than on each MR detail object.
        root.get("note_statistics")
            .or_else(|| {
                root.get("data")
                    .and_then(|data| data.get("note_statistics"))
            })
            .and_then(|statistics| {
                ["all_note_count", "allNoteCount", "note_count", "noteCount"]
                    .into_iter()
                    .find_map(|key| statistics.get(key).and_then(json_u32))
            })
    })
}

/// Read the Commits-tab badge summary from a provider PR/MR detail payload.
/// Returns `None` when the provider omitted every known count field.
pub(crate) fn commit_count_from_detail(detail: &serde_json::Value) -> Option<u32> {
    let detail = unwrap_detail_object(detail);
    [
        "commits",
        "commits_count",
        "commitsCount",
        "commit_count",
        "commitCount",
    ]
    .into_iter()
    .find_map(|key| detail.get(key).and_then(json_u32))
}

fn unwrap_detail_object(detail: &serde_json::Value) -> &serde_json::Value {
    detail
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(detail)
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<u32>().ok())
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestConversationItem {
    pub(crate) id: String,
    /// Portable timeline kinds: `description`, `comment`, `review`, `event`,
    /// `review_requested`, `review_summary`, `commits` (batch header), `commit`
    /// (row under a batch), `merged`.
    pub(crate) kind: String,
    pub(crate) author: String,
    pub(crate) avatar_url: String,
    /// Provider author association (for example GitHub `AUTHOR`).
    pub(crate) author_association: String,
    pub(crate) body: String,
    pub(crate) created_at: String,
    /// Extra display payload: review state, short SHA, target branch, or commit count.
    pub(crate) state: String,
    /// Provider review identifier used by GitHub-style per-review actions.
    pub(crate) review_id: String,
    /// Whether the configured provider identity may dismiss this exact review.
    pub(crate) can_dismiss: bool,
}

pub(crate) fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// First line is the title; remaining lines (after blank separators) are the body.
pub(crate) fn split_commit_message(message: &str) -> (String, String) {
    let mut lines = message.lines();
    let title = lines.next().unwrap_or("").trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n");
    let body = body.trim().to_string();
    (title, body)
}

/// Build a GitHub-style "added N commits" batch plus one row per commit.
///
/// `commits` must be oldest→newest (provider-neutral). The Conversation rows
/// under the batch header render newest→oldest so the latest change is first.
pub(crate) fn conversation_commit_batch_items(
    commits: &[PullRequestCommit],
) -> Vec<PullRequestConversationItem> {
    let author = commits
        .first()
        .map(|commit| commit.author.clone())
        .unwrap_or_default();
    let avatar_url = commits
        .first()
        .map(|commit| commit.avatar_url.clone())
        .unwrap_or_default();
    let created_at = commits
        .iter()
        .rev()
        .map(|commit| commit.authored_at.as_str())
        .find(|value| !value.trim().is_empty())
        .unwrap_or("")
        .to_string();
    conversation_commit_batch_items_for_actor(
        commits,
        author.as_str(),
        avatar_url.as_str(),
        created_at.as_str(),
        commits.len(),
        None,
    )
}

/// Build a GitHub-style commit batch whose actor comes from provider activity.
///
/// GitLab system notes and Gitee push events identify the user who updated the
/// source branch independently from each commit's Git author/committer. The
/// declared count can exceed the included rows when a provider truncates the
/// commit references in a large push event.
pub(crate) fn conversation_commit_batch_items_for_actor(
    commits: &[PullRequestCommit],
    actor: &str,
    actor_avatar_url: &str,
    created_at: &str,
    declared_count: usize,
    event_id: Option<&str>,
) -> Vec<PullRequestConversationItem> {
    let count = declared_count.max(commits.len());
    if count == 0 {
        return Vec::new();
    }
    let author = if actor.trim().is_empty() {
        commits
            .first()
            .map(|commit| commit.author.clone())
            .unwrap_or_default()
    } else {
        actor.to_string()
    };
    let avatar_url = if actor_avatar_url.trim().is_empty() {
        commits
            .first()
            .map(|commit| commit.avatar_url.clone())
            .unwrap_or_default()
    } else {
        actor_avatar_url.to_string()
    };
    let created_at = if created_at.trim().is_empty() {
        commits
            .iter()
            .rev()
            .map(|commit| commit.authored_at.as_str())
            .find(|value| !value.trim().is_empty())
            .unwrap_or("")
            .to_string()
    } else {
        created_at.to_string()
    };
    let mut items = Vec::with_capacity(commits.len() + 1);
    items.push(PullRequestConversationItem {
        id: event_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("commits-{id}"))
            .unwrap_or_else(|| {
                format!(
                    "commits-{}",
                    commits
                        .first()
                        .map(|commit| commit.sha.as_str())
                        .unwrap_or("batch")
                )
            }),
        kind: "commits".to_string(),
        author,
        avatar_url,
        author_association: String::new(),
        body: if count == 1 {
            "added 1 commit".to_string()
        } else {
            format!("added {count} commits")
        },
        created_at,
        state: count.to_string(),
        review_id: String::new(),
        can_dismiss: false,
    });
    for commit in commits.iter().rev() {
        items.push(PullRequestConversationItem {
            id: commit.sha.clone(),
            kind: "commit".to_string(),
            author: commit.author.clone(),
            avatar_url: commit.avatar_url.clone(),
            author_association: String::new(),
            // Full message so the UI can show the title and expand the body.
            body: if commit.message.trim().is_empty() {
                commit.title.clone()
            } else {
                commit.message.clone()
            },
            created_at: commit.authored_at.clone(),
            state: short_sha(commit.sha.as_str()),
            review_id: String::new(),
            can_dismiss: false,
        });
    }
    items
}

pub(crate) fn conversation_merged_item(
    author: &str,
    merge_sha: &str,
    target_branch: &str,
    created_at: &str,
) -> PullRequestConversationItem {
    PullRequestConversationItem {
        id: format!("merged-{}", short_sha(merge_sha)),
        kind: "merged".to_string(),
        author: if author.trim().is_empty() {
            String::new()
        } else {
            author.to_string()
        },
        avatar_url: String::new(),
        author_association: String::new(),
        body: short_sha(merge_sha),
        created_at: created_at.to_string(),
        state: target_branch.to_string(),
        review_id: String::new(),
        can_dismiss: false,
    }
}

pub(crate) fn conversation_system_event_item(
    id: impl Into<String>,
    author: &str,
    body: impl Into<String>,
    created_at: &str,
) -> PullRequestConversationItem {
    PullRequestConversationItem {
        id: id.into(),
        kind: "event".to_string(),
        author: if author.trim().is_empty() {
            String::new()
        } else {
            author.to_string()
        },
        avatar_url: String::new(),
        author_association: String::new(),
        body: body.into(),
        created_at: created_at.to_string(),
        state: String::new(),
        review_id: String::new(),
        can_dismiss: false,
    }
}

/// Keep the description card first, then append activity sorted by `created_at`.
pub(crate) fn finalize_conversation_timeline(
    mut description: PullRequestConversationItem,
    activity: Vec<PullRequestConversationItem>,
) -> Vec<PullRequestConversationItem> {
    description.kind = "description".to_string();
    normalize_conversation_body(&mut description);

    // Treat a GitHub-style `commits` header and its following `commit` rows as
    // one atomic timeline block. Sorting every row independently separates the
    // "added N commits" record from commits whose authored dates differ. This
    // block normalization is shared by GitHub, GitLab, and both Gitee profiles.
    let mut blocks = Vec::<(Option<i64>, usize, Vec<PullRequestConversationItem>)>::new();
    let mut activity = activity
        .into_iter()
        .map(|mut item| {
            normalize_conversation_body(&mut item);
            item
        })
        .peekable();
    while let Some(item) = activity.next() {
        let mut sort_at = normalized_conversation_timestamp(item.created_at.as_str());
        let mut block = vec![item];
        if block[0].kind == "commits" {
            while activity.peek().is_some_and(|next| next.kind == "commit") {
                if let Some(commit) = activity.next() {
                    block.push(commit);
                }
            }
        } else if block[0].kind == "review_summary"
            && activity.peek().is_some_and(|next| next.kind == "review")
            && let Some(review) = activity.next()
        {
            if sort_at.is_none() {
                sort_at = normalized_conversation_timestamp(review.created_at.as_str());
            }
            block.push(review);
        }
        let original_index = blocks.len();
        blocks.push((sort_at, original_index, block));
    }
    blocks.sort_by(|left, right| match (left.0, right.0) {
        (Some(left_time), Some(right_time)) => {
            left_time.cmp(&right_time).then(left.1.cmp(&right.1))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.1.cmp(&right.1),
    });

    let mut items = Vec::new();
    items.push(description);
    items.extend(
        blocks
            .into_iter()
            .flat_map(|(_, _, block)| block.into_iter()),
    );
    items
}

fn normalized_conversation_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

/// Convert provider-specific Markdown/HTML bodies into the one plain-text
/// Conversation presentation shared by GitHub, GitLab, and Gitee.
///
/// The Slint timeline deliberately owns one GitHub-style card implementation
/// and does not execute provider-supplied markup. Parsing at this boundary
/// keeps visible text and paragraph/list breaks while dropping raw tags.
fn normalize_conversation_body(item: &mut PullRequestConversationItem) {
    if !matches!(item.kind.as_str(), "description" | "comment" | "review") {
        return;
    }
    item.body = conversation_body_to_plain_text(item.body.as_str());
}

pub(crate) fn conversation_body_to_plain_text(body: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut output = String::with_capacity(body.len());
    for event in Parser::new_ext(body, options) {
        match event {
            Event::Text(text) | Event::Code(text) => output.push_str(text.as_ref()),
            Event::SoftBreak | Event::HardBreak => push_conversation_line_break(&mut output),
            Event::Start(Tag::Item) => {
                push_conversation_line_break(&mut output);
                output.push_str("• ");
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::TableRow,
            )
            | Event::Rule => push_conversation_line_break(&mut output),
            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                append_html_fragment_text(&mut output, html.as_ref());
            }
            _ => {}
        }
    }

    decode_html_entities(output.trim())
}

fn push_conversation_line_break(output: &mut String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}

fn append_html_fragment_text(output: &mut String, html: &str) {
    let mut tag = String::new();
    let mut inside_tag = false;
    for character in html.chars() {
        match character {
            '<' if !inside_tag => {
                inside_tag = true;
                tag.clear();
            }
            '>' if inside_tag => {
                inside_tag = false;
                let tag_name = tag
                    .trim()
                    .trim_start_matches('/')
                    .split_ascii_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches('/')
                    .to_ascii_lowercase();
                match tag_name.as_str() {
                    "li" if !tag.trim_start().starts_with('/') => {
                        push_conversation_line_break(output);
                        output.push_str("• ");
                    }
                    "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        push_conversation_line_break(output)
                    }
                    _ => {}
                }
            }
            _ if inside_tag => tag.push(character),
            _ => output.push(character),
        }
    }
}

pub(crate) fn decode_html_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(entity_start) = remaining.find('&') {
        decoded.push_str(&remaining[..entity_start]);
        remaining = &remaining[entity_start..];
        let Some(entity_end) = remaining.find(';').filter(|end| *end <= 12) else {
            decoded.push('&');
            remaining = &remaining[1..];
            continue;
        };
        let entity = &remaining[1..entity_end];
        let replacement = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => {
                u32::from_str_radix(&numeric[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            numeric if numeric.starts_with('#') => {
                numeric[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            decoded.push(replacement);
        } else {
            decoded.push_str(&remaining[..=entity_end]);
        }
        remaining = &remaining[entity_end + 1..];
    }
    decoded.push_str(remaining);
    decoded
}

/// True when a provider system note is already covered by structured commit rows.
pub(crate) fn is_redundant_commit_system_note(body: &str) -> bool {
    let normalized = body.trim().to_ascii_lowercase();
    normalized.starts_with("added ") && normalized.contains("commit")
}

/// True when a provider system note is already covered by a structured merge row.
pub(crate) fn is_redundant_merge_system_note(body: &str) -> bool {
    let normalized = body.trim().to_ascii_lowercase();
    normalized == "merged"
        || normalized.starts_with("merged commit")
        || normalized.starts_with("merged this")
        || normalized.starts_with("accepted this")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestCommit {
    pub(crate) sha: String,
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) author: String,
    pub(crate) avatar_url: String,
    pub(crate) authored_at: String,
    pub(crate) committer: String,
    pub(crate) committer_avatar_url: String,
    pub(crate) committed_at: String,
    pub(crate) verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestReviewAction {
    Comment,
    Approve,
    RequestChanges,
}

impl PullRequestReviewAction {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "comment" => Some(Self::Comment),
            "approve" => Some(Self::Approve),
            "request_changes" | "request-changes" => Some(Self::RequestChanges),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestMergeMethod {
    Merge,
    Squash,
    Rebase,
    FastForward,
}

impl PullRequestMergeMethod {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "merge" => Some(Self::Merge),
            "squash" => Some(Self::Squash),
            "rebase" => Some(Self::Rebase),
            "fast_forward" | "fast-forward" => Some(Self::FastForward),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::FastForward => "fast_forward",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PullRequestReadinessKind {
    Checking,
    Ready,
    Conflicts,
    Draft,
    ReviewRequired,
    ChangesRequested,
    ChecksPending,
    ChecksFailed,
    BranchOutOfDate,
    DiscussionsUnresolved,
    Blocked,
    Merged,
    Closed,
    Unavailable,
}

impl PullRequestReadinessKind {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Ready => "ready",
            Self::Conflicts => "conflicts",
            Self::Draft => "draft",
            Self::ReviewRequired => "review_required",
            Self::ChangesRequested => "changes_requested",
            Self::ChecksPending => "checks_pending",
            Self::ChecksFailed => "checks_failed",
            Self::BranchOutOfDate => "branch_out_of_date",
            Self::DiscussionsUnresolved => "discussions_unresolved",
            Self::Blocked => "blocked",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Unavailable => "unavailable",
        }
    }

    pub(crate) const fn label_key(&self) -> &'static str {
        match self {
            Self::Checking => "pr_review.readiness.checking",
            Self::Ready => "pr_review.readiness.ready",
            Self::Conflicts => "pr_review.readiness.conflicts",
            Self::Draft => "pr_review.readiness.draft",
            Self::ReviewRequired => "pr_review.readiness.review_required",
            Self::ChangesRequested => "pr_review.readiness.changes_requested",
            Self::ChecksPending => "pr_review.readiness.checks_pending",
            Self::ChecksFailed => "pr_review.readiness.checks_failed",
            Self::BranchOutOfDate => "pr_review.readiness.branch_out_of_date",
            Self::DiscussionsUnresolved => "pr_review.readiness.discussions_unresolved",
            Self::Blocked => "pr_review.readiness.blocked",
            Self::Merged => "pr_review.readiness.merged",
            Self::Closed => "pr_review.readiness.closed",
            Self::Unavailable => "pr_review.readiness.unavailable",
        }
    }

    pub(crate) const fn detail_key(&self) -> &'static str {
        match self {
            Self::Checking => "pr_review.readiness_detail.checking",
            Self::Ready => "pr_review.readiness_detail.ready",
            Self::Conflicts => "pr_review.readiness_detail.conflicts",
            Self::Draft => "pr_review.readiness_detail.draft",
            Self::ReviewRequired => "pr_review.readiness_detail.review_required",
            Self::ChangesRequested => "pr_review.readiness_detail.changes_requested",
            Self::ChecksPending => "pr_review.readiness_detail.checks_pending",
            Self::ChecksFailed => "pr_review.readiness_detail.checks_failed",
            Self::BranchOutOfDate => "pr_review.readiness_detail.branch_out_of_date",
            Self::DiscussionsUnresolved => "pr_review.readiness_detail.discussions_unresolved",
            Self::Blocked => "pr_review.readiness_detail.blocked",
            Self::Merged => "pr_review.readiness_detail.merged",
            Self::Closed => "pr_review.readiness_detail.closed",
            Self::Unavailable => "pr_review.readiness_detail.unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestMergeBlockKind {
    None,
    ConfigureProvider,
    Readiness,
    Permission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestRevertStrategy {
    PullRequest,
    DirectCommit,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestReadiness {
    pub(crate) kind: PullRequestReadinessKind,
    pub(crate) detail: String,
}

impl PullRequestReadiness {
    pub(crate) fn new(kind: PullRequestReadinessKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestActionState {
    pub(crate) can_comment: bool,
    pub(crate) can_approve: bool,
    pub(crate) can_request_changes: bool,
    pub(crate) can_merge: bool,
    pub(crate) can_revert: bool,
    pub(crate) review_state: String,
    pub(crate) readiness: PullRequestReadiness,
    pub(crate) merge_block_kind: PullRequestMergeBlockKind,
    pub(crate) revert_strategy: PullRequestRevertStrategy,
    pub(crate) allowed_merge_methods: Vec<PullRequestMergeMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestRevertResult {
    pub(crate) number: String,
    pub(crate) title: String,
    pub(crate) web_url: String,
    pub(crate) message: String,
}

/// Provider-neutral status filter. `None` means the caller intentionally
/// requests the provider default and no state query parameter is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestState {
    Open,
    Closed,
    ClosedUnmerged,
    Merged,
    All,
}

impl PullRequestState {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open" | "opened" => Some(Self::Open),
            "closed" => Some(Self::Closed),
            "closed-unmerged" => Some(Self::ClosedUnmerged),
            "merged" => Some(Self::Merged),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::ClosedUnmerged => "closed-unmerged",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn gitee_query(self) -> &'static str {
        match self {
            Self::Open => "opened",
            Self::Closed | Self::ClosedUnmerged => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(super) const fn gitee_public_query(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed | Self::ClosedUnmerged | Self::Merged => "closed",
            Self::All => "all",
        }
    }

    pub(super) const fn gitlab_query(self) -> &'static str {
        match self {
            Self::Open => "opened",
            Self::Closed | Self::ClosedUnmerged => "closed",
            Self::Merged => "merged",
            Self::All => "all",
        }
    }

    pub(crate) const fn github_query(self) -> &'static str {
        match self {
            Self::Open => "is:open",
            Self::Closed => "is:closed",
            Self::ClosedUnmerged => "is:closed is:unmerged",
            Self::Merged => "is:merged",
            Self::All => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PageCollector, PullRequestMergeMethod, PullRequestReviewAction, PullRequestState,
        split_commit_message,
    };

    #[test]
    fn pr_author_identity_preserves_only_explicit_profile_fields() {
        let identity = super::PullRequestAuthorIdentity::from_user(&serde_json::json!({
            "name": " Example User ", "email": null, "public_email": "user@example.com",
            "login": "example-user"
        }));
        assert_eq!(identity.name.as_deref(), Some("Example User"));
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
        for user in [
            serde_json::Value::Null,
            serde_json::json!({
                "login": "example-user", "email": " ", "name": 42,
                "commit": {"author": {"email": "commit@example.com"}}
            }),
        ] {
            assert_eq!(
                super::PullRequestAuthorIdentity::from_user(&user),
                Default::default()
            );
        }
    }

    #[test]
    fn page_collector_distinguishes_short_completion_from_a_full_hard_limit() {
        let mut complete = PageCollector::new(2, 3);
        assert!(complete.append(vec![1, 2]));
        assert!(!complete.append(vec![3]));
        assert_eq!(complete.into_parts(), (vec![1, 2, 3], false));

        let mut truncated = PageCollector::new(2, 2);
        assert!(truncated.append(vec![1, 2]));
        assert!(!truncated.append(vec![3, 4]));
        assert_eq!(truncated.into_parts(), (vec![1, 2, 3, 4], true));
    }

    #[test]
    fn command_models_accept_only_documented_portable_values() {
        assert_eq!(
            PullRequestReviewAction::parse("request-changes"),
            Some(PullRequestReviewAction::RequestChanges)
        );
        assert_eq!(
            PullRequestMergeMethod::parse("fast_forward"),
            Some(PullRequestMergeMethod::FastForward)
        );
        assert_eq!(
            PullRequestState::parse("opened"),
            Some(PullRequestState::Open)
        );
        assert!(PullRequestReviewAction::parse("merge").is_none());
        assert!(PullRequestMergeMethod::parse("approve").is_none());
        assert!(PullRequestState::parse("draft").is_none());
    }

    #[test]
    fn commit_message_model_separates_trimmed_title_and_body() {
        assert_eq!(
            split_commit_message("  Add cache guard  \n\nDetails stay intact.\n"),
            (
                "Add cache guard".to_string(),
                "Details stay intact.".to_string()
            )
        );
        assert_eq!(split_commit_message(""), (String::new(), String::new()));
    }
}
