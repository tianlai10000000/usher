use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const HISTORY_FILE_NAME: &str = "chat_history.jsonl";
pub(crate) const AI_HISTORY_FILE_NAME: &str = "ai_chat_history.jsonl";
const TAIL_CHUNK_SIZE: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredMessageStatus {
    #[default]
    Normal,
    Error,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredChatMessage {
    pub id: i64,
    pub role: StoredMessageRole,
    #[serde(default)]
    pub status: StoredMessageStatus,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct HistoryPage {
    pub messages: Vec<StoredChatMessage>,
    pub has_more_before: bool,
}

#[cfg(test)]
mod tests {
    use super::{StoredChatMessage, StoredMessageStatus};

    #[test]
    fn old_history_without_status_defaults_to_normal() {
        let message: StoredChatMessage = serde_json::from_str(
            r#"{"id":1,"role":"assistant","content":"legacy","timestamp":"12:00"}"#,
        )
        .expect("legacy history should remain readable");

        assert_eq!(message.status, StoredMessageStatus::Normal);
    }
}

#[derive(Debug, Clone)]
pub struct JsonlHistoryStore {
    file_path: PathBuf,
}

impl JsonlHistoryStore {
    pub fn new_default() -> io::Result<Self> {
        Self::new_default_named(HISTORY_FILE_NAME)
    }

    pub(crate) fn new_default_named(file_name: &str) -> io::Result<Self> {
        let base_dir = if let Some(proj_dirs) = ProjectDirs::from("com", "usher", "app") {
            proj_dirs.data_local_dir().to_path_buf()
        } else {
            PathBuf::from(".")
        };
        let im_dir = base_dir.join("im");
        fs::create_dir_all(&im_dir)?;
        let file_path = im_dir.join(file_name);
        if !file_path.exists() {
            File::create(&file_path)?;
        }
        Ok(Self { file_path })
    }

    #[cfg(test)]
    pub(crate) fn open_test_path(file_path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !file_path.exists() {
            File::create(&file_path)?;
        }
        Ok(Self { file_path })
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub fn append(&self, message: &StoredChatMessage) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        let line = serde_json::to_string(message)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}")))?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()
    }

    pub fn load_recent(&self, limit: usize) -> io::Result<Vec<StoredChatMessage>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let lines = self.read_tail_lines(limit.saturating_mul(4))?;
        let mut parsed = Vec::new();
        for line in lines {
            if let Some(msg) = Self::parse_line(&line) {
                parsed.push(msg);
            }
        }

        if parsed.len() > limit {
            Ok(parsed.split_off(parsed.len() - limit))
        } else {
            Ok(parsed)
        }
    }

    pub fn load_before(&self, before_id: i64, limit: usize) -> io::Result<HistoryPage> {
        if limit == 0 {
            return Ok(HistoryPage {
                messages: Vec::new(),
                has_more_before: false,
            });
        }

        let file = OpenOptions::new().read(true).open(&self.file_path)?;
        let reader = BufReader::new(file);
        let mut deque: VecDeque<StoredChatMessage> = VecDeque::with_capacity(limit + 1);
        let mut has_more_before = false;

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(msg) = Self::parse_line(&line) else {
                continue;
            };
            if msg.id < before_id {
                deque.push_back(msg);
                if deque.len() > limit {
                    deque.pop_front();
                    has_more_before = true;
                }
            }
        }

        Ok(HistoryPage {
            messages: deque.into_iter().collect(),
            has_more_before,
        })
    }

    pub fn has_older_than(&self, id: i64) -> io::Result<bool> {
        let file = OpenOptions::new().read(true).open(&self.file_path)?;
        let reader = BufReader::new(file);
        for line_result in reader.lines() {
            let line = match line_result {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(msg) = Self::parse_line(&line) else {
                continue;
            };
            if msg.id < id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn latest_message_id(&self) -> io::Result<i64> {
        let lines = self.read_tail_lines(8)?;
        for line in lines.into_iter().rev() {
            if let Some(msg) = Self::parse_line(&line) {
                return Ok(msg.id);
            }
        }
        Ok(0)
    }

    fn parse_line(line: &str) -> Option<StoredChatMessage> {
        if line.trim().is_empty() {
            return None;
        }
        serde_json::from_str::<StoredChatMessage>(line).ok()
    }

    fn read_tail_lines(&self, max_lines: usize) -> io::Result<Vec<String>> {
        if max_lines == 0 {
            return Ok(Vec::new());
        }

        let mut file = OpenOptions::new().read(true).open(&self.file_path)?;
        let file_len = file.metadata()?.len();
        if file_len == 0 {
            return Ok(Vec::new());
        }

        let mut pos = file_len;
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut newline_count = 0usize;

        while pos > 0 && newline_count <= max_lines {
            let read_size = usize::min(TAIL_CHUNK_SIZE, pos as usize);
            pos -= read_size as u64;
            file.seek(SeekFrom::Start(pos))?;

            let mut chunk = vec![0u8; read_size];
            file.read_exact(&mut chunk)?;
            newline_count += chunk.iter().filter(|&&b| b == b'\n').count();
            chunks.push(chunk);
        }

        chunks.reverse();
        let total_len: usize = chunks.iter().map(Vec::len).sum();
        let mut data = Vec::with_capacity(total_len);
        for chunk in chunks {
            data.extend_from_slice(&chunk);
        }

        let text = String::from_utf8_lossy(&data);
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        if lines.len() > max_lines {
            Ok(lines[lines.len() - max_lines..].to_vec())
        } else {
            Ok(lines)
        }
    }
}
