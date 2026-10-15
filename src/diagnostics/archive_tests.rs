use super::*;

#[test]
fn diagnostic_collection_ignores_symlinks_and_orders_regular_files() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("2026-08-30")).unwrap();
    fs::write(root.path().join("2026-08-30/usher.log"), "log").unwrap();
    fs::write(root.path().join("2026-08-30/usher_tracing.log"), "trace").unwrap();
    fs::write(root.path().join("2026-08-30/settings.toml"), "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.path().join("2026-08-30/usher.log"),
        root.path().join("linked.log"),
    )
    .unwrap();
    let files = collect_log_files(root.path()).unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[0] < files[1]);
    assert!(files.iter().all(|path| path.extension().unwrap() == "log"));
}

fn archives(root: &Path) -> Vec<PathBuf> {
    let mut result = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "gz"))
        .collect::<Vec<_>>();
    result.sort();
    result
}

#[test]
fn consecutive_exports_finalize_readable_private_archives_and_immediately_keep_five() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    fs::write(logs.path().join("usher.log"), "bounded log fixture").unwrap();
    fs::write(logs.path().join("settings.toml"), "synthetic-secret").unwrap();
    for index in 1..=8 {
        let path = export(root.path(), Some(logs.path())).unwrap();
        assert!(
            path.is_file(),
            "the successful returned path must survive its own prune"
        );
        assert_eq!(archives(root.path()).len(), index.min(ARCHIVE_LIMIT));
        let decoder = flate2::read::GzDecoder::new(File::open(&path).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let mut names = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            vec![
                PathBuf::from("logs/usher.log"),
                PathBuf::from("manifest.json")
            ]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    let files = archives(root.path());
    let oldest = files[0].clone();
    let current = export(root.path(), None).unwrap();
    assert_eq!(archives(root.path()).len(), ARCHIVE_LIMIT);
    assert!(!oldest.exists());
    assert!(current.exists());
    assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")
    }));
}

#[test]
fn retention_orders_names_ignores_unrelated_files_and_keeps_current_after_clock_rollback() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..7 {
        fs::write(
            root.path()
                .join(format!("usher-diagnostics-9999-{index}.tar.gz")),
            "fixture",
        )
        .unwrap();
    }
    fs::write(root.path().join("keep-me.txt"), "fixture").unwrap();
    prune(root.path()).unwrap();
    assert!(!root.path().join("usher-diagnostics-9999-0.tar.gz").exists());
    assert!(!root.path().join("usher-diagnostics-9999-1.tar.gz").exists());
    let path = export(root.path(), None).unwrap();
    assert!(
        path.exists(),
        "the current export must remain usable even if older archives have future timestamps"
    );
    assert!(!root.path().join("usher-diagnostics-9999-2.tar.gz").exists());
    assert!(root.path().join("keep-me.txt").exists());
    assert_eq!(archives(root.path()).len(), ARCHIVE_LIMIT);
}

#[test]
fn concurrent_exports_and_startup_prune_are_one_retention_transaction() {
    let root = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        let jobs = (0..12)
            .map(|_| {
                let root = root.path();
                scope.spawn(move || export(root, None).unwrap())
            })
            .collect::<Vec<_>>();
        let cleanup = scope.spawn(|| prune(root.path()).unwrap());
        let mut paths = jobs
            .into_iter()
            .map(|job| job.join().unwrap())
            .collect::<Vec<_>>();
        cleanup.join().unwrap();
        paths.sort();
        paths.dedup();
        assert_eq!(
            paths.len(),
            12,
            "same-millisecond exports must never collide or overwrite"
        );
    });
    assert_eq!(archives(root.path()).len(), ARCHIVE_LIMIT);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), ARCHIVE_LIMIT);
}

#[test]
fn failed_retention_returns_failure_and_removes_the_new_publication() {
    let root = tempfile::tempdir().unwrap();
    let legitimate = export(root.path(), None).unwrap();
    let error = export_with_retention(root.path(), None, |_, current| {
        assert!(
            current.is_file(),
            "failure fixture must run after successful finalization/publication"
        );
        Err(io::Error::other("injected retention failure"))
    })
    .unwrap_err();
    assert!(error.to_string().contains("injected retention failure"));
    assert_eq!(archives(root.path()), vec![legitimate]);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    let blocked_root = root.path().join("ordinary-file");
    fs::write(&blocked_root, "fixture").unwrap();
    assert!(export(&blocked_root, None).is_err());
}

#[test]
fn exported_payload_keeps_file_and_total_budgets_with_unrelated_sources_excluded() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    for index in 0..9 {
        File::create(logs.path().join(format!("fixture-{index}.log")))
            .unwrap()
            .set_len(FILE_LIMIT)
            .unwrap();
    }
    File::create(logs.path().join("oversized.log"))
        .unwrap()
        .set_len(FILE_LIMIT + 1)
        .unwrap();
    fs::write(logs.path().join("settings.toml"), "synthetic-credential").unwrap();
    let path = export(root.path(), Some(logs.path())).unwrap();
    let decoder = flate2::read::GzDecoder::new(File::open(path).unwrap());
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.path().unwrap().into_owned(), entry.size())
        })
        .collect::<Vec<_>>();
    let log_entries = entries
        .iter()
        .filter(|(path, _)| path.starts_with("logs"))
        .collect::<Vec<_>>();
    assert_eq!(log_entries.len(), 8);
    assert_eq!(
        log_entries.iter().map(|(_, bytes)| *bytes).sum::<u64>(),
        TOTAL_LIMIT
    );
    assert!(log_entries.iter().all(|(_, bytes)| *bytes <= FILE_LIMIT));
    assert!(
        !entries
            .iter()
            .any(|(path, _)| path.ends_with("oversized.log") || path.ends_with("settings.toml"))
    );
}
