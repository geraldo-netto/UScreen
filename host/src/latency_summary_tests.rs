// Copyright (c) 2026 Geraldo Netto
use super::percentiles;

fn oracle(values: &[u32]) -> Option<[f64; 3]> {
    if values.is_empty() {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    Some([0.50, 0.95, 1.0].map(|rank| {
        f64::from(ordered[((ordered.len() as f64 - 1.0) * rank).round() as usize]) / 1000.0
    }))
}

fn compare(mut values: Vec<u32>) {
    let expected = oracle(&values);
    let capacity = values.capacity();
    let mut original = values.clone();
    assert_eq!(
        percentiles(&mut values),
        expected,
        "T597: len {}",
        values.len()
    );
    assert_eq!(values.capacity(), capacity, "T597: retain reusable storage");
    original.sort_unstable();
    values.sort_unstable();
    assert_eq!(
        values, original,
        "T597: partition must preserve all samples"
    );
}

#[test]
fn t597_empty_singleton_rounding_units_and_saturation() {
    assert_eq!(percentiles(&mut []), None);
    assert_eq!(percentiles(&mut [0]), Some([0.0; 3]));
    assert_eq!(percentiles(&mut [u32::MAX]), Some([4_294_967.295; 3]));
    assert_eq!(percentiles(&mut [1, 7, 9, 17]), Some([0.009, 0.017, 0.017]));
    for size in [1, 2, 31, 32, 33, 63, 64, 65, 1023, 1024, 1025, 2048] {
        compare(vec![u32::MAX; size]);
        compare(
            (0..size)
                .map(|i| if i % 2 == 0 { 0 } else { u32::MAX })
                .collect(),
        );
    }
}

// Bounded property corpus spans every supported report length plus out-of-bound
// helper lengths. The public tracker still caps stored windows at 1,024 samples.
#[test]
fn t597_order_statistics_match_sort_for_all_window_lengths() {
    let mut seed = 0x597_u32;
    for size in 0..=1025 {
        let random: Vec<_> = (0..size)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            })
            .collect();
        compare(random.clone());
        compare(random.iter().map(|value| value % 8).collect());
        let mut ordered = random;
        ordered.sort_unstable();
        compare(ordered.clone());
        ordered.reverse();
        compare(ordered);
    }
}
