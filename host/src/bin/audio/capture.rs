//! Sink graph callback and worker transfer. Only the worker performs pipe IO.
use anyhow::Result;
use blent_config::audio::{Direction, PcmBlock, PcmQueue};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const BYTES: usize = 1920;
pub(super) struct Capture {
    queue: Arc<Mutex<PcmQueue>>,
    epoch: Instant,
    bytes: [u8; BYTES],
    filled: usize,
    staged_at: Instant,
    discontinuity: bool,
    clock: super::clock::Clock,
}
impl Capture {
    pub(super) fn new(queue: Arc<Mutex<PcmQueue>>, epoch: Instant) -> Self {
        Self {
            queue,
            epoch,
            bytes: [0; BYTES],
            filled: 0,
            staged_at: epoch,
            discontinuity: false,
            clock: Default::default(),
        }
    }
    pub(super) fn position(&mut self, id: u32, pointer: *mut std::ffi::c_void, size: u32) {
        self.clock.position(id, pointer, size);
    }
    pub(super) fn chunk(&mut self, data: &[u8], offset: u32, size: u32, stride: i32) {
        let range = (offset as usize)
            .checked_add(size as usize)
            .and_then(|end| data.get(offset as usize..end));
        if !matches!(stride, 0 | 4) || size % 4 != 0 || size > 38400 || offset % 4 != 0 {
            self.discard();
            return;
        }
        let Some(bytes) = range else {
            self.discard();
            return;
        };
        self.feed(bytes);
    }
    pub(super) fn discard(&mut self) {
        self.filled = 0;
        self.discontinuity = true;
    }
    pub(super) fn silence(&mut self, size: u32) {
        if size > 38400 || size % 4 != 0 {
            self.discard();
            return;
        }
        let mut remaining = size as usize;
        while remaining != 0 {
            let count = remaining.min(BYTES);
            self.feed(&[0; BYTES][..count]);
            remaining -= count;
        }
    }
    fn feed(&mut self, mut bytes: &[u8]) {
        if self.filled != 0 && self.staged_at.elapsed() > Duration::from_millis(200) {
            self.filled = 0;
            self.discontinuity = true;
        }
        while !bytes.is_empty() {
            if self.filled == 0 {
                self.staged_at = Instant::now();
            }
            let count = bytes.len().min(BYTES - self.filled);
            self.bytes[self.filled..self.filled + count].copy_from_slice(&bytes[..count]);
            self.filled += count;
            bytes = &bytes[count..];
            if self.filled == BYTES {
                self.publish();
                self.filled = 0;
            }
        }
    }
    fn publish(&mut self) {
        let mut block = PcmBlock::from_le_bytes(Direction::Speakers, &self.bytes).unwrap();
        block.discontinuity = self.discontinuity;
        block.clock = self.clock.sample();
        self.discontinuity = true;
        if let Ok(mut queue) = self.queue.try_lock() {
            if queue
                .push(block, self.epoch.elapsed().as_millis() as u64)
                .is_ok()
            {
                self.discontinuity = false;
            }
        }
    }
}

pub(super) fn workers(
    queue: Arc<Mutex<PcmQueue>>,
    epoch: Instant,
    running: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    clocked: bool,
) {
    let lifetime = running.clone();
    std::thread::spawn(move || {
        let _ = std::io::stdin().read_exact(&mut [0; 1]);
        lifetime.store(false, Ordering::Release);
    });
    std::thread::spawn(move || {
        while running.load(Ordering::Acquire) && !ready.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
        }
        // READY is written before this worker may lock stdout.
        let _ = output_with_clock(
            &mut std::io::stdout().lock(),
            &queue,
            epoch,
            &running,
            clocked,
        );
        running.store(false, Ordering::Release);
    });
}

#[cfg(test)]
fn output(
    writer: &mut impl Write,
    queue: &Mutex<PcmQueue>,
    epoch: Instant,
    running: &AtomicBool,
) -> Result<()> {
    output_with_clock(writer, queue, epoch, running, false)
}

