use super::super::command_output::{CommandFailureKind, run_command_with_deadline};
use super::*;
use std::sync::atomic::Ordering;
const CHILD: &str = "workflow::tag::initialization::tests::initialization_child_fixture";
#[test]
#[ignore = "only invoked as a cancellable child-process fixture"]
fn initialization_child_fixture() {
    println!("fixture-start");
    std::thread::sleep(Duration::from_secs(10));
    println!("fixture-finished");
}
fn child() -> Vec<String> {
    vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--ignored".into(),
        "--exact".into(),
        CHILD.into(),
        "--nocapture".into(),
    ]
}
#[test]
fn deadline_and_in_flight_cancel_stop_a_hanging_initialization_process() {
    crate::app_runtime::shared_runtime().block_on(async {
        let token = Arc::new(AtomicBool::new(false));
        let started = std::time::Instant::now();
        let failure = run_command_with_deadline(
            child(),
            &token,
            |_| {},
            |_| Ok(()),
            Some(Duration::from_millis(300)),
        )
        .await
        .unwrap_err();
        assert_eq!(failure.kind, CommandFailureKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
        let cancel = token.clone();
        let failure = run_command_with_deadline(
            child(),
            &token,
            move |log| {
                if log.contains("fixture-start") {
                    cancel.store(true, Ordering::Release);
                }
            },
            |_| Ok(()),
            Some(Duration::from_secs(3)),
        )
        .await
        .unwrap_err();
        assert!(failure.is_cancelled());
        assert!(failure.log.contains("fixture-start"));
        assert!(!failure.log.contains("fixture-finished"));
    });
}
#[test]
fn remote_initialization_uses_real_git_with_argv_paths_and_bounded_parsers() {
    let fixture = tempfile::tempdir().unwrap();
    let global = fixture.path().join("empty-gitconfig");
    std::fs::write(&global, b"").unwrap();
    let no_hooks = fixture.path().join("no-hooks");
    std::fs::create_dir(&no_hooks).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "workflow::tag::initialization::tests::isolated_real_git_initialization_fixture",
            "--nocapture",
        ])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", &no_hooks)
        .env("GIT_CONFIG_KEY_1", "commit.gpgsign")
        .env("GIT_CONFIG_VALUE_1", "false")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "the real Git control runs only with explicit isolated Git configuration"]
