//! Portable libx264 worker policy. Zero requests bounded automatic selection.
pub const MAX_WORKERS: u32 = 128;

pub fn validate(requested: u32) -> anyhow::Result<u32> {
    anyhow::ensure!(
        requested <= MAX_WORKERS,
        "Use auto or 1–128 encoder workers"
    );
    Ok(requested)
}

pub fn candidates(encoder: &str, requested: u32) -> Vec<u32> {
    if encoder != "libx264" {
        return vec![0];
    }
    if requested > 0 {
        return vec![requested.min(MAX_WORKERS)];
    }
    vec![1, 2, 4]
}

/// x264 publishes effective workers in its user-data SEI. Absence stays unknown.
pub fn effective_x264(packet: &[u8]) -> Option<u32> {
    let marker = b"x264 - core ";
    let start = packet.windows(marker.len()).position(|p| p == marker)?;
    let text = &packet[start..packet.len().min(start + 4096)];
    let field = b" threads=";
    let index = text.windows(field.len()).position(|p| p == field)? + field.len();
    let digits: Vec<_> = text[index..]
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .take(4)
        .collect();
    if text
        .get(index + digits.len())
        .is_some_and(|b| !b.is_ascii_whitespace() && *b != 0)
    {
        return None;
    }
    let count: u32 = std::str::from_utf8(&digits).ok()?.parse().ok()?;
    (1..=MAX_WORKERS).contains(&count).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t612_budget_order_manual_override_and_bounds() {
        assert_eq!(candidates("libx264", 0), [1, 2, 4]);
        assert_eq!(candidates("libx264", 7), [7]);
        assert_eq!(candidates("h264_vaapi", 7), [0]);
        for n in 0..=256 {
            assert_eq!(validate(n).is_ok(), n <= MAX_WORKERS);
            assert!(candidates("libx264", n)
                .iter()
                .all(|n| (1..=128).contains(n)));
        }
        assert!(validate(u32::MAX).is_err());
    }
    #[test]
    fn t612_effective_count_is_evidence_not_requested_capacity() {
        assert_eq!(
            effective_x264(b"noise x264 - core 164 - options: threads=12 lookahead_threads=2"),
            Some(12)
        );
        for text in [
            b"threads=12".as_slice(),
            b"x264 - core 164",
            b"x264 - core 164 threads=0",
            b"x264 - core 164 threads=12bad",
            b"x264 - core 164 threads=129",
            b"x264 - core 164 threads=999999",
        ] {
            assert_eq!(effective_x264(text), None);
        }
        let valid = b"x264 - core 164 threads=4 ";
        for end in 0..valid.len() {
            let _ = effective_x264(&valid[..end]);
        }
        for byte in 0..=255 {
            assert!(effective_x264(&vec![byte; 4096]).is_none());
        }
    }
}
