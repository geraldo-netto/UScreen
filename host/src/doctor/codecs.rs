//! Read-only codec diagnostics: requested policy, observed child and inventory
//! are different evidence. None establishes successful tablet decoding.
use super::{FileConfig, Level, Report};
use crate::media::Codec;
use std::ffi::OsString;

pub(super) fn report_codec(r: &mut Report, cfg: &FileConfig, output: Option<&str>) {
    if cfg.encoder != "auto" && blent_config::encoding::find(&cfg.encoder).is_none() {
        r.line(
            Level::Warn,
            "tablet codec",
            "unknown encoder; requested codec family cannot be determined",
        );
        return;
    }
    let requested = Codec::from_encoder(&cfg.encoder);
    for codec in Codec::ALL {
        if cfg.encoder == "auto" || codec == requested {
            let required = cfg.encoder != "auto" || codec == Codec::H264;
            report_family(r, cfg, codec, inventory(output, codec), required);
        }
    }
    r.hint("Decoder inventory is not a decode test or speed measurement; exact stream compatibility is negotiated by the connected app.");
}

fn report_family(
    r: &mut Report,
    cfg: &FileConfig,
    codec: Codec,
    report: Option<&str>,
    required: bool,
) {
    let name = codec.label();
    let entries: Vec<_> = report.unwrap_or("").split(',').collect();
    if report == Some("none") {
        r.line(
            if required { Level::Fail } else { Level::Ok },
            "tablet codec",
            &format!("no {name} decoder reported by MediaCodecList"),
        );
    } else if !entries.iter().all(|entry| valid_entry(codec, entry)) {
        r.line(
            Level::Warn,
            "tablet codec",
            &format!("{name} capability unknown (no valid decoder inventory)"),
        );
        r.hint("install the current tablet APK, then rerun doctor; legacy inventories cannot establish support for this codec");
    } else if needs_main10(cfg, codec) && !entries.iter().any(|entry| entry.ends_with("10")) {
        r.line(
            Level::Fail,
            "tablet codec",
            "HEVC Main10 not reported; requested 10-bit stream is unsupported by this inventory",
        );
    } else {
        usable(r, cfg, codec, &entries);
    }
}

fn valid_entry(codec: Codec, entry: &str) -> bool {
    if codec == Codec::Hevc {
        matches!(
            entry,
            "hw8" | "hw10" | "sw8" | "sw10" | "unknown8" | "unknown10"
        )
    } else {
        matches!(entry, "hw" | "sw" | "unclassified")
    }
}
fn needs_main10(cfg: &FileConfig, codec: Codec) -> bool {
    codec == Codec::Hevc && cfg.ten_bit && cfg.encoder != "auto"
}

fn usable(r: &mut Report, cfg: &FileConfig, codec: Codec, entries: &[&str]) {
    let entries = entries
        .iter()
        .copied()
        .filter(|entry| !needs_main10(cfg, codec) || entry.ends_with("10"))
        .collect::<Vec<_>>();
    let name = codec.label();
    if entries.iter().any(|entry| entry.starts_with("hw")) {
        r.line(Level::Ok, "tablet codec", &hardware_label(codec, &entries));
    } else if entries
        .iter()
        .any(|entry| entry.starts_with("unknown") || *entry == "unclassified")
    {
        r.line(
            Level::Warn,
            "tablet codec",
            &format!("{name} decoder reported; acceleration unknown"),
        );
    } else {
        r.line(
            Level::Warn,
            "tablet codec",
            &format!("{name} software decoder only; real-time performance is not guaranteed"),
        );
    }
}

fn hardware_label(codec: Codec, entries: &[&str]) -> String {
    let depth = if codec == Codec::Hevc && entries.contains(&"hw10") {
        " Main10"
    } else {
        ""
    };
    format!("hardware {}{depth} reported by Android", codec.label())
}

