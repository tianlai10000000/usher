use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use usher_ai_review::ReviewTaskId;

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

pub(super) fn session_paths(session_dir: &Path, id: &ReviewTaskId) -> (PathBuf, PathBuf) {
    let filtered =
        id.0.chars()
            .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
            .collect::<String>();
    let safe = if !filtered.is_empty() && filtered == id.0 {
        filtered
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id.0.hash(&mut hasher);
        let prefix = if filtered.is_empty() {
            "task".to_string()
        } else {
            filtered
        };
        format!("{prefix}-{:016x}", hasher.finish())
    };
    (
        session_dir.join(format!("{safe}.json")),
        session_dir.join(format!("{safe}.chat.json")),
    )
}

pub(super) fn reconcile_session_files<'a>(
    session_dir: &Path,
    current_ids: impl Iterator<Item = &'a ReviewTaskId>,
) -> HashSet<ReviewTaskId> {
    let current_ids = current_ids.cloned().collect::<HashSet<_>>();
    let mut desired_paths = HashSet::new();
    for id in &current_ids {
        let (review_path, chat_path) = session_paths(session_dir, id);
        desired_paths.insert(review_path);
        desired_paths.insert(chat_path);
    }

    let entries = match fs::read_dir(session_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return current_ids,
        Err(error) => {
            log::warn!(
                "[AI Review][Session Store] Failed to reconcile {}: {error}",
                session_dir.display()
            );
            return current_ids;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let stale_temporary = file_name.ends_with(".tmp") || file_name.contains(".tmp-");
        let orphaned_session = file_name.ends_with(".json") && !desired_paths.contains(&path);
        if (stale_temporary || orphaned_session)
            && let Err(error) = fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "[AI Review][Session Store] Failed to remove stale file {}: {error}",
                path.display()
            );
        }
    }
    current_ids
}

pub(super) fn remove_stale_temporary_files(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".tmp") || name.contains(".tmp-"));
        if stale
            && let Err(error) = fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "[AI Review][Session Store] Failed to remove temporary file {}: {error}",
                path.display()
            );
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
