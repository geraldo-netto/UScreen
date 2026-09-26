//! T443: only a successful, well-formed ADB listing confirms disappearance.
use blent_config::adb::{transport_of, Transport};
use blent_config::commands::AsyncCommandExt;

pub(crate) async fn query(adb: &str) -> Option<Vec<String>> {
    let output = tokio::process::Command::new(adb)
        .arg("devices")
        .output_bounded()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse(std::str::from_utf8(&output.stdout).ok()?)
}

pub(crate) fn with_synthetic(
    confirmed: Option<Vec<String>>,
    previous: &[String],
    synthetic: Vec<String>,
) -> Option<Vec<String>> {
    let mut devices = confirmed.or_else(|| (!synthetic.is_empty()).then(|| previous.to_vec()))?;
    for serial in synthetic {
        if !devices.contains(&serial) {
            devices.push(serial);
        }
    }
    Some(devices)
}

fn known_state(state: &str) -> bool {
    // ADB's short listing uses a tab before the complete connection state.
    // Missing udev access includes a diagnostic after "no permissions".
    state.starts_with("no permissions")
        || [
            "device",
            "offline",
            "unauthorized",
            "authorizing",
            "connecting",
            "bootloader",
            "recovery",
            "sideload",
            "rescue",
            "host",
            "unknown",
        ]
        .contains(&state)
}

fn parse(text: &str) -> Option<Vec<String>> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next()? != "List of devices attached" {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut ready = Vec::new();
    for line in lines {
        let (serial, state) = line.split_once('\t')?;
        if serial.is_empty() || !known_state(state) || !seen.insert(serial) {
            return None;
        }
        if state == "device" {
            ready.push(serial.to_owned());
        }
    }
    ready.sort_by_key(|serial| transport_of(serial) != Transport::Usb);
    Some(ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t443_synthetic_devices_work_without_inventing_real_disconnects() {
        let previous = vec!["REAL".into()];
        assert_eq!(with_synthetic(None, &previous, vec![]), None);
        assert_eq!(
            with_synthetic(None, &previous, vec!["fake1".into()]),
            Some(vec!["REAL".into(), "fake1".into()])
        );
        assert_eq!(
            with_synthetic(Some(vec![]), &previous, vec!["fake1".into()]),
            Some(vec!["fake1".into()])
        );
    }

    #[test]
    fn t443_inventory_parser_distinguishes_empty_unknown_and_nonready() {
        assert_eq!(parse("List of devices attached\n\n"), Some(vec![]));
        assert_eq!(parse("List of devices attached\nnet:5555\tdevice\nUSB\tdevice\nOFF\toffline\nNO\tno permissions (udev rules)\n"),
                   Some(vec!["USB".into(), "net:5555".into()]));
        for malformed in [
            "",
            "error",
            "List of devices attached\npartial",
            "List of devices attached\nUSB\tbogus",
            "List of devices attached\nUSB\tdevice\nUSB\toffline",
        ] {
            assert_eq!(parse(malformed), None, "T443: {malformed}");
        }
    }
}
