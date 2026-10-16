use super::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CHILD: &str = "workflow::tag::execution::tests::isolated_real_git_create_fixture";
const VERSION: &str = "v1.2.3";

fn isolated_case(case: &str) {
    let fixture = tempfile::tempdir().unwrap();
    let global = fixture.path().join("empty-gitconfig");
    std::fs::write(&global, b"").unwrap();
    let no_hooks = fixture.path().join("no-hooks");
    std::fs::create_dir(&no_hooks).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--ignored", "--exact", CHILD, "--nocapture"])
        .env("USHER_TAG_CREATE_CASE", case)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_COUNT", "6")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", &no_hooks)
        .env("GIT_CONFIG_KEY_1", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_1", "false")
        .env("GIT_CONFIG_KEY_2", "tag.gpgsign")
        .env("GIT_CONFIG_VALUE_2", "false")
        .env("GIT_CONFIG_KEY_3", "user.name")
        .env("GIT_CONFIG_VALUE_3", "Fixture")
        .env("GIT_CONFIG_KEY_4", "user.email")
        .env("GIT_CONFIG_VALUE_4", "fixture@example.invalid")
        .env("GIT_CONFIG_KEY_5", "init.defaultBranch")
        .env("GIT_CONFIG_VALUE_5", "main")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    for variable in [
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_DATE",
        "GIT_TEMPLATE_DIR",
    ] {
        child.env_remove(variable);
    }
    let output = child.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn create_fetches_newer_main_before_rebase_and_tag_with_legacy_negative_control() {
    isolated_case("newer-main");
}

#[test]
fn create_accepts_an_already_synchronized_main() {
    isolated_case("synchronized-main");
}

#[test]
fn missing_remote_main_stops_before_rebase_push_and_tag() {
    isolated_case("missing-main");
}

fn git_output(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap()
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = git_output(repo, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn assert_no_tag(repo: &Path) {
    assert!(
        !git_output(
            repo,
            &["rev-parse", "--verify", &format!("refs/tags/{VERSION}")]
        )
        .status
        .success()
    );
}

fn clone_repository(origin: &Path, destination: &Path) {
    let output = Command::new("git")
        .args(["clone", "--quiet", "--branch", "main"])
        .arg(origin)
        .arg(destination)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup(root: &Path) -> (PathBuf, PathBuf) {
    let repo = root.join("repo 中文 & $()");
    let origin = root.join("origin bare 中文.git");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "--bare", "--quiet"]);
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git(&repo, &["commit", "--quiet", "--allow-empty", "-m", "base"]);
    git(&repo, &["branch", "release"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(
        &repo,
        &[
            "push",
            "--quiet",
            "--set-upstream",
            "origin",
            "main",
            "release",
        ],
    );
    (repo, origin)
}

#[test]
#[ignore = "the real Git control runs only with explicit isolated Git configuration"]
fn isolated_real_git_create_fixture() {
    assert!(
        std::env::var_os("GIT_CONFIG_GLOBAL").is_some(),
        "parent provides an isolated Git config"
    );
    let root = tempfile::tempdir().unwrap();
    let (repo, origin) = setup(root.path());
    let case = std::env::var("USHER_TAG_CREATE_CASE").unwrap();
    let base = git(&repo, &["rev-parse", "main"]);
    let token = Arc::new(AtomicBool::new(false));

    crate::app_runtime::shared_runtime().block_on(async {
        if case == "newer-main" {
            let legacy = root.path().join("legacy clone");
            let writer = root.path().join("upstream writer");
            clone_repository(&origin, &legacy);
            clone_repository(&origin, &writer);
            std::fs::write(writer.join("latest-main.txt"), "latest remote main\n").unwrap();
            git(&writer, &["add", "latest-main.txt"]);
            git(
                &writer,
                &["commit", "--quiet", "-m", "remote main advances"],
            );
            git(&writer, &["push", "--quiet", "origin", "main"]);

            let old_plan = create_tag_plan(legacy.to_str().unwrap(), VERSION, "release notes");
            let mut failed = None;
            for command in old_plan
                .iter()
                .filter(|command| command.argv[3..] != ["pull", "origin", "main"])
            {
                if let Err(error) = run_command(command.argv.clone(), &token, |_| {}).await {
                    failed = Some((command.argv[3..].to_vec(), error.log));
                    break;
                }
            }
            let (command, log) = failed.expect("legacy flow must reject a stale main push");
            assert_eq!(command, ["push", "origin", "main"]);
            assert!(log.contains("rejected"), "{log}");
            assert_eq!(git(&legacy, &["rev-parse", "main"]), base);
            assert_no_tag(&legacy);
            assert_no_tag(&origin);
            println!("Tag baseline without main pull: git push origin main rejected as expected");
        } else if case == "missing-main" {
            git(&repo, &["checkout", "--quiet", "release"]);
            std::fs::write(repo.join("release-only.txt"), "release work\n").unwrap();
            git(&repo, &["add", "release-only.txt"]);
            git(&repo, &["commit", "--quiet", "-m", "release advances"]);
            git(&repo, &["push", "--quiet", "origin", "release"]);
            git(&origin, &["update-ref", "-d", "refs/heads/main"]);
        } else {
            assert_eq!(case, "synchronized-main");
        }

        let result = run_local_tag_flow(
            repo.to_str().unwrap(),
            VERSION,
            "release notes 中文 & $()",
            None,
            token,
        )
        .await;
        if case == "missing-main" {
            let failure = result
                .err()
                .expect("pulling a missing remote main must fail");
            let failed = failure
                .steps
                .iter()
                .position(|step| step.conclusion == Some(JobConclusion::Failure))
                .unwrap();
            assert_eq!(
                failure.steps[failed].definition.command[3..],
                ["pull", "origin", "main"]
            );
            assert!(
                failure.steps[..failed]
                    .iter()
                    .all(|step| step.conclusion == Some(JobConclusion::Success))
            );
            assert!(
                failure.steps[failed + 1..]
                    .iter()
                    .all(|step| step.status == JobStepStatus::Queued && step.conclusion.is_none())
            );
            assert_eq!(git(&repo, &["rev-parse", "main"]), base);
            assert!(
                !git_output(&origin, &["rev-parse", "--verify", "refs/heads/main"])
                    .status
                    .success()
            );
            assert_no_tag(&repo);
            assert_no_tag(&origin);
        } else {
            let success =
                result.unwrap_or_else(|failure| panic!("{}: {:?}", failure.summary, failure.steps));
            assert!(
                success
                    .steps
                    .iter()
                    .all(|step| step.conclusion == Some(JobConclusion::Success))
            );
            let latest_main = git(&origin, &["rev-parse", "refs/heads/main"]);
            assert_eq!(git(&repo, &["rev-parse", "main"]), latest_main);
            assert_eq!(
                git(&repo, &["rev-parse", &format!("{VERSION}^{{commit}}")]),
                latest_main
            );
            assert_eq!(
                git(&origin, &["rev-parse", &format!("{VERSION}^{{commit}}")]),
                latest_main
            );
            assert_eq!(
                git(
                    &origin,
                    &["cat-file", "-t", &format!("refs/tags/{VERSION}")]
                ),
                "tag"
            );
            if case == "newer-main" {
                assert_ne!(latest_main, base);
                assert_eq!(
                    git(&repo, &["show", &format!("{VERSION}:latest-main.txt")]),
                    "latest remote main"
                );
                println!(
                    "Tag candidate with main pull: passed; {VERSION} includes latest main {latest_main}"
                );
            } else {
                assert_eq!(latest_main, base);
            }
        }
    });
}
