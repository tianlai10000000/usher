//! Serialized diagnostic archive publication and immediate retention.
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use flate2::{Compression, write::GzEncoder};

const ARCHIVE_LIMIT: usize = 5;
const FILE_LIMIT: u64 = 8 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 64 * 1024 * 1024;
static ARCHIVE_TRANSACTION: Mutex<()> = Mutex::new(());
static ARCHIVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A published file is still owned until retention succeeds. Any later error
/// removes this new archive rather than returning a partially successful path.
struct Publication {
    file: Option<File>,
    path: PathBuf,
    retained: bool,
}
impl Drop for Publication {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.retained {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) fn export(output_root: &Path, logs: Option<&Path>) -> Result<PathBuf> {
    export_with_retention(output_root, logs, |root, current| {
        prune_locked(root, Some(current))
    })
}

fn export_with_retention(
    output_root: &Path,
    logs: Option<&Path>,
    retain: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<PathBuf> {
    let _transaction = ARCHIVE_TRANSACTION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    fs::create_dir_all(output_root)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".usher-diagnostics-")
        .suffix(".partial")
        .tempfile_in(output_root)?;
    write_bundle(temporary.as_file_mut(), logs)?;
    temporary.as_file().sync_all()?;
    let output = output_root.join(format!(
        "usher-diagnostics-{}-{}-{:020}.tar.gz",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"),
        std::process::id(),
        ARCHIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    let mut published = Publication {
        file: Some(temporary.persist_noclobber(&output)?),
        path: output,
        retained: false,
    };
    retain(output_root, &published.path)?;
    published.retained = true;
    Ok(published.path.clone())
}

fn write_bundle(file: &mut File, logs: Option<&Path>) -> Result<()> {
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "application": "Usher",
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "contents": "Bounded application and tracing logs only; settings and user data are excluded."
    }))?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o600);
    header.set_cksum();
    archive.append_data(&mut header, "manifest.json", manifest.as_slice())?;
    let mut included = 0u64;
    if let Some(root) = logs {
        for path in collect_log_files(root)? {
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file()
                || metadata.len() > FILE_LIMIT
                || included.saturating_add(metadata.len()) > TOTAL_LIMIT
            {
                continue;
            }
            archive
                .append_path_with_name(&path, Path::new("logs").join(path.strip_prefix(root)?))?;
            included = included.saturating_add(metadata.len());
        }
    }
    // tar::finish alone does not report the gzip footer's final write errors.
    archive.into_inner()?.finish()?;
    Ok(())
}

fn collect_log_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "log")
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn prune(root: &Path) -> io::Result<()> {
    let _transaction = ARCHIVE_TRANSACTION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    prune_locked(root, None)
}

fn prune_locked(root: &Path, current: Option<&Path>) -> io::Result<()> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut archives = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("usher-diagnostics-") && name.ends_with(".tar.gz")
            })
        {
            archives.push(entry.path());
        }
    }
    archives.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    let mut remaining = archives.len();
    for archive in archives {
        if remaining <= ARCHIVE_LIMIT {
            break;
        }
        if current == Some(archive.as_path()) {
            continue;
        }
        fs::remove_file(archive)?;
        remaining -= 1;
    }
    Ok(())
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
