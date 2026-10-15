//! Pure semantic-version rules for the Tag workflow.

use std::fmt;

const MAX_REMOTE_TAG_LINE_BYTES: usize = 8 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionBump {
    Major,
    Minor,
    #[default]
    Patch,
}

impl VersionBump {
    pub(crate) fn from_ui(value: &str) -> Self {
        match value {
            "major" => Self::Major,
            "minor" => Self::Minor,
            _ => Self::Patch,
        }
    }

    pub(crate) const fn as_ui_str(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RemoteTagParseError {
    LineTooLong,
}

impl fmt::Display for RemoteTagParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong => formatter.write_str("remote tag reference line exceeded 8 KiB"),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct RemoteTagParser {
    pending: String,
    latest: Option<(String, (u64, u64, u64))>,
}

impl RemoteTagParser {
    pub(super) fn push(&mut self, text: &str) -> Result<(), RemoteTagParseError> {
        for segment in text.split_inclusive('\n') {
            self.pending.push_str(segment);
            if self.pending.ends_with('\n') {
                if self.pending.len() > MAX_REMOTE_TAG_LINE_BYTES + 1 {
                    return Err(RemoteTagParseError::LineTooLong);
                }
                let line = std::mem::take(&mut self.pending);
                self.observe(line.trim_end_matches(['\r', '\n']));
            } else if self.pending.len() > MAX_REMOTE_TAG_LINE_BYTES {
                return Err(RemoteTagParseError::LineTooLong);
            }
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<String, RemoteTagParseError> {
        if self.pending.len() > MAX_REMOTE_TAG_LINE_BYTES {
            return Err(RemoteTagParseError::LineTooLong);
        }
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.observe(line.trim_end_matches('\r'));
        }
        Ok(self
            .latest
            .map(|(tag, _)| tag)
            .unwrap_or_else(|| "v0.0.0".to_string()))
    }

    fn observe(&mut self, line: &str) {
        let Some(tag_ref) = line.split_whitespace().nth(1) else {
            return;
        };
        let Some(tag) = tag_ref.strip_prefix("refs/tags/") else {
            return;
        };
        let Some(parsed) = parse_semver_tag(tag) else {
            return;
        };
        if self
            .latest
            .as_ref()
            .is_none_or(|(_, current)| *current < parsed)
        {
            self.latest = Some((tag.to_string(), parsed));
        }
    }
}

#[cfg(test)]
pub(super) fn latest_remote_tag(output: &str) -> Result<String, RemoteTagParseError> {
    let mut parser = RemoteTagParser::default();
    parser.push(output)?;
    parser.finish()
}

pub(super) fn next_semver(current_version: &str, bump: VersionBump) -> String {
    let (major, minor, patch) = parse_semver_tag(current_version).unwrap_or((0, 0, 0));
    match bump {
        VersionBump::Major => format!("v{}.0.0", major + 1),
        VersionBump::Minor => format!("v{}.{}.0", major, minor + 1),
        VersionBump::Patch => format!("v{}.{}.{}", major, minor, patch + 1),
    }
}

pub(super) fn parse_semver_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let normalized = tag.trim().trim_start_matches(['v', 'V']);
    let core = normalized.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub(super) fn is_valid_semver_tag(tag: &str) -> bool {
    parse_semver_tag(tag).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_and_qualified_semver_tags() {
        assert_eq!(parse_semver_tag("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver_tag("V2.0.1-rc.1"), Some((2, 0, 1)));
        assert_eq!(parse_semver_tag("1.4.0+build.7"), Some((1, 4, 0)));
        assert_eq!(parse_semver_tag("v1.2"), None);
        assert_eq!(parse_semver_tag("release-1.2.3"), None);
    }

    #[test]
    fn computes_each_supported_bump_mode() {
        assert_eq!(next_semver("v1.2.3", VersionBump::Major), "v2.0.0");
        assert_eq!(next_semver("v1.2.3", VersionBump::Minor), "v1.3.0");
        assert_eq!(next_semver("v1.2.3", VersionBump::Patch), "v1.2.4");
        assert_eq!(next_semver("invalid", VersionBump::Patch), "v0.0.1");
    }

    #[test]
    fn version_bump_round_trips_ui_values_and_defaults_unknown_input_to_patch() {
        for bump in [VersionBump::Major, VersionBump::Minor, VersionBump::Patch] {
            assert_eq!(VersionBump::from_ui(bump.as_ui_str()), bump);
        }
        assert_eq!(VersionBump::from_ui("unexpected"), VersionBump::Patch);
        assert_eq!(VersionBump::default(), VersionBump::Patch);
    }

    #[test]
    fn remote_tag_parser_selects_the_latest_valid_reference_across_chunks() {
        let mut parser = RemoteTagParser::default();
        parser
            .push("deadbeef\trefs/tags/V3.0")
            .expect("partial latest record");
        parser
            .push(".0+build.7\nignored\trefs/heads/main\nabc\trefs/tags/v1.9.0\n")
            .expect("first chunk");
        parser
            .push("def\trefs/tags/v2.4.1-rc.2")
            .expect("second chunk");
        assert_eq!(parser.finish().unwrap(), "V3.0.0+build.7");
    }

    #[test]
    fn remote_tag_parser_defaults_when_empty_and_rejects_unbounded_records() {
        assert_eq!(latest_remote_tag("").unwrap(), "v0.0.0");
        let mut parser = RemoteTagParser::default();
        assert_eq!(
            parser.push(&"x".repeat(MAX_REMOTE_TAG_LINE_BYTES + 1)),
            Err(RemoteTagParseError::LineTooLong)
        );
    }
}
