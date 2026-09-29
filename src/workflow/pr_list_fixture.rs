use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Deserialize;

use crate::git::{PullRequestFilters, PullRequestItem, PullRequestReviewStatus, PullRequestState};

const REVIEW_STATUS_FIXTURE_JSON: &str =
    include_str!("../../tests/fixtures/pr-list-review-status.json");

pub(super) fn setup(ui: &crate::MainWindow) {
    use slint::ComponentHandle;

    if !super::pr_list::review_status_fixture_enabled()
        || std::env::var("USHER_PR_LIST_AUTOSTART").as_deref() != Ok("1")
        || !crate::app_paths::benchmark_offline_mode()
        || std::env::var_os("USHER_BENCHMARK_STATE_ROOT").is_none()
    {
        return;
    }
    let weak = ui.as_weak();
    crate::startup::run_after_first_frame(move || {
        slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return };
            let dimension = |key, default, min, max| {
                std::env::var(key)
                    .ok()
                    .and_then(|value| value.parse::<f32>().ok())
                    .filter(|value| value.is_finite())
                    .unwrap_or(default)
                    .clamp(min, max)
            };
            ui.window().set_size(slint::LogicalSize::new(
                dimension("USHER_PR_LIST_WIDTH", 1200.0, 800.0, 1800.0),
                dimension("USHER_PR_LIST_HEIGHT", 900.0, 600.0, 1200.0),
            ));
            if let Ok(locale) = std::env::var("USHER_PR_LIST_LOCALE") {
                if let Ok(canonical) = crate::i18n::apply_preference(&locale) {
                    crate::i18n::sync_main_window(&ui, canonical);
                }
            }
            let provider = std::env::var("USHER_PR_LIST_PROVIDER")
                .ok()
                .filter(|value| matches!(value.as_str(), "github" | "gitlab" | "gitee"))
                .unwrap_or_else(|| "github".into());
            ui.set_workflow_project_name("PR layout fixtures".into());
            ui.set_workflow_pr_repo_project_id("review-status-v1".into());
            ui.set_workflow_pr_repo_type(provider.into());
            ui.set_workflow_selected_flow(2);
            ui.invoke_navigate(crate::Route::Workflow);
            ui.invoke_workflow_pr_list_state_change("open".into());
        });
    });
}

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
    #[serde(default)]
    conflict_status: String,
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
        .filter(|item| matches_query(item, filters.query.as_str()))
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

impl FixturePullRequestItem {
    fn into_pull_request_item(self, now: DateTime<Utc>) -> Result<PullRequestItem, String> {
        let review_status = match self.review_status.as_str() {
            "" | "none" => PullRequestReviewStatus::None,
            "review_required" => PullRequestReviewStatus::ReviewRequired,
            "approved" => PullRequestReviewStatus::Approved,
            "changes_requested" => PullRequestReviewStatus::ChangesRequested,
            value => return Err(format!("Unknown fixture review status: {value}")),
        };
        let conflict_status = if self.state != "open" {
            crate::git::conflicts::ConflictStatus::NotApplicable
        } else {
            match self.conflict_status.as_str() {
                "conflicting" => crate::git::conflicts::ConflictStatus::Conflicting,
                "clear" => crate::git::conflicts::ConflictStatus::Clear,
                "checking" => crate::git::conflicts::ConflictStatus::Checking,
                _ => crate::git::conflicts::ConflictStatus::Unknown,
            }
        };
        Ok(PullRequestItem {
            number: self.number,
            title: self.title,
            state: self.state,
            author: self.author,
            source_branch: self.source_branch,
            target_branch: self.target_branch,
            review_status,
            conflict_status,
            conversation_count: self.conversation_count,
            created_at: (now - Duration::hours(self.updated_hours_ago + 1))
                .to_rfc3339_opts(SecondsFormat::Secs, true),
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
        PullRequestState::ClosedUnmerged => item.state == "closed",
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

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::filtered_review_status_fixture;
    use crate::git::{PullRequestFilters, PullRequestState};

    #[test]
    fn filters_mock_data_by_merged_state_and_query_terms() {
        let filters = PullRequestFilters {
            query: "provider parity".to_string(),
            reviews: "all".to_string(),
            assignee: "any".to_string(),
            sort: "newest".to_string(),
        };
        let items =
            filtered_review_status_fixture(PullRequestState::Merged, &filters, Utc::now()).unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].number, "99015");
    }

    #[test]
    fn mock_data_exercises_open_pagination_and_closed_union() {
        let filters = PullRequestFilters::default();
        let open =
            filtered_review_status_fixture(PullRequestState::Open, &filters, Utc::now()).unwrap();
        let closed =
            filtered_review_status_fixture(PullRequestState::Closed, &filters, Utc::now()).unwrap();
        let merged =
            filtered_review_status_fixture(PullRequestState::Merged, &filters, Utc::now()).unwrap();
        let closed_unmerged =
            filtered_review_status_fixture(PullRequestState::ClosedUnmerged, &filters, Utc::now())
                .unwrap();

        assert!(open.len() > 10);
        assert!(closed.len() > merged.len());
        assert!(merged.len() >= 3);
        assert_eq!(closed.len(), merged.len() + closed_unmerged.len());
    }
}
