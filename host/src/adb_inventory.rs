//! T443: only a successful, well-formed ADB listing confirms disappearance.
use blent_config::adb::{transport_of, Transport};
use blent_config::commands::AsyncCommandExt;

pub async fn query(adb: &str) -> Option<Vec<String>> {
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

pub fn with_synthetic(
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

pub fn parse(text: &str) -> Option<Vec<String>> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines.next()? != "List of devices attached" {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut ready = Vec::new();
    for line in lines {
        let (serial, state) = line.split_once('\t')?;
        if serial.is_empty() || serial.contains('\0') || !known_state(state) || !seen.insert(serial)
        {
            return None;
        }
        if state == "device" {
            ready.push(serial.to_owned());
        }
    }
    ready.sort_by_key(|serial| transport_of(serial) != Transport::Usb);
    Some(ready)
}

pub fn package_presence(out: &std::process::Output) -> Option<bool> {
    if !out.stderr.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?.trim();
    // Android PackageManagerShellCommand.displayPackageFilePath returns 1
    // with empty output when absent. Older adapters also return 0/empty.
    if text.is_empty() && matches!(out.status.code(), Some(0 | 1)) {
        return Some(false);
    }
    if out.status.success() && text.lines().all(|line| line.starts_with("package:/")) {
        return Some(true);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t650_native_argument_nul_never_confirms_inventory() {
        for position in 0..=64 {
            let mut serial = "x".repeat(64);
            serial.insert(position, '\0');
            let inventory = format!("List of devices attached\n{serial}\tdevice\n");
            assert_eq!(
                parse(&inventory),
                None,
                "T650: NUL at serial offset {position}"
            );
        }
        assert_eq!(
            parse("List of devices attached\nUSB-café\tdevice\n"),
            Some(vec!["USB-café".into()])
        );
    }

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