fn output_with_clock(
    writer: &mut impl Write,
    queue: &Mutex<PcmQueue>,
    epoch: Instant,
    running: &AtomicBool,
    clocked: bool,
) -> Result<()> {
    let mut due = Instant::now();
    while running.load(Ordering::Acquire) {
        let mut packet = vec![
            0;
            1 + BYTES
                + if clocked {
                    blent_config::audio::CLOCK_BYTES
                } else {
                    0
                }
        ];
        let pcm = packet.len() - BYTES;
        if let Some(block) = queue
            .lock()
            .map_err(|_| anyhow::anyhow!("audio queue poisoned"))?
            .pop(epoch.elapsed().as_millis() as u64)?
        {
            packet[0] = u8::from(block.discontinuity);
            if clocked {
                packet[1..pcm]
                    .copy_from_slice(&blent_config::audio::ClockSample::encode(block.clock)?);
            }
            for (sample, bytes) in block
                .samples()
                .iter()
                .zip(packet[pcm..].chunks_exact_mut(2))
            {
                bytes.copy_from_slice(&sample.to_le_bytes());
            }
        }
        writer.write_all(&packet)?;
        due += Duration::from_millis(10);
        // Slow readers cannot accumulate a burst of old pacing deadlines.
        due = due.max(Instant::now());
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use blent_config::audio::AudioProfile;
    #[test]
    fn t719_partial_native_frames_expire_across_capture_stalls() {
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap(),
        ));
        let epoch = Instant::now();
        let mut capture = Capture::new(queue.clone(), epoch);
        capture.chunk(&[1; 960], 0, 960, 4);
        std::thread::sleep(Duration::from_millis(210));
        capture.chunk(&[2; 1920], 0, 1920, 4);
        let block = queue
            .lock()
            .unwrap()
            .pop(epoch.elapsed().as_millis() as u64)
            .unwrap()
            .unwrap();
        assert!(
            block.samples().iter().all(|value| *value == 0x0202),
            "T719 stale partial native frame replayed"
        );
        assert!(block.discontinuity);
    }
    #[test]
    fn t719_capture_handles_partial_chunks_bounds_contention_and_retirement() {
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(AudioProfile::new(Direction::Speakers)).unwrap(),
        ));
        let epoch = Instant::now();
        let mut capture = Capture::new(queue.clone(), epoch);
        let bytes = [123u8; BYTES];
        capture.chunk(&bytes, 0, 960, 4);
        assert_eq!(queue.lock().unwrap().queued_frames(), 0);
        capture.chunk(&bytes, 960, 960, 4);
        assert_eq!(
            queue
                .lock()
                .unwrap()
                .pop(epoch.elapsed().as_millis() as u64)
                .unwrap()
                .unwrap()
                .samples()[0],
            0x7b7b
        );
        for (offset, size, stride) in [
            (0, 1, 4),
            (0, 1920, 2),
            (1, 960, 4),
            (1920, 4, 4),
            (u32::MAX, u32::MAX, 4),
            (0, 38404, 4),
        ] {
            capture.chunk(&bytes, offset, size, stride);
            assert_eq!(capture.filled, 0);
            assert!(capture.discontinuity);
        }
        let held = queue.lock().unwrap();
        capture.chunk(&bytes, 0, 1920, 0);
        drop(held);
        assert!(capture.discontinuity);
        for size in [1, 1919, 38401, u32::MAX] {
            capture.silence(size);
            assert_eq!(capture.filled, 0);
        }
        capture.chunk(&bytes, 0, 1920, 4);
        assert!(
            queue
                .lock()
                .unwrap()
                .pop(epoch.elapsed().as_millis() as u64)
                .unwrap()
                .unwrap()
                .discontinuity
        );
        assert!(output(&mut std::io::sink(), &queue, epoch, &AtomicBool::new(false)).is_ok());
        assert!(output(
            &mut &mut [0u8; 2][..],
            &queue,
            epoch,
            &AtomicBool::new(true)
        )
        .is_err());
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    #[test]
    fn t720_capture_attaches_native_graph_clock_and_clocked_ipc_preserves_it() {
        let queue = Arc::new(Mutex::new(
            PcmQueue::new(blent_config::audio::AudioProfile::new(Direction::Speakers)).unwrap(),
        ));
        let epoch = Instant::now();
        let mut capture = Capture::new(queue.clone(), epoch);
        let mut clock: pipewire::spa::sys::spa_io_clock = unsafe { std::mem::zeroed() };
        clock.rate.num = 1;
        clock.rate.denom = 48000;
        clock.position = 96000;
        clock.nsec = 2_000_000_001;
        capture.position(
            pipewire::spa::sys::SPA_IO_Position,
            std::ptr::from_mut(&mut clock).cast(),
            std::mem::size_of_val(&clock) as u32,
        );
        capture.chunk(&[1; BYTES], 0, BYTES as u32, 4);
        let block = queue
            .lock()
            .unwrap()
            .pop(epoch.elapsed().as_millis() as u64)
            .unwrap()
            .unwrap();
        assert_eq!(block.clock.unwrap().frames, 96000);
        queue
            .lock()
            .unwrap()
            .push(block, epoch.elapsed().as_millis() as u64)
            .unwrap();
        let mut buffer = [0; 1945];
        assert!(output_with_clock(
            &mut &mut buffer[..],
            &queue,
            epoch,
            &AtomicBool::new(true),
            true
        )
        .is_err());
        let stamp = blent_config::audio::ClockSample::decode(&buffer[1..25])
            .unwrap()
            .unwrap();
        assert_eq!(stamp.frames, 96000);
        assert_eq!(&buffer[25..], &[1; BYTES]);
        assert!(output_with_clock(
            &mut std::io::sink(),
            &queue,
            epoch,
            &AtomicBool::new(false),
            true
        )
        .is_ok());
    }
}
