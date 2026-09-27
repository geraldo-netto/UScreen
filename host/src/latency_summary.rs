// Copyright (c) 2026 Geraldo Netto
//! T597: exact nearest-index p50/p95/max, microseconds to milliseconds.

/// Empty windows have no statistics. All u32 values, including saturation,
/// remain valid; the caller owns the window-size bound and storage recycling.
pub(crate) fn percentiles(values: &mut [u32]) -> Option<[f64; 3]> {
    let last = values.len().checked_sub(1)?;
    let median = (last as f64 * 0.50).round() as usize;
    let tail = (last as f64 * 0.95).round() as usize;
    // Small windows benefit from sorting; sorted windows avoid partitioning.
    // For larger unsorted windows, each partition only visits its own slice.
    if values.len() <= 64 {
        values.sort_unstable();
    } else if values.is_sorted() {
        // Already in rank order.
    } else if values.is_sorted_by(|a, b| a >= b) {
        values.reverse();
    } else {
        let (lower, _, _) = values.select_nth_unstable(tail);
        lower.select_nth_unstable(median);
    }
    let max = *values[tail..].iter().max()?;
    Some([values[median], values[tail], max].map(|micros| f64::from(micros) / 1000.0))
}

#[cfg(test)]
#[path = "latency_summary_tests.rs"]
mod tests;
