//! Match DRM ownership to RandR EDID, never Xorg's provider-dependent names.
use crate::vdisplay::EvdiConnector;

pub(super) struct Output<'a> {
    pub name: &'a str,
    primary: bool,
    edid: Vec<u8>,
}

fn valid_edid(bytes: &[u8]) -> bool {
    (128..=512).contains(&bytes.len())
        && bytes.len().is_multiple_of(128)
        && bytes.starts_with(&[0, 255, 255, 255, 255, 255, 255, 0])
        && (usize::from(bytes[126]) + 1) * 128 == bytes.len()
        && bytes
            .chunks_exact(128)
            .all(|block| block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) == 0)
}

fn property_edid(lines: &[&str]) -> Vec<u8> {
    let mut rows = lines.iter().skip_while(|line| line.trim() != "EDID:");
    rows.next();
    let text: String = rows
        .map(|line| line.trim())
        .take_while(|line| line.len() == 32 && line.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .take(33)
        .collect();
    let bytes: Vec<_> = text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    if valid_edid(&bytes) {
        bytes
    } else {
        Vec::new()
    }
}

pub(super) fn x11_active_outputs(text: &str) -> Vec<Output<'_>> {
    let lines: Vec<_> = text.lines().collect();
    let mut outputs = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.get(1) != Some(&"connected")
            || !fields.iter().any(|field| super::x11_has_geometry(field))
        {
            continue;
        }
        let end = lines[index + 1..]
            .iter()
            .position(|line| !line.starts_with(char::is_whitespace))
            .map_or(lines.len(), |offset| index + 1 + offset);
        outputs.push(Output {
            name: fields[0],
            primary: fields.contains(&"primary"),
            edid: property_edid(&lines[index + 1..end]),
        });
    }
    outputs
}

fn matching_connectors<'a>(
    output: &Output<'_>,
    connectors: &'a [EvdiConnector],
) -> Vec<&'a EvdiConnector> {
    if !valid_edid(&output.edid) {
        return Vec::new();
    }
    connectors
        .iter()
        .filter(|connector| connector.connected && connector.edid == output.edid)
        .collect()
}

pub(super) fn x11_target_output<'a>(
    pen_only: bool,
    active: &[Output<'a>],
    connectors: &[EvdiConnector],
    card: Option<u32>,
) -> Option<&'a str> {
    if pen_only {
        return physical_output(active, connectors);
    }
    let candidates: Vec<_> = active
        .iter()
        .filter(|output| {
            let owners = matching_connectors(output, connectors);
            owners.len() == 1 && card.is_none_or(|card| owners[0].card == card)
        })
        .collect();
    (candidates.len() == 1).then(|| candidates[0].name)
}

fn physical_output<'a>(active: &[Output<'a>], connectors: &[EvdiConnector]) -> Option<&'a str> {
    // Missing identity cannot prove a connected virtual output is physical.
    if connectors
        .iter()
        .any(|connector| connector.connected && !valid_edid(&connector.edid))
    {
        return None;
    }
    active
        .iter()
        .filter(|output| {
            valid_edid(&output.edid) && matching_connectors(output, connectors).is_empty()
        })
        .max_by_key(|output| output.primary)
        .map(|output| output.name)
}
