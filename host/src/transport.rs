//! T691: shared transport preference and Android address parsing.
use blent_config::adb::{transport_of, Transport};
pub fn parse_tablet_ip(text: &str) -> Option<String> {
    // "inet 192.168.1.42/24 …" or "… src 192.168.1.42 …"
    let mut words = text.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w != "inet" && w != "src" {
            continue;
        }
        let Some(value) = words.peek() else { continue };
        let ip = value.split('/').next().unwrap_or(value);
        if let Ok(address) = ip.parse::<std::net::Ipv4Addr>() {
            if !address.is_loopback() {
                return Some(address.to_string());
            }
        }
    }
    None
}

pub fn attachment_identity(
    serial: &str,
    identities: &std::collections::HashMap<String, String>,
) -> String {
    match identities.get(serial) {
        Some(identity) => format!("device:{identity}"),
        None => format!("transport:{serial}"),
    }
}

pub fn current_transport(
    devices: &[String],
    current: Option<&str>,
    identities: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let current = current?;
    if let Some(identity) = identities.get(current) {
        devices
            .iter()
            .find(|device| identities.get(*device) == Some(identity))
            .cloned()
    } else {
        devices
            .iter()
            .find(|device| device.as_str() == current)
            .cloned()
    }
}

pub fn select_device_transports(
    devices: &[String],
    current: Option<&str>,
    identities: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut selected: Vec<String> = Vec::new();
    let mut groups = std::collections::HashMap::<String, usize>::new();
    for serial in devices {
        // Unknown identities remain distinct; never merge unrelated tablets on
        // an empty or failed getprop response.
        let identity = identities
            .get(serial)
            .map(|id| format!("device:{id}"))
            .unwrap_or_else(|| format!("transport:{serial}"));
        if let Some(&index) = groups.get(&identity) {
            let existing = transport_of(&selected[index]);
            let candidate = transport_of(serial);
            if (candidate == Transport::Usb && existing == Transport::Network)
                || (candidate == existing && Some(serial.as_str()) == current)
            {
                selected[index] = serial.clone();
            }
        } else {
            groups.insert(identity, selected.len());
            selected.push(serial.clone());
        }
    }
    selected
}