fn isolated_real_git_initialization_fixture() {
    let fixtures = tempfile::tempdir().unwrap();
    let repo = fixtures.path().join("repo 中文 & $()");
    let origin = fixtures.path().join("origin bare 中文.git");
    let global = std::env::var_os("GIT_CONFIG_GLOBAL")
        .expect("parent provides a temporary empty global Git config");
    let no_hooks = std::env::var_os("GIT_CONFIG_VALUE_0")
        .expect("parent provides a temporary no-hooks directory");
    let git = |directory: Option<&std::path::Path>, arguments: &[&str]| {
        let mut command = std::process::Command::new("git");
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &global)
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                std::path::Path::new(&no_hooks).display()
            ))
            .args(["-c", "commit.gpgsign=false"]);
        if let Some(directory) = directory {
            command.arg("-C").arg(directory);
        }
        let output = command.args(arguments).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    git(None, &["init", "--quiet", repo.to_str().unwrap()]);
    git(
        None,
        &["init", "--bare", "--quiet", origin.to_str().unwrap()],
    );
    git(
        Some(&repo),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
    git(Some(&repo), &["tag", "v99.0.0"]);
    git(
        Some(&repo),
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(Some(&repo), &["push", "origin", "HEAD:refs/heads/main"]);
    git(Some(&origin), &["tag", "release-one", "refs/heads/main"]);
    let token = Arc::new(AtomicBool::new(false));
    crate::app_runtime::shared_runtime().block_on(async {
        // A local semantic Tag is not a fallback when origin has no accepted Tag.
        assert_eq!(
            inspect(repo.to_str().unwrap(), false, &token)
                .await
                .unwrap(),
            "v0.0.0"
        );

        git(Some(&repo), &["tag", "v1.2.3"]);
        git(Some(&repo), &["push", "origin", "refs/tags/v1.2.3"]);
        git(Some(&origin), &["tag", "v2.9.0", "refs/heads/main"]);
        git(
            Some(&origin),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "tag",
                "-a",
                "V2.10.0-rc.1",
                "-m",
                "remote annotated tag",
                "refs/heads/main",
            ],
        );
        assert_eq!(
            inspect(repo.to_str().unwrap(), false, &token)
                .await
                .unwrap(),
            "V2.10.0-rc.1"
        );
        assert!(
            git(Some(&repo), &["tag", "--list", "v2.9.0", "V2.10.0-rc.1"])
                .stdout
                .is_empty(),
            "Add inspection must leave remote-only Tags absent locally"
        );

        git(Some(&origin), &["tag", "V3.0.0+build.7", "refs/heads/main"]);
        assert_eq!(
            inspect(repo.to_str().unwrap(), false, &token)
                .await
                .unwrap(),
            "V3.0.0+build.7",
            "a later inspection must see a newly published remote Tag"
        );
        assert!(
            git(Some(&repo), &["tag", "--list", "V3.0.0+build.7"])
                .stdout
                .is_empty()
        );

        // Git owns provider URL handling. Rewrite each transport to the bare
        // fixture; file-only protocol admission prevents accidental networking.
        let rewrite_key = format!("url.{}.insteadOf", origin.display());
        for remote in [
            "https://github.com/team/app.git",
            "git@github.com:team/app.git",
            "https://gitlab.com/team/mobile/app.git",
            "git@gitlab.com:team/mobile/app.git",
            "https://gitee.com/team/app.git",
            "git@gitee.com:team/app.git",
            "https://code.example.invalid:8443/team/mobile/app.git",
            "ssh://git@code.example.invalid:2222/team/mobile/app.git",
        ] {
            git(Some(&repo), &["config", "--add", &rewrite_key, remote]);
            git(Some(&repo), &["remote", "set-url", "origin", remote]);
            assert_eq!(
                inspect(repo.to_str().unwrap(), false, &token)
                    .await
                    .unwrap(),
                "V3.0.0+build.7",
                "provider-neutral Git transport must inspect {remote}"
            );
        }
        assert!(
            git(Some(&repo), &["tag", "--list", "V3.0.0+build.7"])
                .stdout
                .is_empty()
        );
        git(
            Some(&repo),
            &["remote", "set-url", "origin", origin.to_str().unwrap()],
        );

        // Remove keeps its fetch so remote-only removal targets exist locally.
        assert_eq!(
            inspect(repo.to_str().unwrap(), true, &token).await.unwrap(),
            "V3.0.0+build.7"
        );
        assert_eq!(
            String::from_utf8(git(Some(&repo), &["tag", "--list", "V3.0.0+build.7"]).stdout)
                .unwrap()
                .trim(),
            "V3.0.0+build.7"
        );

        git(Some(&repo), &["remote", "remove", "origin"]);
        assert!(
            inspect(repo.to_str().unwrap(), false, &token)
                .await
                .is_err(),
            "a missing origin must not silently fall back to a local Tag"
        );
        let missing = fixtures.path().join("missing origin.git");
        git(
            Some(&repo),
            &["remote", "add", "origin", missing.to_str().unwrap()],
        );
        assert!(
            inspect(repo.to_str().unwrap(), false, &token)
                .await
                .is_err(),
            "a failed remote query must not silently fall back to a local Tag"
        );
    });
    let mut parser = RemoteTagParser::default();
    assert!(parser.push(&"x".repeat(8194)).is_err());
}

