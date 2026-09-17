// T404: appended inside latency.rs so the replay measures its actual storage.
pub fn replay_lookup(mode: String, count: usize, gate: std::sync::Arc<std::sync::Barrier>) -> serde_json::Value {
    let tracker = LatencyTracker::new();
    let step = if mode == "sparse" { 64 } else { 1 };
    for index in 0..MAX_TRACKED { tracker.on_encoded(index as u32 * step); }
    let query = match mode.as_str() { "first" => 0, "missing" => 100_000, _ => (MAX_TRACKED as u32 - 1) * step };
    gate.wait();
    let start = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..count {
        let state = tracker.inner.lock().unwrap();
        checksum = checksum.wrapping_add(std::hint::black_box(probe_position(&state, std::hint::black_box(query))).unwrap_or(MAX_TRACKED));
    }
    serde_json::json!({"wall_ns": start.elapsed().as_nanos() as u64, "checksum": checksum})
}
fn replay_window(tracker: &LatencyTracker) {
    for _ in 0..64 {
        let sequence = tracker.next_sequence();
        tracker.on_encoded(sequence);
        tracker.on_rendered(sequence, 100);
    }
    tracker.inner.lock().unwrap().last_report = Some(Instant::now() - std::time::Duration::from_secs(6));
    tracker.maybe_report();
}
pub fn replay_reports(sessions: usize, count: usize) -> serde_json::Value {
    let trackers: Vec<_> = (0..sessions).map(|_| LatencyTracker::new()).collect();
    for tracker in &trackers { replay_window(tracker); replay_window(tracker); }
    crate::ALLOCATIONS.store(0, Ordering::Relaxed);
    crate::RELOCATIONS.store(0, Ordering::Relaxed);
    crate::MEASURE.store(true, Ordering::Relaxed);
    for _ in 0..count {
        for tracker in &trackers { replay_window(tracker); }
    }
    crate::MEASURE.store(false, Ordering::Relaxed);
    serde_json::json!({"allocations": crate::ALLOCATIONS.load(Ordering::Relaxed),
        "reallocations": crate::RELOCATIONS.load(Ordering::Relaxed)})
}
