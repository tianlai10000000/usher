use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Message;

pub(crate) const MAX_STORED_MESSAGES: usize = 1_000;

pub(crate) fn default_storage_path() -> io::Result<PathBuf> {
    directories::ProjectDirs::from("com", "usher", "app")
        .map(|dirs| dirs.data_local_dir().join("messages/messages.json"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "application data directory unavailable",
            )
        })
}

pub(crate) fn load(path: &Path) -> io::Result<Vec<Message>> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn save(path: &Path, messages: &[Message]) -> io::Result<()> {
    let encoded = serde_json::to_vec_pretty(messages).map_err(io::Error::other)?;
    usher_fs::atomic_write(path, &encoded)
}
