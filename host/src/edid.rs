//! Runtime EDID generation (port of scripts/gen-edid.py).
//! Lets the virtual display match any tablet resolution without shipping
//! pre-baked EDID binaries.

use anyhow::{Context, Result};
use std::path::PathBuf;

fn encode_manufacturer_id(s: &[u8; 3]) -> (u8, u8) {
    let c1 = (s[0] - b'A' + 1) as u16;
    let c2 = (s[1] - b'A' + 1) as u16;
    let c3 = (s[2] - b'A' + 1) as u16;
    let byte8 = ((c1 & 0x1F) << 2) | ((c2 >> 3) & 0x03);
    let byte9 = ((c2 & 0x07) << 5) | (c3 & 0x1F);
    (byte8 as u8, byte9 as u8)
}

/// Physical size assumed when the tablet has not reported its own, in mm.
/// Roughly a 14.6" 16:10 panel.
pub const DEFAULT_WIDTH_MM: u32 = 310;
pub const DEFAULT_HEIGHT_MM: u32 = 194;

/// Build a 128-byte EDID with a single detailed timing descriptor.
///
/// `width_mm`/`height_mm` are the panel's real physical size. They matter more
/// than they look: the compositor derives DPI from them, and that is what KDE
/// uses to pick a default scale factor. A wrong size means the desktop comes up
/// at the wrong scale on every fresh connection.
pub fn make_edid_sized(
    width: u32,
    height: u32,
    refresh: u32,
    width_mm: u32,
    height_mm: u32,
) -> Result<Vec<u8>> {
    let pixel_clock_10khz = uscreen_config::display::pixel_clock_10khz(width, height, refresh)?;
    anyhow::ensure!(
        (1..=4095).contains(&width_mm) && (1..=4095).contains(&height_mm),
        "EDID physical dimensions must fit 12 bits and be positive"
    );
    let mut edid = vec![0u8; 128];

    // Header
    edid[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);

    // Manufacturer ID (USC = UScreen)
    let (b8, b9) = encode_manufacturer_id(b"USC");
    edid[8] = b8;
    edid[9] = b9;

    // Product code / serial
    edid[10] = 0x01;
    edid[12..16].copy_from_slice(&1u32.to_le_bytes());

    edid[16] = 0; // week
    edid[17] = 34; // year (2024-1990)
    edid[18] = 1; // version
    edid[19] = 4; // revision

    edid[20] = 0xA5; // Digital, 8 bpc, DisplayPort interface code (5)
    edid[21] = (width_mm / 10).clamp(1, 255) as u8; // max image size, cm
    edid[22] = (height_mm / 10).clamp(1, 255) as u8;
    edid[23] = 0x78; // gamma 2.2
    edid[24] = 0xEE; // RGB, DPMS

    // Chromaticity (sRGB). Bytes 25..=34 inclusive — ten of them; the previous
    // nine-byte copy left the last one zeroed.
    edid[25..35].copy_from_slice(&[0xEE, 0x91, 0xA3, 0x54, 0x4C, 0x99, 0x26, 0x0F, 0x50, 0x54]);

    // No established timings; standard timings unused
    for i in 0..8 {
        edid[38 + i * 2] = 0x01;
        edid[39 + i * 2] = 0x01;
    }

    // Custom fixed-porch timings; these do not implement the CVT-RB formulas.
    let h_active = width;
    let v_active = height;
    let (h_front, h_sync, h_back) = uscreen_config::display::H_PORCHES;
    let h_blank = h_front + h_sync + h_back;
    let (v_front, v_sync, v_back) = uscreen_config::display::V_PORCHES;
    let v_blank = v_front + v_sync + v_back;

    let h_total = h_active + h_blank;
    let v_total = v_active + v_blank;

    let h_image = width_mm;
    let v_image = height_mm;

    // === DTD 1 (bytes 54-71) ===
    let i = 54;
    edid[i..i + 2].copy_from_slice(&pixel_clock_10khz.to_le_bytes());
    edid[i + 2] = (h_active & 0xFF) as u8;
    edid[i + 3] = (h_blank & 0xFF) as u8;
    edid[i + 4] = ((((h_active >> 8) & 0x0F) << 4) | ((h_blank >> 8) & 0x0F)) as u8;
    edid[i + 5] = (v_active & 0xFF) as u8;
    edid[i + 6] = (v_blank & 0xFF) as u8;
    edid[i + 7] = ((((v_active >> 8) & 0x0F) << 4) | ((v_blank >> 8) & 0x0F)) as u8;
    edid[i + 8] = (h_front & 0xFF) as u8;
    edid[i + 9] = (h_sync & 0xFF) as u8;
    edid[i + 10] = (((v_front & 0x0F) << 4) | (v_sync & 0x0F)) as u8;
    edid[i + 11] = ((((h_front >> 8) & 0x03) << 6)
        | (((h_sync >> 8) & 0x03) << 4)
        | (((v_front >> 4) & 0x03) << 2)
        | ((v_sync >> 4) & 0x03)) as u8;
    edid[i + 12] = (h_image & 0xFF) as u8;
    edid[i + 13] = (v_image & 0xFF) as u8;
    edid[i + 14] = ((((h_image >> 8) & 0x0F) << 4) | ((v_image >> 8) & 0x0F)) as u8;
    edid[i + 17] = 0x1E; // non-interlaced, digital separate sync, +h +v

    // === Unused descriptor (bytes 72-89): EDID dummy tag, zero payload ===
    edid[75] = 0x10;

    // === Monitor name descriptor (bytes 90-107) ===
    let i = 90;
    edid[i + 3] = 0xFC;
    let name = b"UScreen\n     ";
    edid[i + 5..i + 5 + 13].copy_from_slice(&name[..13]);

    // === Range limits descriptor (bytes 108-125) ===
    let i = 108;
    edid[i + 3] = 0xFD;
    edid[i + 5] = uscreen_config::MIN_FPS as u8;
    edid[i + 6] = uscreen_config::MAX_FPS as u8;
    // Include the actual rounded DTD clock and every configured refresh.
    // EDID 1.4 range offsets represent horizontal rates above 255 kHz.
    let clock_hz = u32::from(pixel_clock_10khz) * 10_000;
    let min_h = (v_total * uscreen_config::MIN_FPS / 1000).min(clock_hz / (h_total * 1000));
    let max_h = (v_total * uscreen_config::MAX_FPS)
        .div_ceil(1000)
        .max(clock_hz.div_ceil(h_total * 1000));
    edid[i + 4] = (u8::from(min_h > 255) << 2) | (u8::from(max_h > 255) << 3);
    edid[i + 7] = (min_h - if min_h > 255 { 255 } else { 0 }) as u8;
    edid[i + 8] = (max_h - if max_h > 255 { 255 } else { 0 }) as u8;
    let max_pclk_mhz = (pixel_clock_10khz as u32 * 10_000).div_ceil(1_000_000);
    edid[i + 9] = max_pclk_mhz.div_ceil(10).min(255) as u8;
    edid[i + 10] = 0x01; // EDID 1.4: range limits only, no timing formula
    edid[i + 11..i + 18].copy_from_slice(b"\x0A      ");

    // Checksum
    let sum: u32 = edid[..127].iter().map(|&b| b as u32).sum();
    edid[127] = ((256 - (sum % 256)) % 256) as u8;

    Ok(edid)
}

