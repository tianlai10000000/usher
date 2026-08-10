use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Deserialize;

use crate::git::{PullRequestFilters, PullRequestItem, PullRequestReviewStatus, PullRequestState};

const REVIEW_STATUS_FIXTURE_JSON: &str =
    include_str!("../../tests/fixtures/pr-list-review-status.json");

#[derive(Deserialize)]
struct PullRequestListFixture {
    items: Vec<FixturePullRequestItem>,
}

#[derive(Deserialize)]
struct FixturePullRequestItem {
    number: String,
    title: String,
    state: String,
    author: String,
    source_branch: String,
    target_branch: String,
    review_status: String,
    conversation_count: Option<u32>,
    updated_hours_ago: i64,
}

pub(super) fn filtered_review_status_fixture(
    state: PullRequestState,
    filters: &PullRequestFilters,
    now: DateTime<Utc>,
) -> Result<Vec<PullRequestItem>, String> {
    let fixture: PullRequestListFixture = serde_json::from_str(REVIEW_STATUS_FIXTURE_JSON)
        .map_err(|error| format!("Invalid PR list review-status fixture: {error}"))?;
    let mut items = fixture
        .items
        .into_iter()
        .map(|item| item.into_pull_request_item(now))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|item| matches_state(item, state))
        .filter(|item| matches_review(item, filters.reviews.as_str()))
        .collect::<Vec<_>>();
    match filters.sort.as_str() {
        "oldest" | "updated-asc" => items.sort_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then_with(|| left.number.cmp(&right.number))
        }),
        _ => items.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.number.cmp(&left.number))
        }),
    }
    Ok(items)
}

impl FixturePullRequestItem {
    fn into_pull_request_item(self, now: DateTime<Utc>) -> Result<PullRequestItem, String> {
        let review_status = match self.review_status.as_str() {
            "" | "none" => PullRequestReviewStatus::None,
            "review_required" => PullRequestReviewStatus::ReviewRequired,
            "approved" => PullRequestReviewStatus::Approved,
            "changes_requested" => PullRequestReviewStatus::ChangesRequested,
            value => return Err(format!("Unknown fixture review status: {value}")),
        };
        Ok(PullRequestItem {
            number: self.number,
            title: self.title,
            state: self.state,
            author: self.author,
            source_branch: self.source_branch,
            target_branch: self.target_branch,
            review_status,
            conversation_count: self.conversation_count,
            updated_at: (now - Duration::hours(self.updated_hours_ago))
                .to_rfc3339_opts(SecondsFormat::Secs, true),
            web_url: String::new(),
        })
    }
}

fn matches_state(item: &PullRequestItem, state: PullRequestState) -> bool {
    match state {
        PullRequestState::Open => item.state == "open",
        PullRequestState::Closed => matches!(item.state.as_str(), "closed" | "merged"),
        PullRequestState::Merged => item.state == "merged",
        PullRequestState::All => true,
    }
}

fn matches_review(item: &PullRequestItem, review_filter: &str) -> bool {
    match review_filter {
        "required" => item.review_status == PullRequestReviewStatus::ReviewRequired,
        "approved" => item.review_status == PullRequestReviewStatus::Approved,
        "changes-requested" => item.review_status == PullRequestReviewStatus::ChangesRequested,
        _ => true,
    }
}
