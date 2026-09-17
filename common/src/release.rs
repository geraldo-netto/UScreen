//! Release metadata policy shared by synchronous and asynchronous consumers.
pub const API: &str = "https://api.github.com/repos/geraldo-netto/UScreen/releases/latest";
pub const PAGE: &str = "https://github.com/geraldo-netto/UScreen/releases/latest";

pub fn tag_from_json(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("tag_name")?.as_str().map(str::to_owned)
}

pub fn newer_tag(tag: &str, current: &str) -> Option<String> {
    crate::version::is_newer(tag, current).then(|| {
        tag.trim()
            .strip_prefix('v')
            .unwrap_or(tag.trim())
            .to_owned()
    })
}

pub fn newer_from_json(body: &str, current: &str) -> Option<String> {
    newer_tag(&tag_from_json(body)?, current)
}

#[cfg(test)]
mod tests {
    #[test]
    fn t374_shared_release_response_contract() {
        for line in include_str!("../../testdata/release-responses.tsv").lines() {
            let parts: Vec<_> = line.split('\t').collect();
            let expected = (parts[2] != "-").then_some(parts[2]);
            assert_eq!(
                super::newer_from_json(parts[0], "1.2.3").as_deref(),
                expected,
                "T374: {line}"
            );
        }
    }
}
