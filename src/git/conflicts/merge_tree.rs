use super::*;
use crate::git::RepoType;
use std::{
    collections::BTreeSet,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

async fn run(
    dir: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    fetch: bool,
) -> Result<(i32, Vec<u8>), ConflictError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ConflictError::Cancelled);
    }
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.ext.allow=never",
            "-c",
            "protocol.file.allow=never",
            "-c",
            "credential.interactive=false",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "init.templateDir=",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.http.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "http.followRedirects=false",
            "-c",
            "core.sshCommand=ssh -o BatchMode=yes -o StrictHostKeyChecking=yes",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_TEMPLATE_DIR")
        .env_remove("GIT_SSH_COMMAND")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // Fetch may use the user's configured credential helpers. Merge evaluation
    // never loads their custom merge drivers, aliases or filters.
    if !fetch {
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    }
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let mut child = cmd.spawn().map_err(|_| ConflictError::Unsupported)?;
    let tree = crate::app_runtime::process_tree::ProcessTree::attach(&child)
        .map_err(|_| ConflictError::Unavailable)?;
    let stdout = child.stdout.take().ok_or(ConflictError::Unavailable)?;
    let read = async move {
        let mut output = Vec::new();
        stdout
            .take((MAX_CONTENT + 1) as u64)
            .read_to_end(&mut output)
            .await
            .map_err(|_| ConflictError::Unavailable)?;
        if output.len() > MAX_CONTENT {
            return Err(ConflictError::TooLarge);
        }
        Ok(output)
    };
    let monitor = async {
        let mut ticks = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return ConflictError::Cancelled;
            }
            if ticks % 5 == 0 {
                let root = dir.to_owned();
                let bytes = tokio::task::spawn_blocking(move || directory_bytes(&root))
                    .await
                    .map_err(|_| ConflictError::Unavailable);
                match bytes {
                    Ok(Ok(bytes)) if bytes <= 256 * 1024 * 1024 => {}
                    _ => return ConflictError::TooLarge,
                }
            }
            ticks += 1;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        error = monitor => { tree.terminate();let _ = child.kill().await; Err(error) }
        _ = tokio::time::sleep(Duration::from_secs(60)) => { tree.terminate();let _ = child.kill().await; Err(ConflictError::Unavailable) }
        output = read => {
            let output = output?;
            let status = tokio::time::timeout(Duration::from_secs(2),child.wait()).await.map_err(|_| ConflictError::Unavailable)?.map_err(|_| ConflictError::Unavailable)?;
            Ok((status.code().unwrap_or(-1),output))
        }
    }
}

fn directory_bytes(root: &Path) -> std::io::Result<u64> {
    let mut pending = vec![root.to_owned()];
    let mut bytes = 0u64;
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                pending.push(entry.path())
            } else {
                bytes = bytes.saturating_add(meta.len());
            }
            if bytes > 256 * 1024 * 1024 {
                return Ok(bytes);
            }
        }
    }
    Ok(bytes)
}

pub(super) async fn analyze(
    remote: &str,
    provider: RepoType,
    iid: &str,
    mut snapshot: ConflictSnapshot,
    cancel: Arc<AtomicBool>,
) -> Result<ConflictSnapshot, ConflictError> {
    if remote.starts_with('-')
        || remote.contains('\n')
        || snapshot.target_branch.starts_with('-')
        || snapshot.target_branch.contains(':')
    {
        return Err(ConflictError::Unavailable);
    }
    if let Ok(url) = reqwest::Url::parse(remote) {
        if !matches!(url.scheme(), "https" | "http" | "ssh") || url.password().is_some() {
            return Err(ConflictError::Unsupported);
        }
    } else if !remote.starts_with("git@") {
        return Err(ConflictError::Unsupported);
    }
    let temporary = tempfile::tempdir().map_err(|_| ConflictError::Unavailable)?;
    let dir = temporary.path();
    if run(dir, &["init", "--bare", "--quiet"], &cancel, false)
        .await?
        .0
        != 0
    {
        return Err(ConflictError::Unavailable);
    }
    let help = run(dir, &["merge-tree", "-h"], &cancel, false).await?.1;
    if !String::from_utf8_lossy(&help).contains("--write-tree") {
        return Err(ConflictError::Unsupported);
    }
    let head_ref = match provider {
        RepoType::GitLab => format!("refs/merge-requests/{iid}/head"),
        _ => format!("refs/pull/{iid}/head"),
    };
    let target_ref = format!("refs/heads/{}", snapshot.target_branch);
    if run(dir, &["check-ref-format", &target_ref], &cancel, false)
        .await?
        .0
        != 0
    {
        return Err(ConflictError::InvalidContent);
    }
    // Full bounded-time fetch avoids guessing a merge base from shallow history.
    // Both refs come from the validated target repository, including fork PRs.
    let head_spec = format!("{head_ref}:refs/usher/head");
    let target_spec = format!("{target_ref}:refs/usher/target");
    if run(
        dir,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-recurse-submodules",
            "--",
            remote,
            &head_spec,
            &target_spec,
        ],
        &cancel,
        true,
    )
    .await?
    .0 != 0
    {
        return Err(ConflictError::Authentication);
    }
    let head = revision(dir, "refs/usher/head", &cancel).await?;
    let target = revision(dir, "refs/usher/target", &cancel).await?;
    if (!snapshot.head.is_empty() && snapshot.head != head)
        || (!snapshot.target.is_empty() && snapshot.target != target)
    {
        return Err(ConflictError::Stale);
    }
    snapshot.head = head;
    snapshot.target = target;
    analyze_objects(dir, snapshot, &cancel).await
}

