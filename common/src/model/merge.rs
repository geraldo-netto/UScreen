//! Three-way field merging; table leaves are independent, arrays are atomic.
use std::collections::BTreeSet;
use toml::{Table, Value};

pub(super) fn tables(edited: &Table, baseline: &Table, latest: &mut Table) {
    let keys: BTreeSet<_> = edited.keys().chain(baseline.keys()).collect();
    for key in keys {
        if edited.get(key) == baseline.get(key) {
            continue;
        }
        match (edited.get(key), baseline.get(key), latest.get_mut(key)) {
            (Some(Value::Table(edit)), Some(Value::Table(base)), Some(Value::Table(current))) => {
                tables(edit, base, current);
            }
            (Some(value), _, _) => {
                latest.insert(key.clone(), value.clone());
            }
            (None, _, _) => {
                latest.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t703_nested_fields_and_atomic_arrays_preserve_unedited_latest_values() {
        for depth in 1..=32 {
            let prefix = vec!["nested"; depth].join(".");
            let base: Table = toml::from_str(&format!("{prefix}.a=0\n{prefix}.b=0")).unwrap();
            let edit: Table = toml::from_str(&format!("{prefix}.a=-1\n{prefix}.b=0")).unwrap();
            let mut latest: Table =
                toml::from_str(&format!("{prefix}.a=0\n{prefix}.b=9223372036854775807")).unwrap();
            tables(&edit, &base, &mut latest);
            let expected: Table =
                toml::from_str(&format!("{prefix}.a=-1\n{prefix}.b=9223372036854775807")).unwrap();
            assert_eq!(latest, expected);
        }
        let base: Table = toml::from_str("a=[1,2]\nb=0").unwrap();
        let edit: Table = toml::from_str("a=[3]\nb=0").unwrap();
        let mut latest: Table = toml::from_str("a=[4,5]\nb=1\nnew=true").unwrap();
        tables(&edit, &base, &mut latest);
        assert_eq!(
            latest,
            toml::from_str::<Table>("a=[3]\nb=1\nnew=true").unwrap()
        );
    }

    #[test]
    fn t703_changed_value_types_are_atomic_and_absent_removal_is_safe() {
        for value in ["1", "'text'", "[]", "{nested=1}", "false"] {
            let base: Table = toml::from_str("a=0\nremoved=1").unwrap();
            let edit: Table = toml::from_str(&format!("a={value}")).unwrap();
            let mut latest: Table = toml::from_str("other=true").unwrap();
            tables(&edit, &base, &mut latest);
            assert_eq!(latest.get("a"), edit.get("a"));
            assert_eq!(latest.get("other"), Some(&Value::Boolean(true)));
            assert!(!latest.contains_key("removed"));
        }
    }
}
