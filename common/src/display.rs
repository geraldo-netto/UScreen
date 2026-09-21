//! Fixed-porch display timing shared by configuration and EDID generation.
use anyhow::Result;

/// Fallback physical geometry until the authenticated tablet supplies its size.
pub const DEFAULT_WIDTH_MM: u32 = 310;
pub const DEFAULT_HEIGHT_MM: u32 = 194;

pub const H_PORCHES: (u32, u32, u32) = (48, 32, 80);
pub const V_PORCHES: (u32, u32, u32) = (3, 10, 25);

/// Conservative supported DTD boundary, not a compositor compatibility claim.
pub const MIN_PIXEL_CLOCK_10KHZ: u64 = 1000;

fn timing_area(width: u32, height: u32) -> Result<u64> {
    anyhow::ensure!(
        (1..=crate::MAX_DIMENSION).contains(&width) && (1..=crate::MAX_DIMENSION).contains(&height),
        "EDID active dimensions must fit 12 bits: {width}x{height}"
    );
    let horizontal = u64::from(width + H_PORCHES.0 + H_PORCHES.1 + H_PORCHES.2);
    let vertical = u64::from(height + V_PORCHES.0 + V_PORCHES.1 + V_PORCHES.2);
    Ok(horizontal * vertical)
}

/// First integer refresh whose rounded DTD clock reaches 10 MHz. Values above
/// MAX_FPS mean that this geometry has no supported refresh in our range.
pub fn minimum_refresh(width: u32, height: u32) -> Result<u32> {
    let area = timing_area(width, height)?;
    Ok(((MIN_PIXEL_CLOCK_10KHZ * 10_000 - 5000).div_ceil(area) as u32).max(crate::MIN_FPS))
}

/// Saved settings/native-size negotiation may raise a low refresh; explicit
/// requests use pixel_clock_10khz and receive a rejection instead.
pub fn compatible_refresh(width: u32, height: u32, fps: u32) -> Result<u32> {
    let selected = fps.max(minimum_refresh(width, height)?);
    pixel_clock_10khz(width, height, selected)?;
    Ok(selected)
}

pub fn pixel_clock_10khz(width: u32, height: u32, fps: u32) -> Result<u16> {
    let area = timing_area(width, height)?;
    anyhow::ensure!(
        (crate::MIN_FPS..=crate::MAX_FPS).contains(&fps),
        "EDID refresh must be within {}..={} Hz",
        crate::MIN_FPS,
        crate::MAX_FPS
    );
    let clock = (area * u64::from(fps) + 5000) / 10000;
    anyhow::ensure!(clock <= u64::from(u16::MAX),
        "EDID pixel clock for {width}x{height}@{fps} exceeds 655.35 MHz; lower the resolution or refresh rate");
    anyhow::ensure!(clock >= MIN_PIXEL_CLOCK_10KHZ,
        "EDID pixel clock for {width}x{height}@{fps} is below the supported 10 MHz minimum; use at least {} Hz (maximum {} Hz), or increase the resolution",
        minimum_refresh(width, height)?, crate::MAX_FPS);
    Ok(clock as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t321_invalid_and_boundary_numeric_corpus_never_wraps_or_admits_low_clock() {
        for width in [0, 1, 639, 640, 4095, 4096, u32::MAX] {
            for height in [0, 1, 479, 480, 4095, 4096, u32::MAX] {
                check_rates(width, height);
            }
        }
        let mut seed = 0x321a_755bu32;
        for _ in 0..1024 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            check_rates(seed % 5000, seed.rotate_left(16) % 5000);
        }
    }

    fn check_rates(width: u32, height: u32) {
        for fps in [0, 9, 10, 24, 25, 60, 90, 91, u32::MAX] {
            let value = pixel_clock_10khz(width, height, fps);
            if let Ok(clock) = value {
                assert!((1000..=u16::MAX).contains(&clock));
                assert!((1..=4095).contains(&width) && (1..=4095).contains(&height));
                assert!((10..=90).contains(&fps));
            }
        }
    }

    #[test]
    fn t321_minimum_refresh_and_rounded_clock_boundaries_agree() {
        for (width, height, minimum) in [
            (640, 480, 25),
            (640, 1211, 11),
            (640, 1212, 10),
            (1280, 800, 10),
        ] {
            assert_eq!(minimum_refresh(width, height).unwrap(), minimum);
            assert!(pixel_clock_10khz(width, height, minimum).is_ok());
            assert!(pixel_clock_10khz(width, height, minimum - 1).is_err());
            assert_eq!(compatible_refresh(width, height, 10).unwrap(), minimum);
        }
        assert_eq!(
            pixel_clock_10khz(90, 3960, 10).unwrap(),
            1000,
            "exact 9.995 MHz rounds to the minimum DTD clock"
        );
        assert!(pixel_clock_10khz(89, 3960, 10).is_err());
        assert!(compatible_refresh(1, 1, 10).is_err());
        assert!(minimum_refresh(u32::MAX, 480).is_err());
        assert!(compatible_refresh(3840, 2160, 90).is_err());
    }

    #[test]
    fn t321_low_clock_is_rejected_with_supported_refresh_alternative() {
        let error = pixel_clock_10khz(640, 480, 10)
            .expect_err("T321: 4.14 MHz must not be supported")
            .to_string();
        assert!(error.contains("10 MHz"));
        assert!(error.contains("25 Hz"));
        assert!(pixel_clock_10khz(640, 480, 24).is_err());
        assert_eq!(pixel_clock_10khz(640, 480, 25).unwrap(), 1036);
    }

    #[test]
    fn t321_saved_low_clock_preserves_geometry_and_raises_refresh() {
        let mut config = crate::FileConfig {
            width: 640,
            height: 480,
            fps: 10,
            ..Default::default()
        };
        assert!(config.validate().is_err());
        config.sanitize();
        assert_eq!((config.width, config.height, config.fps), (640, 480, 25));
        assert!(config.validate().is_ok());
    }
}