/// Build an EDID at the default physical size.
#[cfg(test)]
pub fn make_edid(width: u32, height: u32, refresh: u32) -> Vec<u8> {
    make_edid_sized(width, height, refresh, DEFAULT_WIDTH_MM, DEFAULT_HEIGHT_MM).unwrap()
}

/// Bumped whenever the generator changes. It is part of the cache filename so
/// that fixing a bug here actually reaches existing installs — without it, a
/// stale file from a previous version would be reused forever.
const EDID_GENERATION: u32 = 6;

/// Write (or reuse) a generated EDID for this mode and return its path.
pub fn ensure_edid_sized(
    width: u32,
    height: u32,
    refresh: u32,
    width_mm: u32,
    height_mm: u32,
) -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = PathBuf::from(home).join(".local/share/uscreen/edid");
    ensure_edid_in(&dir, width, height, refresh, width_mm, height_mm)
}

fn ensure_edid_in(
    dir: &std::path::Path,
    width: u32,
    height: u32,
    refresh: u32,
    width_mm: u32,
    height_mm: u32,
) -> Result<PathBuf> {
    let edid = make_edid_sized(width, height, refresh, width_mm, height_mm)?;
    std::fs::create_dir_all(dir).context("create EDID dir")?;
    let path = dir.join(format!(
        "auto-v{}-{}x{}@{}-{}x{}mm.bin",
        EDID_GENERATION, width, height, refresh, width_mm, height_mm
    ));
    if std::fs::read(&path).ok().as_deref() != Some(edid.as_slice()) {
        // Only complete, matching generated contents are reusable. A previous
        // interrupted write must not poison every subsequent capture attempt.
        use std::io::Write;
        let mut temporary = tempfile::NamedTempFile::new_in(dir).context("create EDID file")?;
        temporary.write_all(&edid).context("write EDID")?;
        temporary.as_file().sync_all().context("sync EDID")?;
        temporary.persist(&path).context("replace EDID")?;
        tracing::info!(
            "Generated EDID for {}x{}@{} ({}x{}mm) at {:?}",
            width,
            height,
            refresh,
            width_mm,
            height_mm,
            path
        );
    }
    Ok(path)
}

