//! T559: semantic TOML paths keep repeated section keys distinct.
use std::collections::{BTreeMap, BTreeSet};
use toml::Value;

type Fields = BTreeMap<Vec<String>, Value>;

pub(super) fn changes(before: &str, after: &str) -> Vec<String> {
    let (Ok(before), Ok(after)) = (fields(before), fields(after)) else {
        return Vec::new();
    };
    let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .map(|key| {
            let path = key
                .iter()
                .map(|part| display_key(part))
                .collect::<Vec<_>>()
                .join(".");
            format!(
                "{path} = {} (was {})",
                display_value(after.get(key)),
                display_value(before.get(key))
            )
        })
        .collect()
}

fn fields(text: &str) -> Result<Fields, toml::de::Error> {
    let value: Value = toml::from_str(text)?;
    let mut result = Fields::new();
    flatten(&value, &mut Vec::new(), &mut result);
    Ok(result)
}

fn flatten(value: &Value, path: &mut Vec<String>, output: &mut Fields) {
    if let Value::Table(table) = value {
        for (key, value) in table {
            path.push(key.clone());
            flatten(value, path, output);
            path.pop();
        }
    } else {
        output.insert(path.clone(), value.clone());
    }
}

fn display_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        key.to_owned()
    } else {
        Value::String(key.to_owned()).to_string()
    }
}

fn display_value(value: Option<&Value>) -> String {
    value
        .map(ToString::to_string)
        .unwrap_or_else(|| "<unset>".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t559_diffs_semantic_paths_and_added_removed_values() {
        assert!(changes("width=2\n[camera]\nwidth=4", "camera.width = 4\nwidth = 2").is_empty());
        assert_eq!(
            changes("[camera]\nwidth=4", "[camera]\nwidth=6"),
            ["camera.width = 6 (was 4)"]
        );
        assert_eq!(
            changes("a=1", "b=[2,3]"),
            ["a = <unset> (was 1)", "b = [2, 3] (was <unset>)"]
        );
        assert_eq!(
            changes("'a.b'=1\n[a]\nb=2", "'a.b'=3\n[a]\nb=2"),
            ["\"a.b\" = 3 (was 1)"]
        );
        assert_eq!(display_key(""), "\"\"");
        assert_eq!(display_key("A_-09"), "A_-09");
    }

    #[test]
    fn t559_malformed_and_bounded_nested_input_never_fabricates_changes() {
        for text in ["[", "a =", "a=1\na=2", "a='unterminated", "a=[[[", "\0"] {
            assert!(changes(text, "x=1").is_empty());
            assert!(changes("x=1", text).is_empty());
        }
        for depth in 1..=32 {
            let key = vec!["nested"; depth].join(".");
            let before = format!("{key}=0");
            let after = format!("{key}=1");
            assert_eq!(changes(&before, &after), [format!("{key} = 1 (was 0)")]);
        }
    }
}
