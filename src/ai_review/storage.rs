use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;

static NEXT_BACKUP: AtomicU64 = AtomicU64::new(1);

pub(super) enum JsonLoad<T> {
    Missing,
    Loaded(T),
    CorruptPreserved { backup: PathBuf, error: String },
}

pub(super) fn load_json_preserving_corrupt<T: DeserializeOwned>(
    path: &Path,
) -> Result<JsonLoad<T>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(JsonLoad::Missing);
        }
        Err(error) => return Err(format!("Failed to read {}: {error}", path.display())),
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(JsonLoad::Loaded(value)),
        Err(error) => {
            let backup = corrupt_backup_path(path);
            fs::rename(path, &backup).map_err(|rename_error| {
                format!(
                    "{} contains invalid JSON ({error}); preserving it failed ({rename_error}), so writes are disabled",
                    path.display()
                )
            })?;
            Ok(JsonLoad::CorruptPreserved {
                backup,
                error: error.to_string(),
            })
        }
    }
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let sequence = NEXT_BACKUP.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("ai-review.json");
    path.with_file_name(format!(
        "{file_name}.corrupt-{millis}-{}-{sequence}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_json_is_preserved_before_returning_a_recovery_state() {
        let dir = std::env::temp_dir().join(format!(
            "usher-ai-review-corrupt-storage-{}-{}",
            std::process::id(),
            NEXT_BACKUP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        fs::write(&path, b"{broken").unwrap();

        let result = load_json_preserving_corrupt::<serde_json::Value>(&path).unwrap();
        let JsonLoad::CorruptPreserved { backup, error } = result else {
            panic!("expected corrupt JSON recovery");
        };
        assert!(!path.exists());
        assert_eq!(fs::read(&backup).unwrap(), b"{broken");
        assert!(!error.is_empty());

        fs::remove_file(backup).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