/// Write (or reuse) a generated EDID at the default physical size.
#[allow(dead_code)]
pub fn ensure_edid(width: u32, height: u32, refresh: u32) -> Result<PathBuf> {
    ensure_edid_sized(width, height, refresh, DEFAULT_WIDTH_MM, DEFAULT_HEIGHT_MM)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_t320_dummy_descriptor(edid: &[u8]) {
        // EDID base-block display descriptor: zero clock/reserved bytes,
        // dummy tag 0x10, and zero reserved payload. All-zero is invalid.
        assert_eq!(edid.len(), 128);
        assert_eq!(&edid[72..75], &[0, 0, 0]);
        assert_eq!(edid[75], 0x10, "T320: unused descriptor has no dummy tag");
        assert!(edid[76..90].iter().all(|&byte| byte == 0));
        assert_eq!(
            edid.iter().map(|&byte| u32::from(byte)).sum::<u32>() % 256,
            0
        );
    }

    #[test]
    fn t320_rust_unused_descriptor_is_valid() {
        for (width, height, fps) in [(1920, 1080, 60), (2960, 1848, 90), (1024, 4095, 10)] {
            assert_t320_dummy_descriptor(&make_edid(width, height, fps));
        }
    }

    #[test]
    fn t320_python_unused_descriptor_is_valid() {
        for (width, height, fps) in [(1920, 1080, 60), (2960, 1848, 90), (1024, 4095, 10)] {
            assert_t320_dummy_descriptor(&python_edid(width, height, fps, 310, 194).unwrap());
        }
    }

    #[test]
    fn t246_repairs_incomplete_and_wrong_generated_edids() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let expected = make_edid_sized(1920, 1080, 60, 310, 194).unwrap();
        let path = ensure_edid_in(dir.path(), 1920, 1080, 60, 310, 194).unwrap();
        let wrong_mode = make_edid_sized(1280, 720, 60, 310, 194).unwrap();
        for corrupt in [vec![], expected[..64].to_vec(), wrong_mode] {
            std::fs::write(&path, &corrupt).unwrap();
            assert_eq!(
                ensure_edid_in(dir.path(), 1920, 1080, 60, 310, 194).unwrap(),
                path
            );
            assert_eq!(
                std::fs::read(&path).unwrap(),
                expected,
                "T246: damaged generated EDID was reused"
            );
        }
        let valid_inode = std::fs::metadata(&path).unwrap().ino();
        ensure_edid_in(dir.path(), 1920, 1080, 60, 310, 194).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().ino(),
            valid_inode,
            "valid cache should be reused"
        );
    }

    #[test]
    fn t246_cache_directory_is_not_returned_as_an_edid() {
        let dir = tempfile::tempdir().unwrap();
        let path = ensure_edid_in(dir.path(), 1920, 1080, 60, 310, 194).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(
            ensure_edid_in(dir.path(), 1920, 1080, 60, 310, 194).is_err(),
            "T246: helper would be given a directory instead of an EDID"
        );
    }

    // Independent DTD decoder: Linux drm_edid.h orders width low, height
    // low, then both high nibbles at offsets 12, 13, 14.
    fn physical_size(edid: &[u8]) -> (u32, u32) {
        (
            u32::from(edid[66]) | (u32::from(edid[68] & 0xf0) << 4),
            u32::from(edid[67]) | (u32::from(edid[68] & 0x0f) << 8),
        )
    }

    #[test]
    fn t114_rust_dtd_reports_exact_physical_size() {
        for (w, h) in [(310, 194), (255, 256), (4095, 4095)] {
            assert_eq!(
                physical_size(&make_edid_sized(1920, 1080, 60, w, h).unwrap()),
                (w, h)
            );
        }
    }

    #[test]
    fn t114_old_edid_cache_is_not_reused() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("auto-v3-1920x1080@60-310x194mm.bin");
        std::fs::write(&old, [0u8; 128]).unwrap();
        let path = ensure_edid_in(dir.path(), 1920, 1080, 60, 310, 194).unwrap();
        assert_ne!(path, old);
        assert_eq!(physical_size(&std::fs::read(path).unwrap()), (310, 194));
    }

    #[test]
    fn t114_python_dtd_reports_exact_physical_size() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("display.bin");
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/gen-edid.py");
        assert!(std::process::Command::new("python3")
            .arg(script)
            .args(["1920", "1080", "60"])
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success());
        assert_eq!(physical_size(&std::fs::read(output).unwrap()), (310, 194));
    }

    fn python_edid(w: u32, h: u32, fps: u32, wm: u32, hm: u32) -> Option<Vec<u8>> {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("display.bin");
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/gen-edid.py");
        let status = std::process::Command::new("python3")
            .arg(script)
            .args([w.to_string(), h.to_string(), fps.to_string()])
            .arg(&output)
            .args([wm.to_string(), hm.to_string()])
            .output()
            .unwrap()
            .status;
        status.success().then(|| std::fs::read(output).unwrap())
    }

    #[test]
    fn t332_shared_mode_validation_agrees_with_both_generators_at_clock_boundary() {
        for (width, height, fps, valid) in [
            (3840, 2160, 60, true),
            (3840, 2160, 74, true),
            (3840, 2160, 75, false),
            (3840, 2160, 90, false),
            (4095, 4095, 60, false),
            (640, 480, 10, true),
        ] {
            let config = uscreen_config::FileConfig {
                width,
                height,
                fps,
                ..Default::default()
            };
            let rust = make_edid_sized(width, height, fps, 310, 194);
            let python = python_edid(width, height, fps, 310, 194);
            assert_eq!(
                config.validate().is_ok(),
                valid,
                "T332: {width}x{height}@{fps}"
            );
            assert_eq!(rust.is_ok(), valid);
            assert_eq!(python.is_some(), valid);
            if valid {
                assert_eq!(rust.unwrap(), python.unwrap());
            }
        }
    }

    #[test]
    fn t115_generators_agree_on_boundaries_color_and_ranges() {
        for (w, h, fps, wm, hm) in [
            (1920, 1080, 10, 310, 194),
            (2960, 1848, 90, 314, 195),
            (4095, 2160, 30, 4095, 4095),
            (1, 1080, 10, 255, 256),
            (1024, 4095, 90, 310, 194),
        ] {
            let rust = make_edid_sized(w, h, fps, wm, hm).unwrap();
            let python = python_edid(w, h, fps, wm, hm).unwrap();
            assert_eq!(rust, python, "generator mismatch at {w}x{h}@{fps}");
            assert!(u32::from(rust[113]) <= fps && u32::from(rust[114]) >= fps);
            let pclk = u32::from(u16::from_le_bytes([rust[54], rust[55]])) * 10_000;
            let horizontal = pclk as f64 / f64::from(w + 160);
            let low = u32::from(rust[115]) + if rust[112] & 4 != 0 { 255 } else { 0 };
            let high = u32::from(rust[116]) + if rust[112] & 8 != 0 { 255 } else { 0 };
            assert!(f64::from(low * 1000) <= horizontal && f64::from(high * 1000) >= horizontal);
            // sRGB xy coordinates decoded independently from packed 10-bit values.
            let expected = [0.640, 0.330, 0.300, 0.600, 0.150, 0.060, 0.3127, 0.3290];
            for (index, expected) in expected.into_iter().enumerate() {
                let low_bits = (rust[25 + index / 4] >> (6 - (index % 4) * 2)) & 3;
                let value = (u16::from(rust[27 + index]) * 4 + u16::from(low_bits)) as f64 / 1024.0;
                assert!((value - expected).abs() < 1.0 / 1024.0);
            }
        }
    }

    #[test]
    fn t115_both_generators_reject_unrepresentable_or_unsupported_values() {
        for (w, h, fps, wm, hm) in [
            (4096, 1080, 60, 310, 194),
            (1920, 4096, 60, 310, 194),
            (0, 1080, 60, 310, 194),
            (1920, 1080, 0, 310, 194),
            (1920, 1080, 9, 310, 194),
            (1920, 1080, 91, 310, 194),
            (4095, 4095, 90, 310, 194),
            (1920, 1080, 60, 4096, 194),
            (1920, 1080, 60, 310, 0),
        ] {
            assert!(
                python_edid(w, h, fps, wm, hm).is_none(),
                "Python accepted {w}x{h}@{fps}, {wm}x{hm} mm"
            );
            assert!(
                make_edid_sized(w, h, fps, wm, hm).is_err(),
                "Rust accepted {w}x{h}@{fps}, {wm}x{hm} mm"
            );
        }
    }

    #[test]
    fn t115_old_range_cache_is_not_reused() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("auto-v4-1920x1080@10-310x194mm.bin");
        std::fs::write(&old, [0u8; 128]).unwrap();
        let path = ensure_edid_in(dir.path(), 1920, 1080, 10, 310, 194).unwrap();
        assert_ne!(path, old);
    }

    #[test]
    fn t061_rejects_unrepresentable_edid_modes() {
        for (w, h, fps) in [
            (4096, 1080, 60),
            (1920, 4096, 60),
            (4095, 4095, 90),
            (0, 1080, 60),
        ] {
            assert!(
                make_edid_sized(w, h, fps, 310, 194).is_err(),
                "{w}x{h}@{fps} must not be truncated"
            );
        }
        assert!(make_edid_sized(4095, 2160, 30, 310, 194).is_ok());
    }

    #[test]
    fn edid_checksum_is_valid() {
        for (w, h) in [(2960u32, 1848u32), (2560, 1600), (1920, 1200), (2000, 1200)] {
            let edid = make_edid(w, h, 60);
            assert_eq!(edid.len(), 128);
            let sum: u32 = edid.iter().map(|&b| b as u32).sum();
            assert_eq!(sum % 256, 0, "checksum for {}x{}", w, h);
            // Decode DTD active size back
            let h_act = edid[56] as u32 | (((edid[58] >> 4) as u32) << 8);
            let v_act = edid[59] as u32 | (((edid[61] >> 4) as u32) << 8);
            assert_eq!((h_act, v_act), (w, h));
        }
    }
}
