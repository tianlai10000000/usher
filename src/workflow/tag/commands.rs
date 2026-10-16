//! Pure Git command and Job step planning for Tag workflows.

use crate::i18n::LocalizedText;
use crate::jobs::{JobStepData, JobStepDefinition};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedCommand {
    pub(super) name: LocalizedText,
    pub(super) argv: Vec<String>,
    pub(super) detail: LocalizedText,
}

impl PlannedCommand {
    fn queued_step(&self) -> JobStepData {
        JobStepData::queued(
            JobStepDefinition::new(self.name.clone(), self.argv.clone()),
            self.detail.clone(),
        )
    }
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
            name: LocalizedText::message("tag.step.pull_main"),
            argv: git(repo, &["pull", "origin", "main"]),
            detail: LocalizedText::message("tag.step.pull_main_detail"),
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

pub(super) fn remove_tag_plan(repo: &str, tag: &str) -> Vec<PlannedCommand> {
    vec![
        PlannedCommand {
            name: LocalizedText::message("tag.step.enter_repository"),
            argv: git(repo, &["rev-parse", "--show-toplevel"]),
            detail: LocalizedText::message("tag.step.enter_repository_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.delete_local"),
            argv: git(repo, &["tag", "-d", tag]),
            detail: LocalizedText::message("tag.step.delete_local_detail"),
        },
        PlannedCommand {
            name: LocalizedText::message("tag.step.delete_remote"),
            argv: git(repo, &["push", "--delete", "origin", tag]),
            detail: LocalizedText::message("tag.step.delete_remote_detail"),
        },
    ]
}

pub(super) fn create_tag_steps(plan: &[PlannedCommand]) -> Vec<JobStepData> {
    let mut steps = plan
        .iter()
        .map(PlannedCommand::queued_step)
        .collect::<Vec<_>>();
    steps.push(JobStepData::queued(
        JobStepDefinition::new(
            LocalizedText::message("tag.step.complete"),
            vec!["complete".into()],
        ),
        LocalizedText::message("tag.step.complete_detail"),
    ));
    steps
}

pub(super) fn remove_tag_steps(plan: &[PlannedCommand]) -> Vec<JobStepData> {
    let mut steps = plan
        .iter()
        .map(PlannedCommand::queued_step)
        .collect::<Vec<_>>();
    steps.push(JobStepData::queued(
        JobStepDefinition::new(
            LocalizedText::message("tag.step.refresh_remote"),
            vec!["git".into(), "fetch".into(), "--tags".into()],
        ),
        LocalizedText::message("tag.step.refresh_remote_detail"),
    ));
    steps.push(JobStepData::queued(
        JobStepDefinition::new(
            LocalizedText::message("tag.step.complete"),
            vec!["complete".into()],
        ),
        LocalizedText::message("tag.step.remove_complete_waiting"),
    ));
    steps
}

pub(crate) fn default_create_tag_steps() -> Vec<JobStepData> {
    create_tag_steps(&create_tag_plan(
        "<repository>",
        "<version>",
        "<description>",
    ))
}

pub(super) fn default_remove_tag_steps() -> Vec<JobStepData> {
    remove_tag_steps(&remove_tag_plan("<repository>", "<tag>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_plan_keeps_repository_and_user_values_as_distinct_arguments() {
        let plan = create_tag_plan("/tmp/repo with spaces", "v1.2.3", "release notes");
        assert_eq!(plan.len(), 9);
        assert_eq!(
            plan[7].argv,
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
    fn create_plan_pulls_main_after_checkout_before_rebase_and_publication() {
        let plan = create_tag_plan("/repo", "v1.2.3", "notes");
        let commands = plan
            .iter()
            .map(|command| command.argv[3..].join(" "))
            .collect::<Vec<_>>();
        assert_eq!(
            commands,
            [
                "rev-parse --show-toplevel",
                "checkout release",
                "pull origin release",
                "checkout main",
                "pull origin main",
                "rebase release",
                "push origin main",
                "tag -a v1.2.3 -m notes",
                "push origin v1.2.3",
            ]
        );
    }

    #[test]
    fn pull_main_step_reprojects_in_the_current_locale() {
        use crate::jobs::{JobRunDescriptor, JobSnapshot, apply_job_run};
        use slint::Model;

        let _locale_guard = crate::i18n::test_locale_guard();
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::MainWindow::new().unwrap();
        let snapshot = JobSnapshot::queued(default_create_tag_steps());
        let descriptor = JobRunDescriptor {
            source: "tag-pull-main-locale-test".into(),
            ..Default::default()
        };
        assert_eq!(
            snapshot.steps[4].definition.name,
            LocalizedText::message("tag.step.pull_main")
        );
        let mut rows = None;
        for (locale, name, detail) in [
            ("en", "Pull main", "Pull the latest main Branch changes."),
            ("zh-CN", "拉取 main", "拉取 main Branch 的最新变更。"),
        ] {
            crate::i18n::apply_preference(locale).unwrap();
            apply_job_run(&ui, &descriptor, &snapshot, &LocalizedText::empty(), false);
            let projected_rows = ui.get_job_run_steps();
            if let Some(previous) = rows.as_ref() {
                assert!(previous == &projected_rows);
            }
            let projected = projected_rows.row_data(4).unwrap();
            assert_eq!(projected.name.as_str(), name);
            assert_eq!(projected.detail.as_str(), detail);
            rows = Some(projected_rows);
        }
        crate::i18n::apply_preference(usher_i18n::fallback_locale().id).unwrap();
    }

    #[test]
    fn remove_plan_deletes_local_then_remote_tag() {
        let plan = remove_tag_plan("/repo", "v2.0.0");
        assert_eq!(plan[1].argv, ["git", "-C", "/repo", "tag", "-d", "v2.0.0"]);
        assert_eq!(
            plan[2].argv,
            ["git", "-C", "/repo", "push", "--delete", "origin", "v2.0.0"]
        );
    }

    #[test]
    fn create_steps_preserve_each_planned_command_before_completion() {
        let plan = create_tag_plan("/repo", "v1.2.3", "notes");
        let steps = create_tag_steps(&plan);

        assert_eq!(steps.len(), plan.len() + 1);
        for (step, command) in steps.iter().zip(&plan) {
            assert_eq!(step.definition.name, command.name);
            assert_eq!(step.definition.command, command.argv);
            assert_eq!(step.detail, command.detail);
        }
        assert_eq!(
            steps.last().unwrap().definition.name,
            LocalizedText::message("tag.step.complete")
        );
    }

    #[test]
    fn remove_steps_append_refresh_and_completion_after_the_plan() {
        let plan = remove_tag_plan("/repo", "v2.0.0");
        let steps = remove_tag_steps(&plan);

        assert_eq!(steps.len(), plan.len() + 2);
        for (step, command) in steps.iter().zip(&plan) {
            assert_eq!(step.definition.name, command.name);
            assert_eq!(step.definition.command, command.argv);
            assert_eq!(step.detail, command.detail);
        }
        assert_eq!(
            steps[plan.len()].definition.name,
            LocalizedText::message("tag.step.refresh_remote")
        );
        assert_eq!(
            steps[plan.len() + 1].definition.name,
            LocalizedText::message("tag.step.complete")
        );
    }
}
