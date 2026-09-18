//! Fixed-porch display timing shared by configuration and EDID generation.
use anyhow::Result;

pub const H_PORCHES: (u32, u32, u32) = (48, 32, 80);
pub const V_PORCHES: (u32, u32, u32) = (3, 10, 25);

pub fn pixel_clock_10khz(width: u32, height: u32, fps: u32) -> Result<u16> {
    anyhow::ensure!(
        (1..=crate::MAX_DIMENSION).contains(&width) && (1..=crate::MAX_DIMENSION).contains(&height),
        "EDID active dimensions must fit 12 bits: {width}x{height}"
    );
    anyhow::ensure!(
        (crate::MIN_FPS..=crate::MAX_FPS).contains(&fps),
        "EDID refresh must be within {}..={} Hz",
        crate::MIN_FPS,
        crate::MAX_FPS
    );
    let horizontal = u64::from(width + H_PORCHES.0 + H_PORCHES.1 + H_PORCHES.2);
    let vertical = u64::from(height + V_PORCHES.0 + V_PORCHES.1 + V_PORCHES.2);
    let clock = (horizontal * vertical * u64::from(fps) + 5000) / 10000;
    anyhow::ensure!(clock > 0 && clock <= u64::from(u16::MAX),
        "EDID pixel clock for {width}x{height}@{fps} exceeds 655.35 MHz; lower the resolution or refresh rate");
    Ok(clock as u16)
}