async fn analyze_objects(
    dir: &Path,
    mut snapshot: ConflictSnapshot,
    cancel: &AtomicBool,
) -> Result<ConflictSnapshot, ConflictError> {
    let (exit, output) = run(
        dir,
        &[
            "merge-tree",
            "--write-tree",
            "-z",
            "--messages",
            &snapshot.head,
            &snapshot.target,
        ],
        &cancel,
        false,
    )
    .await?;
    if exit != 0 && exit != 1 {
        return Err(ConflictError::Unavailable);
    }
    let (tree, paths) = parse_output(&output)?;
    snapshot.local = true;
    if exit == 0 {
        if snapshot.status == ConflictStatus::Conflicting {
            return Err(ConflictError::Stale);
        }
        snapshot.status = ConflictStatus::Clear;
        return Ok(snapshot);
    }
    snapshot.status = ConflictStatus::Conflicting;
    let mut total = 0usize;
    for path in paths {
        let (_, entry) = run(dir, &["ls-tree", "-z", &tree, "--", &path], cancel, false).await?;
        let text_mode = entry.starts_with(b"100644 ") || entry.starts_with(b"100755 ");
        let spec = format!("{tree}:{path}");
        let (exit, bytes) = run(dir, &["cat-file", "blob", &spec], &cancel, false).await?;
        total = total.saturating_add(bytes.len());
        if total > MAX_CONTENT {
            return Err(ConflictError::TooLarge);
        }
        let content = if text_mode && exit == 0 && !bytes.contains(&0) {
            String::from_utf8(bytes).ok()
        } else {
            None
        };
        let blocks = content
            .as_ref()
            .map(|s| parse_blocks(s))
            .transpose()?
            .unwrap_or_default();
        snapshot.files.push(ConflictFile {
            path: path.clone(),
            target_path: path,
            content: content.map(Into::into),
            blocks,
        });
    }
    if snapshot.files.is_empty() {
        return Err(ConflictError::Unsupported);
    }
    Ok(snapshot)
}

async fn revision(
    dir: &Path,
    reference: &str,
    cancel: &AtomicBool,
) -> Result<String, ConflictError> {
    let (exit, bytes) = run(
        dir,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        cancel,
        false,
    )
    .await?;
    let sha = String::from_utf8(bytes)
        .map_err(|_| ConflictError::InvalidContent)?
        .trim()
        .to_owned();
    if exit != 0 || !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConflictError::InvalidContent);
    }
    Ok(sha)
}

