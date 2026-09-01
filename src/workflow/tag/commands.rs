//! Pure Git command planning for Tag workflows.

use crate::i18n::LocalizedText;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedCommand {
    pub(super) name: LocalizedText,
    pub(super) argv: Vec<String>,
    pub(super) detail: LocalizedText,
}

fn git(repo: &str, args: &[&str]) -> Vec<String> {
    let mut argv = vec!["git".to_string(), "-C".to_string(), repo.to_string()];
    argv.extend(args.iter().map(|value| (*value).to_string()));
    argv
}

pub(super) fn create_tag_plan(repo: &str, version: &str, description: &str) -> Vec<PlannedCommand> {
    vec![
        PlannedCommand {
            name: LocalizedText::message("tag.step.enter_repository"),
            argv: git(repo, &["rev-parse", "--show-toplevel"]),
            detail: LocalizedText::message_with(
                "tag.step.using_repository",
                [("repository", repo.to_string())],
            ),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.checkout_release"),
            argv: git(repo, &["checkout", "release"]),
            detail: LocalizedText::message("tag.step.checkout_release_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.pull_release"),
            argv: git(repo, &["pull", "origin", "release"]),
            detail: LocalizedText::message("tag.step.pull_release_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.checkout_main"),
            argv: git(repo, &["checkout", "main"]),
            detail: LocalizedText::message("tag.step.checkout_main_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.rebase_main"),
            argv: git(repo, &["rebase", "release"]),
            detail: LocalizedText::message("tag.step.rebase_main_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.push_main"),
            argv: git(repo, &["push", "origin", "main"]),
            detail: LocalizedText::message("tag.step.push_main_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.create_annotated"),
            argv: git(repo, &["tag", "-a", version, "-m", description]),
            detail: LocalizedText::message_with(
                "tag.step.create_annotated_version_detail",
                [("version", version.to_string())],
            ),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.push_tag"),
            argv: git(repo, &["push", "origin", version]),
            detail: LocalizedText::message_with(
                "tag.step.push_tag_version_detail",
                [("version", version.to_string())],
            ),
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
