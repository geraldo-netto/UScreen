//! T695 owned Windows event fixture, not a production capture backend.
use super::*;
use std::{cell::Cell, rc::Rc};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateEventW, SetEvent, WaitForSingleObject},
};

struct Event {
    handle: HANDLE,
    interval: Cell<u32>,
    expires: Cell<u64>,
    now: Cell<u64>,
}
impl Event {
    fn new() -> Rc<Self> {
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        assert!(!handle.is_null());
        Rc::new(Self {
            handle,
            interval: Cell::new(200),
            expires: Cell::new(0),
            now: Cell::new(0),
        })
    }
    fn interval(&self, dirty: bool) -> u32 {
        if dirty {
            return 0;
        } // Motion is never throttled by an idle request.
        if self.now.get() >= self.expires.get() {
            COMPATIBLE_MS
        } else {
            self.interval.get()
        }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        assert_ne!(unsafe { CloseHandle(self.handle) }, 0);
    }
}
struct Sender(Rc<Event>);
impl Cadence for Sender {
    fn publish(&self, interval_ms: u32) -> Result<()> {
        anyhow::ensure!(
            interval_ms == super::super::SPARSE_MS,
            "unsupported cadence"
        );
        self.0.interval.set(interval_ms);
        self.0
            .expires
            .set(self.0.now.get().saturating_add(LEASE_US));
        anyhow::ensure!(
            unsafe { SetEvent(self.0.handle) } != 0,
            "event delivery failed"
        );
        Ok(())
    }
    fn clear(&self) {
        self.0.expires.set(0);
    }
}
#[test]
fn t695_native_owned_event_delivers_and_expires_requests() {
    let event = Event::new();
    let epoch = Epoch {
        encoder: 1,
        decoder: 2,
        viewer: 3,
    };
    let mut control = Control::new(Sender(event.clone()), epoch, true);
    for sequence in 0..=16 {
        let pts = u64::from(sequence) * 200_000;
        event.now.set(pts + 110_000);
        control.encoded(
            epoch,
            Encoded {
                sequence,
                pts_us: Some(pts as i64),
                ready_us: pts + 100_000,
                keyframe: sequence % 2 == 0,
            },
        );
        control.acknowledge(epoch, u64::from(sequence) + 1, sequence, pts + 110_000);
        control.tick(epoch, true, pts + 110_000).unwrap();
    }
    assert_eq!(
        unsafe { WaitForSingleObject(event.handle, 100) },
        WAIT_OBJECT_0
    );
    assert_eq!(
        unsafe { WaitForSingleObject(event.handle, 0) },
        WAIT_TIMEOUT
    );
    assert_eq!(event.interval(false), 500);
    assert_eq!(event.interval(true), 0);
    event.now.set(event.now.get() + LEASE_US);
    assert_eq!(event.interval(false), 200);
    control.tick(epoch, true, 3_310_000).unwrap();
    assert_eq!(event.interval(false), 500);
    drop(control);
    assert_eq!(event.interval(false), 200);
    for interval in [0, 1, 200, 499, 501, u32::MAX] {
        assert!(Sender(event.clone()).publish(interval).is_err());
    }
}