#[cfg(unix)]
#[test]
#[ignore = "only invoked as a child-process tree fixture"]
fn initialization_descendant_fixture() {
    let marker =
        std::env::var_os("USHER_TAG_TEST_DESCENDANT_MARKER").expect("explicit fixture marker");
    if let Some(release) = std::env::var_os("USHER_TAG_TEST_DESCENDANT_RELEASE") {
        let release = std::path::PathBuf::from(release);
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        while !release.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "direct-child control did not release its descendant"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        std::thread::sleep(Duration::from_millis(1500));
    }
    std::fs::write(marker, b"descendant escaped cancellation").unwrap();
}
#[cfg(unix)]
#[test]
#[ignore = "only invoked as a parent-process tree fixture"]
fn initialization_parent_fixture() {
    let external_marker = std::env::var_os("USHER_TAG_TEST_DESCENDANT_MARKER");
    let fixture = external_marker
        .is_none()
        .then(|| tempfile::tempdir().unwrap());
    let marker = external_marker
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| fixture.as_ref().unwrap().path().join("must-not-exist"));
    let mut descendant = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "workflow::tag::initialization::tests::initialization_descendant_fixture",
            "--nocapture",
        ])
        .env("USHER_TAG_TEST_DESCENDANT_MARKER", &marker)
        .spawn()
        .unwrap();
    println!("descendant-marker={}", marker.display());
    std::thread::sleep(Duration::from_secs(10));
    let _ = descendant.wait();
}
#[cfg(unix)]
#[test]
fn legacy_direct_child_only_termination_allows_a_git_style_descendant_to_escape() {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let fixture = tempfile::tempdir().unwrap();
    let marker = fixture.path().join("escaped-descendant");
    let release = fixture.path().join("release-descendant");
    crate::app_runtime::shared_runtime().block_on(async {
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "workflow::tag::initialization::tests::initialization_parent_fixture",
                "--nocapture",
            ])
            .env("USHER_TAG_TEST_DESCENDANT_MARKER", &marker)
            .env("USHER_TAG_TEST_DESCENDANT_RELEASE", &release)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        // This owner is solely cleanup: the legacy observation below only
        // signals the direct child. Drop still kills descendants on any panic.
        let cleanup = crate::app_runtime::process_tree::ProcessTree::attach(&child).unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let reported_marker = tokio::time::timeout(Duration::from_secs(3), async {
            let mut line = String::new();
            loop {
                line.clear();
                assert_ne!(stdout.read_line(&mut line).await.unwrap(), 0);
                if let Some(path) = line.trim_end().strip_prefix("descendant-marker=") {
                    break std::path::PathBuf::from(path);
                }
            }
        })
        .await
        .expect("parent fixture did not report its descendant within the deadline");
        assert_eq!(reported_marker, marker);
        assert!(!marker.exists());
        child.start_kill().unwrap();
        let status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("legacy direct-child kill did not finish within the deadline")
            .unwrap();
        assert!(!status.success());
        // Release only after the direct child is reaped: a resulting write
        // therefore proves its descendant survived that exact legacy boundary.
        std::fs::write(&release, b"direct child reaped").unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !std::fs::read(&marker)
                .is_ok_and(|contents| contents == b"descendant escaped cancellation")
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the old direct-child-only mode did not expose descendant escape");
        assert_eq!(
            std::fs::read(&marker).unwrap(),
            b"descendant escaped cancellation"
        );
        cleanup.terminate();
    });
}
#[cfg(unix)]
#[test]
fn deadline_and_cancel_terminate_git_style_descendants_before_they_can_write() {
    crate::app_runtime::shared_runtime().block_on(async {
        for cancel_requested in [false, true] {
            let command = vec![
                std::env::current_exe()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                "--ignored".into(),
                "--exact".into(),
                "workflow::tag::initialization::tests::initialization_parent_fixture".into(),
                "--nocapture".into(),
            ];
            let token = Arc::new(AtomicBool::new(false));
            let cancel = token.clone();
            let mut output = String::new();
            let failure = run_command_with_deadline(
                command,
                &token,
                move |log| {
                    if cancel_requested && log.contains("descendant-marker=") {
                        cancel.store(true, Ordering::Release);
                    }
                },
                |text| {
                    output.push_str(text);
                    Ok(())
                },
                Some(Duration::from_millis(500)),
            )
            .await
            .unwrap_err();
            assert_eq!(
                failure.kind,
                if cancel_requested {
                    CommandFailureKind::Cancelled
                } else {
                    CommandFailureKind::TimedOut
                }
            );
            let marker = output
                .lines()
                .find_map(|line| line.strip_prefix("descendant-marker="))
                .expect("parent fixture must have started its descendant");
            let marker = std::path::PathBuf::from(marker);
            tokio::time::sleep(Duration::from_millis(1800)).await;
            assert!(
                !marker.exists(),
                "descendant survived command cancellation: {}",
                marker.display()
            );
            std::fs::remove_dir_all(marker.parent().unwrap()).unwrap();
        }
    });
}
