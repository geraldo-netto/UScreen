//! Bounded, portable RGBA thumbnail of the already-transformed webcam output.
use super::Lens;
use anyhow::{ensure, Result};

const SIDE: usize = 160;

#[derive(Debug)]
pub struct CameraPreview {
    lens: Lens,
    size: [usize; 2],
    rgba: Vec<u8>,
    created: std::time::Instant,
}

impl CameraPreview {
    pub fn new(lens: Lens, size: [usize; 2], rgba: Vec<u8>) -> Result<Self> {
        ensure!(
            size.iter().all(|side| (1..=SIDE).contains(side)),
            "camera preview dimensions exceed bounds"
        );
        ensure!(
            rgba.len() == size[0] * size[1] * 4,
            "camera preview buffer size mismatch"
        );
        Ok(Self {
            lens,
            size,
            rgba,
            created: std::time::Instant::now(),
        })
    }
    pub fn fresh(&self) -> bool {
        self.created.elapsed() < std::time::Duration::from_secs(2)
    }
    pub fn lens(&self) -> Lens {
        self.lens
    }
    pub fn size(&self) -> [usize; 2] {
        self.size
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn from_yuv420(lens: Lens, width: u32, height: u32, bytes: &[u8]) -> Result<Self> {
        validate_yuv(width, height, bytes.len())?;
        let (width, height) = (width as usize, height as usize);
        let longest = width.max(height);
        let size = [
            (width * SIDE / longest).max(1),
            (height * SIDE / longest).max(1),
        ];
        let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
        for y in 0..size[1] {
            for x in 0..size[0] {
                rgba.extend(pixel(
                    bytes,
                    width,
                    height,
                    x * width / size[0],
                    y * height / size[1],
                ));
            }
        }
        Self::new(lens, size, rgba)
    }
}

fn validate_yuv(width: u32, height: u32, length: usize) -> Result<()> {
    ensure!(
        (2..=1920).contains(&width) && (2..=1080).contains(&height),
        "invalid camera preview source dimensions"
    );
    ensure!(
        width.is_multiple_of(2) && height.is_multiple_of(2),
        "camera preview requires even YUV dimensions"
    );
    ensure!(
        length == (width * height * 3 / 2) as usize,
        "camera preview source buffer size mismatch"
    );
    Ok(())
}

fn pixel(bytes: &[u8], width: usize, height: usize, x: usize, y: usize) -> [u8; 4] {
    let luma = width * height;
    let chroma = (y / 2) * (width / 2) + x / 2;
    let c = i32::from(bytes[y * width + x]) - 16;
    let d = i32::from(bytes[luma + chroma]) - 128;
    let e = i32::from(bytes[luma + luma / 4 + chroma]) - 128;
    let channel = |value: i32| ((value + 128) >> 8).clamp(0, 255) as u8;
    [
        channel(298 * c + 409 * e),
        channel(298 * c - 100 * d - 208 * e),
        channel(298 * c + 516 * d),
        255,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t543_preview_bounds_colours_and_shape_preserve_exported_pixels() {
        let mut frame =
            CameraPreview::from_yuv420(Lens::Rear, 2, 2, &[16, 235, 81, 145, 128, 128]).unwrap();
        assert!(frame.fresh());
        frame.created -= std::time::Duration::from_secs(2);
        assert!(!frame.fresh());
        assert_eq!(frame.size(), [160, 160]);
        assert_eq!(frame.lens(), Lens::Rear);
        assert_eq!(&frame.rgba()[..4], &[0, 0, 0, 255]);
        assert_eq!(&frame.rgba()[80 * 4..80 * 4 + 4], &[255, 255, 255, 255]);
        let red =
            CameraPreview::from_yuv420(Lens::Front, 2, 2, &[81, 81, 81, 81, 90, 240]).unwrap();
        assert!(red.rgba()[0] >= 250 && red.rgba()[1] < 3 && red.rgba()[2] < 3);
        let wide = CameraPreview::from_yuv420(Lens::Front, 4, 2, &[128; 12]).unwrap();
        assert_eq!(wide.size(), [160, 80]);
        for width in [0, 1, 2, 3, 1920, 1921, u32::MAX] {
            for length in [0, 1, 5, 6, 7, 100] {
                assert_eq!(
                    CameraPreview::from_yuv420(Lens::Front, width, 2, &vec![128; length]).is_ok(),
                    width == 2 && length == 6
                );
            }
        }
        for size in [[0, 1], [1, 0], [161, 1], [usize::MAX, usize::MAX], [1, 1]] {
            assert!(CameraPreview::new(Lens::Front, size, vec![]).is_err());
        }
    }
}
