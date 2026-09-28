/// Length of the Annex B prefix at the beginning of a slice.
pub fn annex_b_prefix_len(data: &[u8]) -> Option<usize> {
    if data.starts_with(&[0, 0, 0, 1]) {
        Some(4)
    } else if data.starts_with(&[0, 0, 1]) {
        Some(3)
    } else {
        None
    }
}

/// Start-code and NAL-header offsets, including an incomplete trailing prefix.
pub fn annex_b_starts(data: &[u8]) -> Vec<(usize, usize)> {
    annex_b_offsets(data).collect()
}

/// Allocation-free scanner; offsets include a prefix whose header is incomplete.
pub fn annex_b_offsets(data: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut offset = 0;
    std::iter::from_fn(move || {
        while offset + 3 <= data.len() {
            if let Some(length) = annex_b_prefix_len(&data[offset..]) {
                let start = offset;
                offset += length;
                return Some((start, offset));
            }
            offset += 1;
        }
        None
    })
}
