//! Portable repair policy for a device's `adb reverse --list` snapshot.
//! Native adapters execute the returned missing routes with --no-rebind.
use anyhow::{bail, ensure, Result};
use std::collections::HashMap;

fn parse(text: &str) -> Result<HashMap<&str, &str>> {
    ensure!(text.len() <= 65536, "ADB reverse listing exceeds 64 KiB");
    let mut mappings = HashMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(fields.len() == 3, "Malformed ADB reverse listing");
        ensure!(
            mappings.insert(fields[1], fields[2]).is_none(),
            "Duplicate ADB reverse endpoint"
        );
    }
    Ok(mappings)
}

/// Preserve matching routes and reject conflicting/ambiguous snapshots before
/// scheduling any mutation. A concurrent new owner is protected by --no-rebind.
pub fn missing(text: &str, expected: &[(u16, u16)]) -> Result<Vec<(u16, u16)>> {
    let mappings = parse(text)?;
    let mut missing = Vec::new();
    for &(remote, local) in expected {
        ensure!(
            remote != 0 && local != 0,
            "Repair requires fixed nonzero ports"
        );
        let source = format!("tcp:{remote}");
        match mappings.get(source.as_str()) {
            None => missing.push((remote, local)),
            Some(target) if **target == format!("tcp:{local}") => {}
            Some(_) => bail!("ADB reverse endpoint {source} belongs to a different destination"),
        }
    }
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t557_only_absent_routes_are_repaired_without_stealing_destinations() {
        let expected = [(8890, 9010), (8891, 9011)];
        assert_eq!(missing("", &expected).unwrap(), expected);
        let one = "UsbFfs tcp:8890 tcp:9010\nUsbFfs tcp:9999 localabstract:other\n";
        assert_eq!(missing(one, &expected).unwrap(), [(8891, 9011)]);
        let both = format!("{one}UsbFfs tcp:8891 tcp:9011\r\n\n");
        assert!(missing(&both, &expected).unwrap().is_empty());
        assert!(missing("UsbFfs tcp:8891 tcp:1111\n", &expected).is_err());
        assert!(missing(
            "UsbFfs tcp:8891 tcp:9011\nUsbFfs tcp:8891 tcp:9011",
            &expected
        )
        .is_err());
        assert!(missing("", &[(0, 1)]).is_err());
        assert!(missing("", &[(1, 0)]).is_err());
    }

    #[test]
    fn t557_bounded_malformed_listing_fuzz_fails_closed() {
        let expected = [(8890, 9010), (8891, 9011)];
        for fields in 1..12 {
            let text = "field ".repeat(fields);
            assert_eq!(missing(&text, &expected).is_ok(), fields == 3);
        }
        assert!(missing(&" ".repeat(65536), &expected).is_ok());
        assert!(missing(&" ".repeat(65537), &expected).is_err());
        for port in [1, 1024, 8890, 65535] {
            let text = format!("UsbFfs tcp:{port} tcp:{port}\n");
            assert!(missing(&text, &[(port, port)]).unwrap().is_empty());
            assert_eq!(missing("", &[(port, port)]).unwrap(), [(port, port)]);
        }
    }
}
