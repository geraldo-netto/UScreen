use super::*;
use crate::capture_capacity::Request;
use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_FREE};

#[test]
fn t697_native_allocation_bounds_and_release() {
    for mib in crate::model::PIPE_CAPACITIES_MIB {
        let plan = Request {
            buffer_mib: mib,
            workers: 1,
        }
        .plan(1)
        .unwrap();
        let mut buffer = OwnedBuffer::new(plan).unwrap();
        let capacity = buffer.capacity();
        assert_eq!(capacity, mib as usize * 1024 * 1024);
        assert!(buffer.read(0, capacity).unwrap().iter().all(|b| *b == 0));
        buffer.write(0, b"head").unwrap();
        buffer.write(capacity - 4, b"tail").unwrap();
        assert_eq!(buffer.read(0, 4).unwrap(), b"head");
        assert_eq!(buffer.read(capacity - 4, 4).unwrap(), b"tail");
        for offset in [capacity - 1, capacity, capacity + 1, usize::MAX] {
            assert!(buffer.write(offset, b"four").is_err());
            assert!(buffer.read(offset, 4).is_none());
        }
        assert!(buffer.read(0, usize::MAX).is_none());
        assert!(buffer.read(capacity, 0).unwrap().is_empty());
        buffer.write(capacity, &[]).unwrap();
        let address = buffer.address.as_ptr();
        drop(buffer);
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe { VirtualQuery(address.cast(), &mut info, std::mem::size_of_val(&info)) },
            0
        );
        assert_eq!(info.State, MEM_FREE, "T697 owner released its reservation");
    }
}
#[test]
fn t697_native_bounded_buffer_property_and_worker_evidence() {
    let plan = Request {
        buffer_mib: 1,
        workers: 3,
    }
    .plan(8)
    .unwrap();
    let mut buffer = OwnedBuffer::new(plan).unwrap();
    let mut random = 0x697u64;
    for _ in 0..512 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let offset = (random >> 32) as usize % (buffer.capacity() + 64);
        let data = vec![random as u8; random as usize % 64];
        let fits = offset + data.len() <= buffer.capacity();
        assert_eq!(buffer.write(offset, &data).is_ok(), fits);
        assert_eq!(
            buffer.read(offset, data.len()),
            fits.then_some(data.as_slice())
        );
    }
    // Caller plus two successfully started, joined native OS threads. This is
    // capacity evidence for an owned fixture, not a shipped conversion pool.
    let threads: Vec<_> = (0..2).map(|_| std::thread::spawn(|| 1u32)).collect();
    let started = 1 + threads.into_iter().map(|t| t.join().unwrap()).sum::<u32>();
    let report = plan.record(&buffer, started).unwrap();
    assert_eq!(report.worker_capacity, 3);
    assert_eq!(report.active_workers(0), 0);
    assert_eq!(report.active_workers(1), 1);
    assert_eq!(report.active_workers(u32::MAX), 3);
}
