//! Strict release-tag comparison shared by the daemon and desktop GUI.
//! Android follows the same contract, checked with testdata/version-comparisons.tsv.

fn parse(value: &str) -> Option<semver::Version> {
    let value = value.trim();
    let version = semver::Version::parse(value.strip_prefix('v').unwrap_or(value)).ok()?;
    // Match the bounded release fields on every platform; never coerce overflow.
    [version.major, version.minor, version.patch]
        .iter()
        .all(|&part| part <= u32::MAX as u64)
        .then_some(version)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn t123_shared_release_version_contract() {
        for line in include_str!("../../testdata/version-comparisons.tsv").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            assert_eq!(
                super::is_newer(parts[0], parts[1]),
                parts[2] == "true",
                "{line}"
            );
        }
    }
}
