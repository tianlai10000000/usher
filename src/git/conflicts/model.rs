use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ConflictStatus {
    Checking,
    Conflicting,
    Clear,
    #[default]
    Unknown,
    NotApplicable,
}

impl ConflictStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Conflicting => "conflicting",
            Self::Clear => "clear",
            Self::Unknown => "unknown",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictError {
    Unavailable,
    Authentication,
    Unsupported,
    TooLarge,
    InvalidContent,
    Stale,
    Cancelled,
}

impl ConflictError {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Authentication => "conflicts.authentication",
            Self::Unsupported => "conflicts.unsupported",
            Self::TooLarge => "conflicts.too_large",
            Self::InvalidContent => "conflicts.invalid_content",
            Self::Stale => "conflicts.stale",
            Self::Cancelled | Self::Unavailable => "conflicts.unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConflictBlock {
    pub(crate) start: usize,
    pub(crate) separator: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ConflictFile {
    pub(crate) path: String,
    pub(crate) target_path: String,
    pub(crate) content: Option<Arc<str>>,
    pub(crate) blocks: Vec<ConflictBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConflictSnapshot {
    pub(crate) status: ConflictStatus,
    pub(crate) head: String,
    pub(crate) target: String,
    pub(crate) source_branch: String,
    pub(crate) target_branch: String,
    pub(crate) local: bool,
    pub(crate) files: Vec<ConflictFile>,
}

pub(crate) const MAX_CONTENT: usize = 8 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 1000;

/// Only complete, balanced marker groups become conflict blocks. Ordinary
/// source text containing marker-like strings must not become a fake range.
pub(crate) fn parse_blocks(content: &str) -> Result<Vec<ConflictBlock>, ConflictError> {
    if content.len() > MAX_CONTENT {
        return Err(ConflictError::TooLarge);
    }
    let mut blocks = Vec::new();
    let mut start = None;
    let mut separator = None;
    let mut marker_width = 0;
    for (index, line) in content.lines().enumerate() {
        if index >= 100_000 || line.len() > 64 * 1024 {
            return Err(ConflictError::TooLarge);
        }
        let run = line.bytes().take_while(|b| *b == b'<').count();
        if run >= 7 && line.as_bytes().get(run) == Some(&b' ') {
            if start.is_some() {
                return Err(ConflictError::InvalidContent);
            }
            start = Some(index);
            marker_width = run;
        } else if start.is_some() && line == "=".repeat(marker_width) {
            if separator.replace(index).is_some() {
                return Err(ConflictError::InvalidContent);
            }
        } else if start.is_some() && line.starts_with(&(">".repeat(marker_width) + " ")) {
            blocks.push(ConflictBlock {
                start: start.take().unwrap(),
                separator: separator.take().ok_or(ConflictError::InvalidContent)?,
                end: index,
            });
        }
    }
    if start.is_some() {
        return Err(ConflictError::InvalidContent);
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflict_range_includes_both_markers_and_diff3_ancestor() {
        let blocks = parse_blocks(
            "context\n<<<<<<< source\na\n||||||| base\nb\n=======\nc\n>>>>>>> target\nnext",
        )
        .unwrap();
        assert_eq!(
            blocks,
            vec![ConflictBlock {
                start: 1,
                separator: 5,
                end: 7
            }]
        );
        assert!(parse_blocks("<<<<<<< source\na\n=======\nb").is_err());
        assert!(
            parse_blocks("let text = \"<<<<<<< source\";")
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn empty_sides_crlf_and_multiple_blocks() {
        let text = "<<<<<<< s\r\n=======\r\n>>>>>>> t\r\n\n<<<<<<< s\na\n=======\nb\n>>>>>>> t";
        let blocks = parse_blocks(text).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!((blocks[0].start, blocks[0].end), (0, 2));
        assert_eq!((blocks[1].start, blocks[1].end), (4, 8));
    }
}
