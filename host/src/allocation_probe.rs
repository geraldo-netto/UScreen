//! T384: opt-in counters on the calling test thread; absent from production builds.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct Counts {
    pub allocations: u64,
    pub reallocations: u64,
    pub requested_bytes: u64,
    pub explicit_copy_bytes: u64,
    pub scanner_input_bytes: u64,
}
thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

fn update(change: impl FnOnce(&mut Counts)) {
    let _ = COUNTS.try_with(|cell| {
        if let Some(mut counts) = cell.get() {
            change(&mut counts);
            cell.set(Some(counts));
        }
    });
}

struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        update(|c| {
            c.allocations += 1;
            c.requested_bytes += layout.size() as u64;
        });
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        update(|c| {
            c.allocations += 1;
            c.requested_bytes += layout.size() as u64;
        });
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        update(|c| {
            c.reallocations += 1;
            c.requested_bytes += size as u64;
        });
        System.realloc(ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}

pub(crate) fn copied(bytes: usize) {
    update(|c| c.explicit_copy_bytes += bytes as u64);
}
pub(crate) fn scanned(bytes: usize) {
    update(|c| c.scanner_input_bytes += bytes as u64);
}

pub(crate) fn measure<T>(operation: impl FnOnce() -> T) -> (T, Counts) {
    COUNTS.with(|c| {
        assert!(c.get().is_none());
        c.set(Some(Counts::default()));
    });
    let result = operation();
    let counts = COUNTS.with(|c| c.replace(None).unwrap());
    (result, counts)
}
