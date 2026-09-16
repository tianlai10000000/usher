use std::collections::BTreeSet;

use serde_json::Value;

#[test]
fn review_status_fixture_covers_status_and_responsive_cases() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/pr-list-review-status.json"))
        .expect("review-status fixture must be valid JSON");
    let items = fixture["items"]
        .as_array()
        .expect("review-status fixture must contain an items array");
    let statuses = items
        .iter()
        .filter_map(|item| item["review_status"].as_str())
        .collect::<BTreeSet<_>>();

    assert_eq!(items.len(), 36);
    let numbers = items
        .iter()
        .map(|item| item["number"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(numbers.len(), items.len());

    assert_eq!(
        statuses,
        BTreeSet::from(["approved", "changes_requested", "none", "review_required",])
    );
    assert!(items.iter().any(|item| {
        item["title"]
            .as_str()
            .is_some_and(|title| title.len() > 180)
    }));
    assert!(items.iter().any(|item| {
        item["source_branch"]
            .as_str()
            .is_some_and(|branch| branch.len() > 50)
    }));
}
