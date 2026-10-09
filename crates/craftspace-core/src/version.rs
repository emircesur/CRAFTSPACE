//! Lenient version parsing for release tags such as `v0.5.0`, `0.5.0-rc.2` or `artcraft-v0.41.0`.

use semver::Version;

/// Parse the version out of a release tag or asset fragment.
///
/// Accepts a leading prefix (`v`, `app-v`, `App v`), and missing minor/patch components (`v1`, `v1.2`).
pub fn parse_tag(tag: &str) -> Option<Version> {
    let tag = tag.trim();
    let bytes = tag.as_bytes();
    // The version starts at the first digit that begins the tag or follows `v`, `-`, `_` or a space.
    let start = (0..bytes.len())
        .find(|&i| bytes[i].is_ascii_digit() && (i == 0 || matches!(bytes[i - 1], b'v' | b'V' | b'-' | b'_' | b' ')))?;
    parse_lenient(&tag[start..])
}

fn parse_lenient(s: &str) -> Option<Version> {
    if let Ok(v) = Version::parse(s) {
        return Some(v);
    }
    // Split off pre-release / build metadata, pad the numeric core to three parts.
    let core_end = s.find(['-', '+']).unwrap_or(s.len());
    let (core, rest) = s.split_at(core_end);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty()
        || parts.len() > 3
        || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let mut padded = parts.join(".");
    for _ in parts.len()..3 {
        padded.push_str(".0");
    }
    Version::parse(&format!("{padded}{rest}")).ok()
}

/// Whether `s` looks like a version (used to tell `app-0.5.0-...` from `app-cli-0.5.0-...`).
pub fn is_version(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_digit()) && parse_lenient(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_tags() {
        assert_eq!(parse_tag("v0.5.0"), Some(Version::new(0, 5, 0)));
        assert_eq!(parse_tag("0.5.0"), Some(Version::new(0, 5, 0)));
        assert_eq!(parse_tag("artcraft-v0.41.0"), Some(Version::new(0, 41, 0)));
        assert_eq!(parse_tag("PhotoCraft v0.5.0"), Some(Version::new(0, 5, 0)));
        assert_eq!(parse_tag("v1.2"), Some(Version::new(1, 2, 0)));
        assert_eq!(parse_tag("v2"), Some(Version::new(2, 0, 0)));
        let rc = parse_tag("v0.1.1-rc.5").unwrap();
        assert_eq!(rc.pre.as_str(), "rc.5");
        assert!(rc < Version::new(0, 1, 1));
    }

    #[test]
    fn rejects_non_versions() {
        assert_eq!(parse_tag("nightly"), None);
        assert_eq!(parse_tag(""), None);
        assert!(!is_version("cli-0.5.0"));
        assert!(is_version("0.5.0"));
        assert!(is_version("0.1.1-rc.4"));
    }
}
