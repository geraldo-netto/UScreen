//! T577: Xorg provider names are not DRM connector identities.
use super::*;
use crate::vdisplay::EvdiConnector;

pub(crate) fn edid(width: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 128];
    bytes[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
    bytes[12..16].copy_from_slice(&width.to_le_bytes());
    bytes[127] = 0u8.wrapping_sub(
        bytes[..127]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
    );
    bytes
}

pub(crate) fn output(name: &str, primary: bool, bytes: &[u8]) -> String {
    let mut text = format!(
        "{name} connected {}1280x800+0+0\n\tEDID:\n",
        if primary { "primary " } else { "" }
    );
    for row in bytes.chunks(16) {
        text.push_str("\t\t");
        for byte in row {
            text.push_str(&format!("{byte:02x}"));
        }
        text.push('\n');
    }
    text
}

fn connector(card: u32, name: &str, width: u32) -> EvdiConnector {
    EvdiConnector {
        card,
        name: name.into(),
        connected: true,
        edid: edid(width),
    }
}

#[test]
fn t577_provider_renaming_cannot_assign_another_tablets_output() {
    let connectors = [connector(1, "DVI-I-1", 1280), connector(2, "DVI-I-2", 1200)];
    let text = output("DVI-I-2-1", false, &connectors[0].edid)
        + &output("DVI-I-3-2", false, &connectors[1].edid);
    let active = x11_active_outputs(&text);
    assert_eq!(
        x11_target_output(false, &active, &connectors, Some(1)),
        Some("DVI-I-2-1")
    );
    assert_eq!(
        x11_target_output(false, &active, &connectors, Some(2)),
        Some("DVI-I-3-2")
    );
}

#[test]
fn t577_missing_invalid_and_ambiguous_identity_never_falls_back_to_names() {
    let connectors = [connector(1, "DVI-I-1", 1280)];
    for text in [
        "DVI-I-1 connected 1280x800+0+0\n".to_owned(),
        output("DVI-I-1", false, &[0; 128]),
        output("DVI-I-1", false, &edid(1200)),
        "DVI-I-1 disconnected\n".into(),
        "DVI-I-1 connected\n".into(),
    ] {
        assert!(
            x11_target_output(false, &x11_active_outputs(&text), &connectors, Some(1)).is_none()
        );
    }
    let text = output("DVI-I-2-1", false, &edid(1280));
    let active = x11_active_outputs(&text);
    assert!(x11_target_output(false, &active, &connectors, Some(2)).is_none());
    assert_eq!(
        x11_target_output(false, &active, &connectors, None),
        Some("DVI-I-2-1")
    );
    let same = [connector(1, "DVI-I-1", 1280), connector(2, "DVI-I-2", 1280)];
    assert!(x11_target_output(false, &active, &same, Some(1)).is_none());
    let duplicates = text.clone() + &output("DVI-I-3-2", false, &edid(1280));
    assert!(x11_target_output(
        false,
        &x11_active_outputs(&duplicates),
        &connectors,
        Some(1)
    )
    .is_none());
    let mut disconnected = connectors;
    disconnected[0].connected = false;
    assert!(x11_target_output(false, &active, &disconnected, Some(1)).is_none());
}

#[test]
fn t577_physical_mode_excludes_renamed_virtual_outputs_and_unknown_owners() {
    let mut connectors = [connector(1, "DVI-I-1", 1280)];
    let text = output("DVI-I-2-1", true, &edid(1280))
        + &output("HDMI-A-1", false, &edid(1920))
        + &output("eDP-1", true, &edid(1600));
    let active = x11_active_outputs(&text);
    assert_eq!(
        x11_target_output(true, &active, &connectors, None),
        Some("eDP-1")
    );
    connectors[0].edid.clear();
    assert!(x11_target_output(true, &active, &connectors, None).is_none());
    assert!(x11_target_output(true, &[], &[], None).is_none());
}

#[test]
fn t577_edid_parser_rejects_bounded_invalid_lengths_and_checksums() {
    let connectors = [connector(1, "DVI-I-1", 1280)];
    for length in 0..=640 {
        let bytes = vec![0xff; length];
        let text = output("DVI-I-2-1", false, &bytes);
        assert!(
            x11_target_output(false, &x11_active_outputs(&text), &connectors, Some(1)).is_none()
        );
    }
    for offset in 0..128 {
        let mut bytes = edid(1280);
        bytes[offset] ^= 1;
        let text = output("DVI-I-2-1", false, &bytes);
        assert!(
            x11_target_output(false, &x11_active_outputs(&text), &connectors, Some(1)).is_none()
        );
    }
    for marker in [
        "EDID:\n\t\tzzzz",
        "EDID:\n\t\t🦀",
        "OTHER:\n\t\t00000000000000000000000000000000",
    ] {
        let text = format!("DVI-I-2-1 connected 1280x800+0+0\n\t{marker}\n");
        assert!(
            x11_target_output(false, &x11_active_outputs(&text), &connectors, Some(1)).is_none()
        );
    }
}
