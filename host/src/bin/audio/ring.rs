use blent_config::audio::{PcmQueue, BLOCK_FRAMES};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

pub(super) struct Render {
    queue: Arc<Mutex<PcmQueue>>,
    epoch: Instant,
    block: [i16; BLOCK_FRAMES],
    cursor: usize,
    at: u64,
    clock: super::clock::Clock,
}
impl Render {
    pub(super) fn new(queue: Arc<Mutex<PcmQueue>>, epoch: Instant) -> Self {
        Self {
            queue,
            epoch,
            block: [0; BLOCK_FRAMES],
            cursor: BLOCK_FRAMES,
            at: 0,
            clock: Default::default(),
        }
    }
    pub(super) fn position(&mut self, id: u32, pointer: *mut std::ffi::c_void, size: u32) {
        self.clock.position(id, pointer, size);
    }
    pub(super) fn quantum(&self, maximum: usize) -> usize {
        self.clock.quantum(maximum)
    }
    pub(super) fn fill(&mut self, output: &mut [u8]) {
        output.fill(0);
        let now = self.epoch.elapsed().as_millis() as u64;
        if now.saturating_sub(self.at) > 200 {
            self.cursor = BLOCK_FRAMES;
        }
        for bytes in output.chunks_exact_mut(2) {
            if self.cursor == BLOCK_FRAMES {
                self.next(now);
            }
            bytes.copy_from_slice(&self.block[self.cursor].to_le_bytes());
            self.cursor += 1;
        }
    }
    fn next(&mut self, now: u64) {
        self.block.fill(0);
        if let Ok(mut queue) = self.queue.try_lock() {
            queue.native_clock(self.clock.sample(), self.epoch.elapsed().as_millis() as u64);
            // Serialize the clock sample with queue updates, not callback entry.
            let _ = queue.render(self.epoch.elapsed().as_millis() as u64, &mut self.block);
        }
        self.cursor = 0;
        self.at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blent_config::audio::{AudioProfile, Direction, PcmBlock};
    #[test]
    fn t718_render_bounds_underflow_lock_contention_and_clock() {
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap(),
        ));
        let mut render = Render::new(queue.clone(), Instant::now());
        for size in 0..1024 {
            let mut bytes = vec![99; size];
            render.fill(&mut bytes);
            assert!(bytes.iter().all(|b| *b == 0));
            assert!(render.quantum(size) <= size);
        }
        let guard = queue.lock().unwrap();
        render.fill(&mut [1; 1920]);
        drop(guard);
        for _ in 0..4 {
            queue
                .lock()
                .unwrap()
                .push(
                    PcmBlock::from_le_bytes(Direction::Microphone, &[1; 960]).unwrap(),
                    render.epoch.elapsed().as_millis() as u64,
                )
                .unwrap();
        }
        render.cursor = BLOCK_FRAMES;
        let mut bytes = [0; 960];
        render.fill(&mut bytes);
        assert!(bytes.iter().any(|b| *b != 0));
        render.epoch = Instant::now() - std::time::Duration::from_secs(1);
        render.fill(&mut bytes);
        assert_eq!(bytes, [0; 960]);
        let mut clock: pipewire::spa::sys::spa_io_clock = unsafe { std::mem::zeroed() };
        clock.duration = 128;
        clock.rate.num = 1;
        clock.rate.denom = 48000;
        render.position(
            pipewire::spa::sys::SPA_IO_Position,
            std::ptr::from_mut(&mut clock).cast(),
            std::mem::size_of_val(&clock) as u32,
        );
        assert_eq!(render.quantum(4096), 256);
        render.position(999, std::ptr::null_mut(), 0);
        assert_eq!(render.quantum(4096), 256);
        render.position(pipewire::spa::sys::SPA_IO_Position, std::ptr::null_mut(), 0);
        assert_eq!(render.quantum(4096), 960);
    }
    #[test]
    fn t718_callback_clock_is_sampled_after_acquiring_queue() {
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(AudioProfile::new(Direction::Microphone)).unwrap(),
        ));
        let mut render = Render::new(
            queue.clone(),
            Instant::now() - std::time::Duration::from_millis(10),
        );
        for _ in 0..4 {
            queue
                .lock()
                .unwrap()
                .push(
                    PcmBlock::from_le_bytes(Direction::Microphone, &[1; 960]).unwrap(),
                    10,
                )
                .unwrap();
        }
        // A worker can push between callback entry and its successful try_lock.
        render.next(9);
        assert!(
            render.block.iter().any(|sample| *sample != 0),
            "T718 stale callback timestamp caused false silence"
        );
    }
}
