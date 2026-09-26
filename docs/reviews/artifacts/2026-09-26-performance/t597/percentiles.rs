use std::hint::black_box;
use std::time::Instant;
fn original(values: &mut [u32]) -> [u32; 3] {
    values.sort_unstable();
    [values[((values.len() - 1) as f64 * 0.50).round() as usize],
     values[((values.len() - 1) as f64 * 0.95).round() as usize], values[values.len()-1]]
}
fn candidate(values: &mut [u32]) -> [u32; 3] {
    let median = ((values.len() - 1) as f64 * 0.50).round() as usize;
    let tail = ((values.len() - 1) as f64 * 0.95).round() as usize;
    let p50 = *values.select_nth_unstable(median).1;
    let p95 = *values.select_nth_unstable(tail).1;
    [p50,p95,*values.iter().max().unwrap()]
}
fn fixture(n: usize, distribution: &str, seed: &mut u32) -> Vec<u32> {
    let mut source: Vec<u32> = (0..n).map(|_| {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        *seed
    }).collect();
    if distribution == "duplicates" { source.iter_mut().for_each(|v| *v %= 8); }
    if distribution == "sorted" { source.sort_unstable(); }
    source
}
fn sample(source: &[u32], distribution: &str, trial: usize) {
    let n = source.len();
    assert_eq!(original(&mut source.to_vec()), candidate(&mut source.to_vec()));
    for (name, work) in [("sort", original as fn(&mut [u32]) -> [u32; 3]), ("select", candidate)] {
        let mut values = vec![0; n];
        let start = Instant::now();
        for _ in 0..20000 {
            values.copy_from_slice(source);
            black_box(work(black_box(&mut values)));
        }
        println!("{{\"size\":{n},\"distribution\":\"{distribution}\",\"trial\":{trial},\"algorithm\":\"{name}\",\"ns\":{}}}", start.elapsed().as_nanos());
    }
}
fn main() {
    let mut seed = 42u32;
    for n in [16, 64, 256, 1024] {
        for distribution in ["random", "duplicates", "sorted"] {
            let source = fixture(n, distribution, &mut seed);
            for trial in 0..3 { sample(&source, distribution, trial); }
        }
    }
}
