use super::*;
#[test]
fn t720_native_clock_requires_valid_io_and_tracks_driver_rate_epochs() {
    let mut reader = Clock::default();
    assert!(reader.sample().is_none());
    assert_eq!(reader.quantum(4096), 960);
    let mut clock: sys::spa_io_clock = unsafe { std::mem::zeroed() };
    clock.id = 7;
    clock.nsec = 1_000_000;
    clock.position = 441;
    clock.duration = 441;
    clock.rate.num = 1;
    clock.rate.denom = 44100;
    reader.position(
        sys::SPA_IO_Position,
        std::ptr::from_mut(&mut clock).cast(),
        std::mem::size_of_val(&clock) as u32,
    );
    let first = reader.sample().unwrap();
    assert_eq!(first.frames, 480);
    assert_eq!(reader.quantum(4096), 960);
    assert_eq!(reader.sample().unwrap(), first);
    clock.id = 8;
    assert_ne!(reader.sample().unwrap().epoch, first.epoch);
    clock.rate.denom = 48000;
    assert_eq!(reader.sample().unwrap().frames, 441);
    for flags in [sys::SPA_IO_CLOCK_FLAG_FREEWHEEL, u32::MAX] {
        clock.flags = flags;
        assert!(reader.sample().is_none());
    }
    clock.flags = 0;
    for (num, denom) in [(0, 48000), (1, 0), (u32::MAX, 1)] {
        clock.rate.num = num;
        clock.rate.denom = denom;
        clock.position = u64::MAX;
        assert!(reader.sample().is_none());
        for maximum in 0..1024 {
            assert!(reader.quantum(maximum) <= maximum);
        }
    }
    clock.rate.num = 1;
    clock.rate.denom = 48000;
    clock.position = 10;
    clock.nsec = 0;
    assert!(reader.sample().is_none());
    clock.nsec = 1;
    reader.position(999, std::ptr::null_mut(), 0);
    assert!(reader.sample().is_some());
    reader.position(
        sys::SPA_IO_Position,
        std::ptr::from_mut(&mut clock).cast(),
        1,
    );
    assert!(reader.sample().is_none());
    reader.position(sys::SPA_IO_Position, std::ptr::null_mut(), u32::MAX);
    assert!(reader.sample().is_none());
}

#[test]
fn t720_numeric_clock_monitor_is_owned_and_reports_unavailable_or_measured_state() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    let queue = Arc::new(Mutex::new(
        blent_config::audio::PcmQueue::new(blent_config::audio::AudioProfile::new(
            blent_config::audio::Direction::Microphone,
        ))
        .unwrap(),
    ));
    let running = Arc::new(AtomicBool::new(true));
    retire_monitor(monitor(queue.clone(), running.clone(), false));
    let worker = monitor(queue, running.clone(), true);
    std::thread::sleep(std::time::Duration::from_millis(60));
    running.store(false, Ordering::Release);
    retire_monitor(worker);
    assert!(description(None).contains("unmeasured"));
    assert!(description(Some(-500)).contains("-500 ppm"));
}
