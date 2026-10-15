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
fn local_and_remote_initialization_use_real_git_with_argv_paths_and_bounded_parsers() {
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
    let git = |arguments: &[&std::ffi::OsStr]| {
        let output = std::process::Command::new("git")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &global)
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                std::path::Path::new(&no_hooks).display()
            ))
            .args(["-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    use std::ffi::OsStr;
    git(&[OsStr::new("init"), OsStr::new("--quiet"), repo.as_os_str()]);
    git(&[
        OsStr::new("init"),
        OsStr::new("--bare"),
        OsStr::new("--quiet"),
        origin.as_os_str(),
    ]);
    git(&[
        OsStr::new("-C"),
        repo.as_os_str(),
        OsStr::new("-c"),
        OsStr::new("user.name=Fixture"),
        OsStr::new("-c"),
        OsStr::new("user.email=fixture@example.invalid"),
        OsStr::new("commit"),
        OsStr::new("--allow-empty"),
        OsStr::new("-m"),
        OsStr::new("fixture"),
    ]);
    git(&[
        OsStr::new("-C"),
        repo.as_os_str(),
        OsStr::new("tag"),
        OsStr::new("v1.2.3"),
    ]);
    git(&[
        OsStr::new("-C"),
        repo.as_os_str(),
        OsStr::new("remote"),
        OsStr::new("add"),
        OsStr::new("origin"),
        origin.as_os_str(),
    ]);
    git(&[
        OsStr::new("-C"),
        repo.as_os_str(),
        OsStr::new("push"),
        OsStr::new("--tags"),
        OsStr::new("origin"),
    ]);
    let token = Arc::new(AtomicBool::new(false));
    crate::app_runtime::shared_runtime().block_on(async {
        assert_eq!(
            inspect(repo.to_str().unwrap(), false, &token)
                .await
                .unwrap(),
            "v1.2.3"
        );
        assert_eq!(
            inspect(repo.to_str().unwrap(), true, &token).await.unwrap(),
            "v1.2.3"
        );
    });
    let mut parser = TagParser::new(false);
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
