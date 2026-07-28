//! Project-card pull-request overview orchestration.

use super::model::ProjectConfig;

#[derive(Debug, Clone)]
pub(super) struct ProjectPrOverviewTask {
    pub(super) project_alias: String,
    pub(super) remote_repo: String,
    pub(super) repo_project_id: String,
    pub(super) repo_type: String,
}

pub(super) fn collect_pr_overview_tasks(
    items: &mut [ProjectConfig],
    query: &str,
) -> Vec<ProjectPrOverviewTask> {
    let needle = query.trim().to_lowercase();
    let mut tasks = Vec::new();

    for item in items
        .iter_mut()
        .filter(|item| matches_project_query(item, &needle))
    {
        if !item.is_pr_auto_update || item.pr_overview.loading || item.pr_overview.failed {
            continue;
        }
        if !item.pr_overview.fat_display.is_empty()
            && !item.pr_overview.uat_display.is_empty()
            && !item.pr_overview.prod_display.is_empty()
        {
            continue;
        }

        item.pr_overview.loading = true;
        item.pr_overview.failed = false;
        item.pr_overview.fat_display = "-".to_string();
        item.pr_overview.uat_display = "-".to_string();
        item.pr_overview.prod_display = "-".to_string();
        tasks.push(ProjectPrOverviewTask {
            project_alias: item.project_alias.clone(),
            remote_repo: item.remote_repo.clone(),
            repo_project_id: item.repo_project_id.clone(),
            repo_type: item.repo_type.clone(),
        });
    }

    tasks
}

fn matches_project_query(item: &ProjectConfig, needle: &str) -> bool {
    needle.is_empty()
        || item.project_name.to_lowercase().contains(needle)
        || item.project_alias.to_lowercase().contains(needle)
}
