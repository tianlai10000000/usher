use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use fs2::FileExt;

static INSTANCE_LOCK_FILE: OnceLock<File> = OnceLock::new();

fn lock_file_name(namespace: Option<&str>) -> String {
    let namespace = namespace.filter(|value| {
        !value.is_empty()
            && value.len() <= 80
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    });
    namespace.map_or_else(
        || "usher.instance.lock".to_owned(),
        |value| format!("usher.{value}.instance.lock"),
    )
}

fn lock_file_path() -> PathBuf {
    let namespace = std::env::var("USHER_BENCHMARK_INSTANCE_NAMESPACE").ok();
    let file_name = lock_file_name(namespace.as_deref());
    if let Some(proj_dirs) = directories::ProjectDirs::from("com", "usher", "app") {
        let dir = proj_dirs.data_local_dir();
        let _ = std::fs::create_dir_all(dir);
        dir.join(file_name)
    } else {
        PathBuf::from(file_name)
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

#[cfg(test)]
mod tests {
    use super::lock_file_name;

    #[test]
    fn ordinary_launches_keep_the_global_lock_name() {
        assert_eq!(lock_file_name(None), "usher.instance.lock");
        assert_eq!(lock_file_name(Some("")), "usher.instance.lock");
    }

    #[test]
    fn benchmark_namespaces_are_bounded_and_path_safe() {
        assert_eq!(
            lock_file_name(Some("chat-fast-drag-20260814")),
            "usher.chat-fast-drag-20260814.instance.lock"
        );
        assert_eq!(lock_file_name(Some("../escape")), "usher.instance.lock");
        assert_eq!(
            lock_file_name(Some("contains space")),
            "usher.instance.lock"
        );
    }
}
