//! Pure semantic-version rules for the Tag workflow.

pub(super) fn next_semver_for_mode(current_version: &str, version_mode: &str) -> String {
    let (major, minor, patch) = parse_semver_tag(current_version).unwrap_or((0, 0, 0));
    match version_mode {
        "major" => format!("v{}.0.0", major + 1),
        "minor" => format!("v{}.{}.0", major, minor + 1),
        _ => format!("v{}.{}.{}", major, minor, patch + 1),
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
        assert_eq!(next_semver_for_mode("v1.2.3", "major"), "v2.0.0");
        assert_eq!(next_semver_for_mode("v1.2.3", "minor"), "v1.3.0");
        assert_eq!(next_semver_for_mode("v1.2.3", "patch"), "v1.2.4");
        assert_eq!(next_semver_for_mode("invalid", "patch"), "v0.0.1");
    }
}
