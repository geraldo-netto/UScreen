mod latency;
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Barrier};
struct Measured;
static MEASURE: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static RELOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl std::alloc::GlobalAlloc for Measured {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if MEASURE.load(Ordering::Relaxed) { ALLOCATIONS.fetch_add(1, Ordering::Relaxed); }
        std::alloc::System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) { std::alloc::System.dealloc(pointer, layout) }
    unsafe fn realloc(&self, pointer: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        if MEASURE.load(Ordering::Relaxed) { RELOCATIONS.fetch_add(1, Ordering::Relaxed); }
        std::alloc::System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Measured = Measured;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let sessions: usize = args[1].parse().unwrap();
    let mode = &args[2];
    let count: usize = args[3].parse().unwrap();
    if mode == "reports" {
        println!("{}", latency::replay_reports(sessions, count));
        return;
    }
    let gate = Arc::new(Barrier::new(sessions));
    let workers: Vec<_> = (0..sessions).map(|_| {
        let (mode, gate) = (mode.clone(), gate.clone());
        std::thread::spawn(move || latency::replay_lookup(mode, count, gate))
    }).collect();
    let lanes: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
    println!("{}", serde_json::json!({"lanes": lanes}));
}
