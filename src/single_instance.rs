use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use fs2::FileExt;

static INSTANCE_LOCK_FILE: OnceLock<File> = OnceLock::new();

fn lock_file_path() -> PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("com", "usher", "app") {
        let dir = proj_dirs.data_local_dir();
        let _ = std::fs::create_dir_all(dir);
        dir.join("usher.instance.lock")
    } else {
        PathBuf::from("usher.instance.lock")
    }
}

/// Acquire global process lock.
/// Returns false when another instance is already running.
pub fn acquire_instance_lock() -> Result<bool> {
    let path = lock_file_path();
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open lock file: {}", path.display()))?;

    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = INSTANCE_LOCK_FILE.set(file);
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}
