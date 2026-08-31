//! Provider-neutral pull-request overview counting.

use std::future::Future;

use super::{PullRequestItem, PullRequestPage};

const OVERVIEW_PAGE_SIZE: u32 = 100;
const OVERVIEW_MAX_PAGES: u32 = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EnvironmentPullRequestCounts {
    pub(crate) fat: u32,
    pub(crate) uat: u32,
    pub(crate) prod: u32,
}

pub(super) async fn count_standard_environment_pages<LoadPage, LoadFuture>(
    mut load_page: LoadPage,
) -> Result<EnvironmentPullRequestCounts, String>
where
    LoadPage: FnMut(u32, u32) -> LoadFuture,
    LoadFuture: Future<Output = Result<PullRequestPage, String>>,
{
    let mut counts = EnvironmentPullRequestCounts::default();
    let mut page_number = 1u32;

    loop {
        let page = load_page(page_number, OVERVIEW_PAGE_SIZE).await?;
        let item_count = page.items.len();
        add_items(&mut counts, &page.items);

        if is_last_page(page_number, page.page_count, item_count)
            || page_number >= OVERVIEW_MAX_PAGES
        {
            break;
        }
        page_number = page_number.saturating_add(1);
    }

    Ok(counts)
}

fn is_last_page(page_number: u32, page_count: Option<u32>, item_count: usize) -> bool {
    page_count.is_some_and(|page_count| page_number >= page_count)
        || item_count < OVERVIEW_PAGE_SIZE as usize
}

fn add_items(counts: &mut EnvironmentPullRequestCounts, items: &[PullRequestItem]) {
    for item in items {
        match standard_environment(item.target_branch.as_str()) {
            Some("fat") => counts.fat = counts.fat.saturating_add(1),
            Some("uat") => counts.uat = counts.uat.saturating_add(1),
            Some("prod") => counts.prod = counts.prod.saturating_add(1),
            _ => {}
        }
    }
}

fn standard_environment(target_branch: &str) -> Option<&'static str> {
    let branch = target_branch
        .trim()
        .rsplit_once(':')
        .map(|(_, branch)| branch)
        .unwrap_or(target_branch.trim());
    match branch {
        "fat" => Some("fat"),
        "uat" => Some("uat"),
        "prod" => Some("prod"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(target_branch: &str) -> PullRequestItem {
        PullRequestItem {
            number: String::new(),
            title: String::new(),
            state: "open".to_string(),
            author: String::new(),
            source_branch: String::new(),
            target_branch: target_branch.to_string(),
            review_status: Default::default(),
            conversation_count: None,
            created_at: String::new(),
            updated_at: String::new(),
            web_url: String::new(),
        }
    }

    #[test]
    fn counts_only_standard_target_branches() {
        let mut counts = EnvironmentPullRequestCounts::default();
        add_items(
            &mut counts,
            &[
                item("fat"),
                item("owner:fat"),
                item("uat"),
                item("prod"),
                item("main"),
            ],
        );

        assert_eq!(
            counts,
            EnvironmentPullRequestCounts {
                fat: 2,
                uat: 1,
                prod: 1,
            }
        );
    }

    #[test]
    fn does_not_fold_branch_case_or_suffixes() {
        let mut counts = EnvironmentPullRequestCounts::default();
        add_items(
            &mut counts,
            &[item("FAT"), item("feature/fat"), item("production")],
        );

        assert_eq!(counts, EnvironmentPullRequestCounts::default());
    }

    #[test]
    fn stops_on_provider_page_count_or_short_page() {
        assert!(is_last_page(2, Some(2), OVERVIEW_PAGE_SIZE as usize));
        assert!(is_last_page(1, None, 99));
        assert!(!is_last_page(1, Some(2), OVERVIEW_PAGE_SIZE as usize));
        assert!(!is_last_page(1, None, OVERVIEW_PAGE_SIZE as usize));
    }

    #[tokio::test]
    async fn overview_scan_stops_at_the_hard_page_limit() {
        let mut calls = 0u32;
        let counts = count_standard_environment_pages(|_, _| {
            calls += 1;
            std::future::ready(Ok(PullRequestPage {
                items: (0..OVERVIEW_PAGE_SIZE).map(|_| item("fat")).collect(),
                total: 0,
                total_exact: false,
                page_count: None,
            }))
        })
        .await
        .unwrap();

        assert_eq!(calls, OVERVIEW_MAX_PAGES);
        assert_eq!(counts.fat, OVERVIEW_PAGE_SIZE * OVERVIEW_MAX_PAGES);
    }
}
