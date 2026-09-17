//! Transport classification for serial forms emitted by supported ADB listings.
//! This identifies ADB routing, not the tablet's physical radio or charging state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transport {
    Usb,
    Network,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Network => "Network ADB",
        }
    }
}

/// ADB socket endpoints include a colon (including IPv6). Its discovery path also
/// registers mDNS instance/service names without a port. Plain device serials
/// retain the existing USB classification; never infer physical device identity
/// from a network service's instance-name prefix.
/// Sources: AOSP adb_mdns.h, client/mdns_utils.cpp and client/transport_mdns.cpp.
pub fn transport_of(serial: &str) -> Transport {
    if serial.contains(':') || is_mdns_serial(serial) {
        Transport::Network
    } else {
        Transport::Usb
    }
}

fn is_mdns_serial(serial: &str) -> bool {
    let serial = serial.to_ascii_lowercase();
    let serial = serial.strip_suffix('.').unwrap_or(&serial);
    let serial = serial.strip_suffix(".local").unwrap_or(serial);
    [
        "._adb._tcp",
        "._adb-tls-connect._tcp",
        "._adb-tls-pairing._tcp",
    ]
    .iter()
    .any(|service| serial.ends_with(service))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t278_mdns_service_and_domain_forms_are_network_routes() {
        for service in ["_adb", "_adb-tls-connect", "_adb-tls-pairing"] {
            for suffix in ["", ".", ".local", ".local."] {
                let serial = format!("instance.{service}._tcp{suffix}");
                assert_eq!(transport_of(&serial), Transport::Network, "T278: {serial}");
                assert_eq!(
                    transport_of(&serial.to_ascii_uppercase()),
                    Transport::Network
                );
            }
        }
        for plain in [
            "8002RH1010011900",
            "UNKNOWN_A",
            "adb-USB_SERIAL",
            "prefix_adb-tls-connect",
        ] {
            assert_eq!(transport_of(plain), Transport::Usb);
        }
    }
}
