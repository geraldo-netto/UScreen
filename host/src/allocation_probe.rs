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

// The caller uses a current-thread runtime; these include its harness tasks.
#[cfg(test)]
pub(crate) async fn measure_async<T>(
    operation: impl std::future::Future<Output = T>,
) -> (T, Counts) {
    COUNTS.with(|c| {
        assert!(c.get().is_none());
        c.set(Some(Counts::default()));
    });
    let result = operation.await;
    let counts = COUNTS.with(|c| c.replace(None).unwrap());
    (result, counts)
}

#[cfg(target_os = "linux")]
pub(crate) fn cpu_ns() -> u64 {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut value) },
        0
    );
    value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64
}
#[cfg(windows)]
pub(crate) fn cpu_ns() -> u64 {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentThread, GetThreadTimes},
    };
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    assert_ne!(
        unsafe {
            GetThreadTimes(
                GetCurrentThread(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        },
        0
    );
    let ticks = |v: FILETIME| (u64::from(v.dwHighDateTime) << 32) | u64::from(v.dwLowDateTime);
    (ticks(kernel) + ticks(user)) * 100
}
