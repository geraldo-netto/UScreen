//! PipeWire driver-clock adapter; no callback IO, allocation or blocking.
use blent_config::audio::ClockSample;
use pipewire::spa::sys;

#[derive(Default)]
pub(super) struct Clock {
    pointer: *const sys::spa_io_clock,
    epoch: u64,
    signature: Option<(u32, u32, u32)>,
}
impl Clock {
    pub(super) fn position(&mut self, id: u32, pointer: *mut std::ffi::c_void, size: u32) {
        if id != sys::SPA_IO_Position {
            return;
        }
        let minimum = std::mem::offset_of!(sys::spa_io_clock, duration) + 8;
        self.pointer = if size as usize >= minimum {
            pointer.cast()
        } else {
            std::ptr::null()
        };
        self.epoch = self.epoch % i64::MAX as u64 + 1;
        self.signature = None;
    }
    pub(super) fn sample(&mut self) -> Option<ClockSample> {
        if self.pointer.is_null() {
            return None;
        }
        // The pointer lifetime ends at io_changed. Read only the checked prefix.
        let (id, flags, rate, frames, nanos) = unsafe {
            (
                std::ptr::addr_of!((*self.pointer).id).read_unaligned(),
                std::ptr::addr_of!((*self.pointer).flags).read_unaligned(),
                std::ptr::addr_of!((*self.pointer).rate).read_unaligned(),
                std::ptr::addr_of!((*self.pointer).position).read_unaligned(),
                std::ptr::addr_of!((*self.pointer).nsec).read_unaligned(),
            )
        };
        if flags & sys::SPA_IO_CLOCK_FLAG_FREEWHEEL != 0 || rate.num == 0 || rate.denom == 0 {
            return None;
        }
        let signature = (id, rate.num, rate.denom);
        if self.signature != Some(signature) {
            self.signature = Some(signature);
            self.epoch = self.epoch % i64::MAX as u64 + 1;
        }
        let frames = u64::try_from(
            u128::from(frames) * u128::from(rate.num) * 48000 / u128::from(rate.denom),
        )
        .ok()?;
        let sample = ClockSample {
            epoch: self.epoch,
            frames,
            nanos,
        };
        sample.valid().then_some(sample)
    }
    pub(super) fn quantum(&self, maximum: usize) -> usize {
        if self.pointer.is_null() {
            return 480.min(maximum / 2) * 2;
        }
        let (duration, rate) = unsafe {
            (
                std::ptr::addr_of!((*self.pointer).duration).read_unaligned(),
                std::ptr::addr_of!((*self.pointer).rate).read_unaligned(),
            )
        };
        let frames = duration
            .saturating_mul(48000)
            .saturating_mul(rate.num as u64)
            / u64::from(rate.denom.max(1));
        (frames.min((maximum / 2) as u64) as usize) * 2
    }
}

/// Numeric diagnostics run on an owned ordinary worker, outside queue locks.
pub(super) fn monitor(
    queue: std::sync::Arc<std::sync::Mutex<blent_config::audio::PcmQueue>>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    enabled: bool,
) -> Option<std::thread::JoinHandle<()>> {
    if !enabled {
        return None;
    }
    std::thread::Builder::new()
        .name("blent-audio-clock".into())
        .spawn(move || {
            let mut next = std::time::Instant::now();
            while running.load(std::sync::atomic::Ordering::Acquire) {
                if std::time::Instant::now() >= next {
                    let measured = queue.try_lock().ok().map(|queue| queue.measured_drift());
                    if let Some(measured) = measured {
                        eprintln!("{}", description(measured));
                    }
                    next = std::time::Instant::now() + std::time::Duration::from_secs(5);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
        .ok()
}
fn description(measured: Option<i32>) -> String {
    match measured {
        Some(ppm) => format!("Blent microphone native drift correction: {ppm} ppm"),
        None => {
            "Blent microphone native clock drift unmeasured (warming, reset or unavailable)".into()
        }
    }
}
pub(super) fn retire_monitor(monitor: Option<std::thread::JoinHandle<()>>) {
    if let Some(monitor) = monitor {
        let _ = monitor.join();
    }
}

#[cfg(test)]
#[path = "clock/tests.rs"]
mod tests;
