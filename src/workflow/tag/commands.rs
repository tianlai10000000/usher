//! Pure Git command planning for Tag workflows.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedCommand {
    pub(super) name: &'static str,
    pub(super) argv: Vec<String>,
    pub(super) detail: String,
}

fn git(repo: &str, args: &[&str]) -> Vec<String> {
    let mut argv = vec!["git".to_string(), "-C".to_string(), repo.to_string()];
    argv.extend(args.iter().map(|value| (*value).to_string()));
    argv
}

pub(super) fn create_tag_plan(repo: &str, version: &str, description: &str) -> Vec<PlannedCommand> {
    vec![
        PlannedCommand {
            name: "Enter local repository",
            argv: git(repo, &["rev-parse", "--show-toplevel"]),
            detail: format!("Using local repository at '{repo}'."),
        },
        PlannedCommand {
            name: "Checkout release",
            argv: git(repo, &["checkout", "release"]),
            detail: "Switch to release branch.".to_string(),
        },
        PlannedCommand {
            name: "Pull release",
            argv: git(repo, &["pull", "origin", "release"]),
            detail: "Update local release branch to latest.".to_string(),
        },
        PlannedCommand {
            name: "Checkout main",
            argv: git(repo, &["checkout", "main"]),
            detail: "Switch to main branch.".to_string(),
        },
        PlannedCommand {
            name: "Rebase main on release",
            argv: git(repo, &["rebase", "release"]),
            detail: "Rebase main onto release.".to_string(),
        },
        PlannedCommand {
            name: "Push main",
            argv: git(repo, &["push", "origin", "main"]),
            detail: "Push main branch to remote.".to_string(),
        },
        PlannedCommand {
            name: "Create annotated tag",
            argv: git(repo, &["tag", "-a", version, "-m", description]),
            detail: format!("Create annotated tag '{version}'."),
        },
        PlannedCommand {
            name: "Push tag",
            argv: git(repo, &["push", "origin", version]),
            detail: format!("Push tag '{version}' to remote."),
        },
    ]
}

pub(super) fn remove_tag_plan(repo: &str, tag: &str) -> Vec<Vec<String>> {
    vec![
        git(repo, &["rev-parse", "--show-toplevel"]),
        git(repo, &["tag", "-d", tag]),
        git(repo, &["push", "--delete", "origin", tag]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_plan_keeps_repository_and_user_values_as_distinct_arguments() {
        let plan = create_tag_plan("/tmp/repo with spaces", "v1.2.3", "release notes");
        assert_eq!(plan.len(), 8);
        assert_eq!(
            plan[6].argv,
            [
                "git",
                "-C",
                "/tmp/repo with spaces",
                "tag",
                "-a",
                "v1.2.3",
                "-m",
                "release notes"
            ]
        );
    }

    #[test]
    fn remove_plan_deletes_local_then_remote_tag() {
        let plan = remove_tag_plan("/repo", "v2.0.0");
        assert_eq!(plan[1], ["git", "-C", "/repo", "tag", "-d", "v2.0.0"]);
        assert_eq!(
            plan[2],
            ["git", "-C", "/repo", "push", "--delete", "origin", "v2.0.0"]
        );
    }
}