fn payload<'a>(output: Option<&'a str>, prefix: &str) -> Option<&'a str> {
    output?
        .split_once(prefix)
        .map(|(_, tail)| tail.split(['"', '\r', '\n']).next().unwrap_or("").trim())
}
fn inventory(output: Option<&str>, codec: Codec) -> Option<&str> {
    if let Some(text) = payload(output, "BLENT_CODECS_V2:") {
        let mut matches = text
            .split(';')
            .filter_map(|entry| entry.split_once('='))
            .filter(|(name, _)| *name == codec.wire_name());
        let (_, value) = matches.next()?;
        return matches.next().is_none().then_some(value);
    }
    if codec == Codec::Hevc {
        payload(output, "BLENT_CODECS_V1:")
    } else {
        None
    }
}

pub(super) async fn report_live_encoder(r: &mut Report, cfg: &FileConfig, instance: u32) {
    let observed = tokio::task::spawn_blocking(move || {
        let fifo = super::fifo_path_for(instance).ok()?;
        let inventory = super::processes::same_user_processes().ok()?;
        let pids = super::encoders_for_fifo(&inventory, &fifo);
        if pids.len() != 1 {
            return None;
        }
        let encoder = inventory.iter().find(|p| p.pid == pids[0])?;
        observed_profile(&encoder.arguments).map(str::to_owned)
    })
    .await
    .ok()
    .flatten();
    r.line(
        Level::Ok,
        "encoder selection",
        &format!(
            "requested {}; observed running child {} (decoding unverified)",
            cfg.encoder,
            observed.as_deref().unwrap_or("unknown")
        ),
    );
}

fn observed_profile(args: &[OsString]) -> Option<&str> {
    let name = argument(args, "-c:v")?;
    blent_config::encoding::find(name)?;
    if name == "h264_vaapi" && argument(args, "-profile:v") == Some("constrained_baseline") {
        Some("h264_vaapi_baseline")
    } else {
        Some(name)
    }
}
fn argument<'a>(args: &'a [OsString], key: &str) -> Option<&'a str> {
    let mut matches = args.windows(2).filter(|pair| pair[0] == key);
    let value = matches.next()?[1].to_str()?;
    matches.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t438_auto_inventory_does_not_require_every_optional_codec() {
        let mut report = Report::new();
        report_codec(
            &mut report,
            &FileConfig::default(),
            Some("BLENT_CODECS_V2:h264=hw;hevc=none;vp9=none;av1=none"),
        );
        assert_eq!((report.warnings, report.failures), (0, 0));
    }

    #[test]
    fn t438_unknown_encoder_does_not_claim_an_h264_selection() {
        let mut report = Report::new();
        let cfg = FileConfig {
            encoder: "unrecognized-encoder".into(),
            ..Default::default()
        };
        report_codec(&mut report, &cfg, Some("BLENT_CODECS_V2:h264=hw"));
        let text = report.messages.borrow().join("\n");
        assert!(text.contains("unknown encoder"), "T438: {text}");
        assert!(!text.contains("H.264"), "T438: {text}");
    }
    #[test]
    fn t438_observed_profiles_and_duplicate_inventory_are_not_guessed() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            observed_profile(&args(&[
                "-c:v",
                "h264_vaapi",
                "-profile:v",
                "constrained_baseline"
            ])),
            Some("h264_vaapi_baseline")
        );
        assert_eq!(
            observed_profile(&args(&["-c:v", "libaom-av1"])),
            Some("libaom-av1")
        );
        assert_eq!(
            observed_profile(&args(&["-c:v", "libaom-av1", "-c:v", "libx264"])),
            None
        );
        assert_eq!(observed_profile(&args(&["-c:v", "unknown"])), None);
        assert_eq!(
            inventory(Some("BLENT_CODECS_V2:vp9=hw;vp9=none"), Codec::Vp9),
            None
        );
        assert_eq!(inventory(Some("BLENT_CODECS_V1:hw10"), Codec::Av1), None);
    }
}
