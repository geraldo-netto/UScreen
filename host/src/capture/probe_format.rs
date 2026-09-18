//! Inspect a bounded sample of stock encoder output; wrapper defaults are not proof.
use crate::media::{Codec, VideoPacket};
use anyhow::{ensure, Context, Result};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use uscreen_config::negotiation::{Profile, StreamProfile};

pub(super) async fn inspect(
    codec: Codec,
    width: u32,
    height: u32,
    frame: &VideoPacket,
) -> Result<StreamProfile> {
    let bytes = sample(codec, width, height, frame);
    if codec == Codec::Hevc {
        ensure!(hevc_main_tier(&bytes), "Unknown or unsupported HEVC tier");
    }
    tokio::time::timeout(Duration::from_secs(2), query(codec, width, height, &bytes))
        .await
        .context("Format probe deadline")?
}

fn hevc_main_tier(bytes: &[u8]) -> bool {
    let mut seen = false;
    for (_, offset) in crate::encoder_io::annex_b_offsets(bytes) {
        if bytes
            .get(offset)
            .is_none_or(|header| (header >> 1) & 63 != 33)
        {
            continue;
        }
        // SPS: two-byte NAL header, one byte of VPS/sublayer/nesting fields,
        // then profile_space (2), general_tier_flag (1), profile_idc (5).
        // No emulation-prevention byte can precede this second RBSP byte.
        let Some(profile) = bytes.get(offset + 3) else {
            return false;
        };
        if profile & 0x20 != 0 {
            return false;
        }
        seen = true;
    }
    seen
}

pub(super) fn sample(codec: Codec, width: u32, height: u32, frame: &VideoPacket) -> Vec<u8> {
    let mut bytes = if codec.framed() {
        let mut header = b"DKIF\0\0\x20\0".to_vec();
        header.extend_from_slice(if codec == Codec::Vp9 {
            b"VP90"
        } else {
            b"AV01"
        });
        header.extend_from_slice(&(width as u16).to_le_bytes());
        header.extend_from_slice(&(height as u16).to_le_bytes());
        header.extend_from_slice(&60u32.to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&(frame.data.len() as u32).to_le_bytes());
        header.extend_from_slice(&0u64.to_le_bytes());
        header
    } else {
        frame
            .codec_config
            .as_ref()
            .map(|c| c.to_vec())
            .unwrap_or_default()
    };
    bytes.extend_from_slice(&frame.data);
    bytes
}

async fn query(codec: Codec, width: u32, height: u32, bytes: &[u8]) -> Result<StreamProfile> {
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-f",
            codec.muxer(),
            "-show_entries",
            "stream=codec_name,profile,level,pix_fmt,width,height",
            "-of",
            "json",
            "pipe:0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let write = async {
        input.write_all(bytes).await?;
        input.shutdown().await?;
        drop(input);
        Ok::<_, anyhow::Error>(())
    };
    let read = async {
        let mut data = Vec::new();
        output.take(65537).read_to_end(&mut data).await?;
        ensure!(data.len() <= 65536, "Oversized format probe");
        Ok::<_, anyhow::Error>(data)
    };
    let ((), data) = tokio::try_join!(write, read)?;
    ensure!(child.wait().await?.success(), "Format probe failed");
    parse(&data, codec, width, height)
}

fn parse(data: &[u8], codec: Codec, width: u32, height: u32) -> Result<StreamProfile> {
    let report: serde_json::Value = serde_json::from_slice(data)?;
    let streams = report["streams"].as_array().context("Missing streams")?;
    ensure!(streams.len() == 1, "Ambiguous probe streams");
    let stream = &streams[0];
    ensure!(
        stream["codec_name"].as_str() == Some(codec.wire_name()),
        "Unexpected codec"
    );
    ensure!(
        stream["width"] == width && stream["height"] == height,
        "Unexpected probe geometry"
    );
    let profile = profile(
        codec,
        stream["profile"].as_str().context("Missing profile")?,
    )
    .context("Unknown profile")?;
    let depth = match stream["pix_fmt"].as_str() {
        Some("yuv420p" | "nv12") => 8,
        Some("yuv420p10le" | "p010le") => 10,
        _ => anyhow::bail!("Unsupported pixel format"),
    };
    let level = level(codec, stream["level"].as_u64().context("Missing level")?)
        .context("Unknown level")?;
    let observed = StreamProfile {
        codec: codec.wire_name().into(),
        format: Profile {
            profile: profile.into(),
            depth,
            level,
        },
    };
    ensure!(observed.valid(), "Unsupported stream profile");
    Ok(observed)
}

fn profile(codec: Codec, name: &str) -> Option<&'static str> {
    let choices: &[(&str, &str)] = match codec {
        Codec::H264 => &[
            ("Constrained Baseline", "constrained-baseline"),
            ("Baseline", "baseline"),
            ("Main", "main"),
            ("High", "high"),
        ],
        Codec::Hevc => &[("Main", "main"), ("Main 10", "main10")],
        Codec::Vp9 => &[("Profile 0", "profile0"), ("Profile 2", "profile2")],
        Codec::Av1 => &[("Main", "main")],
    };
    choices
        .iter()
        .find(|(source, _)| *source == name)
        .map(|(_, wire)| *wire)
}

fn level(codec: Codec, value: u64) -> Option<u32> {
    let value = u32::try_from(value).ok()?;
    match codec {
        Codec::Hevc => (value % 3 == 0).then_some(value / 3),
        Codec::Av1 => (value < 24).then_some(20 + (value / 4) * 10 + value % 4),
        _ => Some(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t478_hevc_level_cannot_silently_treat_high_tier_as_main() {
        let main = [0, 0, 0, 1, 0x42, 1, 1, 1, 0x60];
        assert!(hevc_main_tier(&main));
        let mut high = main;
        high[7] |= 0x20;
        assert!(!hevc_main_tier(&high));
        assert!(!hevc_main_tier(&main[..7]));
        assert!(!hevc_main_tier(&[]));
        let mixed = [main.as_slice(), high.as_slice()].concat();
        assert!(!hevc_main_tier(&mixed));
    }
    #[test]
    fn t478_unknown_mismatched_and_non_420_output_cannot_authorize_profiles() {
        let good = serde_json::json!({"streams":[{"codec_name":"h264","width":640,"height":480,"profile":"Constrained Baseline","level":31,"pix_fmt":"yuv420p"}]});
        assert_eq!(
            parse(&serde_json::to_vec(&good).unwrap(), Codec::H264, 640, 480)
                .unwrap()
                .format
                .depth,
            8
        );
        for (field, value) in [
            ("profile", "unknown"),
            ("pix_fmt", "yuv444p"),
            ("codec_name", "hevc"),
        ] {
            let mut bad = good.clone();
            bad["streams"][0][field] = value.into();
            assert!(parse(&serde_json::to_vec(&bad).unwrap(), Codec::H264, 640, 480).is_err());
        }
        assert!(parse(&serde_json::to_vec(&good).unwrap(), Codec::H264, 1280, 800).is_err());
    }
}
