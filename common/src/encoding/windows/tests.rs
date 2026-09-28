use super::*;

fn request(name: &str, format: Format) -> Request {
    Request {
        encoder: name.into(),
        format,
        fps: 60,
        bitrate: 20000,
        quality: 18,
        workers: 0,
    }
}
#[test]
fn t685_catalog_profiles_keep_hardware_out_of_live_admission() {
    for name in CANDIDATES {
        for format in [Format::Baseline, Format::Main, Format::High, Format::Main10] {
            let request = request(name, format);
            let compatible = if name.starts_with("hevc") {
                matches!(format, Format::Main | Format::Main10)
            } else {
                format != Format::Main10
            };
            assert_eq!(request.validate().is_ok(), compatible);
            if compatible {
                let options = request.options().unwrap();
                assert_eq!(options.iter().filter(|(k, _)| k == "-profile:v").count(), 1);
                assert!(options.contains(&("-g".into(), "60".into())));
            }
        }
    }
    assert!(super::super::find("h264_amf").is_none());
    assert!(super::super::find("hevc_qsv").is_none());
}
#[test]
fn t685_options_preserve_defaults_and_distinguish_capacity_from_quality() {
    let cpu = request("libx264", Format::Baseline).options().unwrap();
    for option in super::super::Profile::new("libx264", 60, 20000, 18)
        .unwrap()
        .cli_options(false)
    {
        assert!(cpu.contains(&option));
    }
    let mut manual = request("libx264", Format::High);
    manual.workers = 128;
    assert!(manual
        .options()
        .unwrap()
        .contains(&("-threads".into(), "128".into())));
    for name in ["h264_amf", "hevc_amf", "h264_qsv", "hevc_qsv"] {
        let options = request(name, Format::Main).options().unwrap();
        assert!(options.contains(&("-async_depth".into(), "1".into())));
        assert!(!options
            .iter()
            .any(|(k, _)| k == "-threads" || k == "-maxrate"));
        assert_eq!(
            options.iter().any(|(k, _)| k == "-look_ahead"),
            name == "h264_qsv"
        );
    }
}
#[test]
fn t685_bounded_invalid_requests_and_probe_commands() {
    for value in [
        0,
        1,
        9,
        10,
        12,
        32,
        60,
        90,
        91,
        128,
        129,
        1000,
        60000,
        60001,
        u32::MAX,
    ] {
        check_bounds(value);
    }
    let mut bad = request("h264_amf", Format::Main);
    bad.workers = 1;
    assert!(bad.validate().is_err());
    for name in ["", "unknown", "libx264 -x", "libx264\0", "h264_vaapi"] {
        assert!(request(name, Format::Main).options().is_err());
    }
    let good = request("libx264", Format::Baseline);
    let args = good.probe_arguments(640, 480, 65).unwrap();
    assert!(args
        .windows(2)
        .any(|a| a == ["-i", "testsrc2=size=640x480:rate=60"]));
    assert_eq!(&args[args.len() - 3..], ["-f", "h264", "pipe:1"]);
    for frames in 0..=256 {
        assert_eq!(
            good.probe_arguments(640, 480, frames).is_ok(),
            (2..=120).contains(&frames)
        );
    }
    for dimensions in [(0, 480), (640, 0), (641, 480), (640, 481), (u32::MAX, 480)] {
        assert!(good
            .probe_arguments(dimensions.0, dimensions.1, 65)
            .is_err());
    }
}
fn check_bounds(value: u32) {
    let mut r = request("libx264", Format::Baseline);
    r.fps = value;
    assert_eq!(
        r.validate().is_ok(),
        (crate::MIN_FPS..=crate::MAX_FPS).contains(&value)
    );
    r = request("libx264", Format::Baseline);
    r.bitrate = value;
    assert_eq!(
        r.validate().is_ok(),
        (crate::MIN_BITRATE_KBPS..=crate::MAX_BITRATE_KBPS).contains(&value)
    );
    r = request("libx264", Format::Baseline);
    r.quality = value;
    assert_eq!(
        r.validate().is_ok(),
        (crate::MIN_QUALITY..=crate::MAX_QUALITY).contains(&value)
    );
    r = request("libx264", Format::Baseline);
    r.workers = value;
    assert_eq!(r.validate().is_ok(), value <= 128);
}

#[cfg(all(windows, feature = "commands"))]
#[test]
fn t685_native_stock_ffmpeg_accepts_recipe_options() {
    use crate::commands::SyncCommandExt;
    use std::{process::Command, time::Duration};
    for name in CANDIDATES {
        let args = request(name, Format::Main)
            .probe_arguments(160, 120, 4)
            .unwrap();
        let output = Command::new("ffmpeg.exe")
            .args(args)
            .output_timeout(Duration::from_secs(8))
            .unwrap();
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!error.contains("Unrecognized option"), "{name}: {error}");
        assert!(
            !error.contains("Unable to parse option value"),
            "{name}: {error}"
        );
        if name == "libx264" {
            assert!(output.status.success(), "{error}");
        }
        if output.status.success() {
            assert!(!output.stdout.is_empty());
        }
        eprintln!(
            "T685 {name}: initialized={}, bytes={}, diagnostic={error}",
            output.status.success(),
            output.stdout.len()
        );
    }
}
