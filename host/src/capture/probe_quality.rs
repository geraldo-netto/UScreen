//! A cheap synthetic first-frame fidelity gate, not a perceptual quality claim.
use crate::media::{Codec, VideoPacket};
use anyhow::{ensure, Context, Result};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub(super) async fn inspect(
    codec: Codec,
    width: u32,
    height: u32,
    frame: &VideoPacket,
) -> Result<f64> {
    let bytes = super::probe_format::sample(codec, width, height, frame);
    let reference = super::probe::reference(width, height);
    tokio::time::timeout(Duration::from_secs(2), decode(codec, &bytes, &reference))
        .await
        .context("Quality probe deadline")?
}

async fn decode(codec: Codec, bytes: &[u8], reference: &[u8]) -> Result<f64> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-xerror",
            "-threads",
            "1",
            "-f",
            codec.muxer(),
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-pix_fmt",
            "nv12",
            "-f",
            "rawvideo",
            "pipe:1",
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
        output
            .take(reference.len() as u64 + 1)
            .read_to_end(&mut data)
            .await?;
        ensure!(
            data.len() == reference.len(),
            "Unexpected decoded probe size"
        );
        Ok::<_, anyhow::Error>(data)
    };
    let ((), pixels) = tokio::try_join!(write, read)?;
    ensure!(child.wait().await?.success(), "Probe decode failed");
    Ok(psnr(reference, &pixels))
}

fn psnr(reference: &[u8], pixels: &[u8]) -> f64 {
    let square_error: u64 = reference
        .iter()
        .zip(pixels)
        .map(|(&a, &b)| (i32::from(a) - i32::from(b)).pow(2) as u64)
        .sum();
    if square_error == 0 {
        return 99.99;
    }
    10.0 * (255.0_f64.powi(2) * reference.len() as f64 / square_error as f64).log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t479_quality_gate_includes_chroma_and_preserves_lossless_order() {
        assert_eq!(psnr(&[16, 16, 128], &[16, 16, 128]), 99.99);
        assert!(psnr(&[16, 16, 128], &[16, 16, 100]) < 25.0);
        assert!(psnr(&[16, 16, 128], &[16, 16, 127]) > 50.0);
    }
}