fn parse_output(output: &[u8]) -> Result<(String, BTreeSet<String>), ConflictError> {
    let mut parts = output.split(|b| *b == 0);
    let tree = std::str::from_utf8(parts.next().ok_or(ConflictError::InvalidContent)?)
        .map_err(|_| ConflictError::InvalidContent)?
        .to_owned();
    if !matches!(tree.len(), 40 | 64) || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConflictError::InvalidContent);
    }
    let mut paths = BTreeSet::new();
    for record in parts.by_ref() {
        if record.is_empty() {
            break;
        }
        let record = std::str::from_utf8(record).map_err(|_| ConflictError::InvalidContent)?;
        let (_, path) = record
            .split_once('\t')
            .ok_or(ConflictError::InvalidContent)?;
        paths.insert(path.to_owned());
        if paths.len() > MAX_FILES {
            return Err(ConflictError::TooLarge);
        }
    }
    // -z message records are path-count, paths, stable type, human text.
    while let Some(count) = parts.next().filter(|p| !p.is_empty()) {
        let count = std::str::from_utf8(count)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n <= MAX_FILES)
            .ok_or(ConflictError::InvalidContent)?;
        let mut message_paths = Vec::with_capacity(count);
        for _ in 0..count {
            message_paths.push(
                std::str::from_utf8(parts.next().ok_or(ConflictError::InvalidContent)?)
                    .map_err(|_| ConflictError::InvalidContent)?
                    .to_owned(),
            );
        }
        let kind = parts.next().ok_or(ConflictError::InvalidContent)?;
        parts.next().ok_or(ConflictError::InvalidContent)?;
        if kind.starts_with(b"CONFLICT") {
            paths.extend(message_paths);
        }
        if paths.len() > MAX_FILES {
            return Err(ConflictError::TooLarge);
        }
    }
    Ok((tree, paths))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().into()
    }
    #[tokio::test]
    async fn real_merge_keeps_text_binary_and_modify_delete_without_checkout_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path();
        git(dir, &["init", "-q", "--initial-branch=main"]);
        for (name, data) in [
            ("sp ace\nfile.txt", b"before\nbase\nafter\n".as_slice()),
            ("binary", b"\0base"),
            ("deleted.txt", b"base\n"),
        ] {
            std::fs::write(dir.join(name), data).unwrap();
        }
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "base"]);
        git(dir, &["checkout", "-qb", "source"]);
        std::fs::write(dir.join("sp ace\nfile.txt"), "before\nsource\nafter\n").unwrap();
        std::fs::write(dir.join("binary"), b"\0source").unwrap();
        std::fs::remove_file(dir.join("deleted.txt")).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "source"]);
        let head = git(dir, &["rev-parse", "HEAD"]);
        git(dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("sp ace\nfile.txt"), "before\ntarget\nafter\n").unwrap();
        std::fs::write(dir.join("binary"), b"\0target").unwrap();
        std::fs::write(dir.join("deleted.txt"), "target\n").unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "target"]);
        let target = git(dir, &["rev-parse", "HEAD"]);
        let snapshot = ConflictSnapshot {
            status: ConflictStatus::Conflicting,
            head,
            target: target.clone(),
            source_branch: "source".into(),
            target_branch: "main".into(),
            local: false,
            files: vec![],
        };
        let cancel = AtomicBool::new(false);
        let help = run(dir, &["merge-tree", "-h"], &cancel, false)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&help.1).contains("--write-tree"));
        let result = analyze_objects(dir, snapshot, &cancel).await.unwrap();
        assert!(result.local);
        assert_eq!(result.files.len(), 3);
        assert!(
            result
                .files
                .iter()
                .find(|f| f.path == "binary")
                .unwrap()
                .content
                .is_none()
        );
        let text = result
            .files
            .iter()
            .find(|f| f.path == "sp ace\nfile.txt")
            .unwrap();
        assert_eq!(text.blocks.len(), 1);
        assert!(
            text.content
                .as_ref()
                .unwrap()
                .contains("source\n=======\ntarget")
        );
        assert!(
            result
                .files
                .iter()
                .find(|f| f.path == "deleted.txt")
                .unwrap()
                .blocks
                .is_empty()
        );
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), target);
        assert!(git(dir, &["status", "--porcelain"]).is_empty());
        assert!(git(dir, &["ls-files", "-u"]).is_empty());
    }
    #[tokio::test]
    async fn cancelled_git_command_never_starts() {
        let temporary = tempfile::tempdir().unwrap();
        let result = run(
            temporary.path(),
            &["init", "--bare"],
            &AtomicBool::new(true),
            false,
        )
        .await;
        assert_eq!(result.unwrap_err(), ConflictError::Cancelled);
        assert!(!temporary.path().join("HEAD").exists());
    }
    #[test]
    fn nul_protocol_preserves_unusual_paths_and_deduplicates_stages() {
        let tree = "a".repeat(40);
        let output = format!(
            "{tree}\0{}\0{}\0\0",
            format!("100644 {tree} 1\ta\nb.rs"),
            format!("100644 {tree} 2\ta\nb.rs")
        );
        let (actual, paths) = parse_output(output.as_bytes()).unwrap();
        assert_eq!(actual, tree);
        assert_eq!(paths.into_iter().collect::<Vec<_>>(), vec!["a\nb.rs"]);
    }
}
