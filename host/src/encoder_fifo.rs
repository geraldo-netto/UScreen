//! Linux FIFO readiness and latched cancellation for the optional encoder.
use super::{FrameSource, Waiting};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(crate) struct StopSignal {
    requested: AtomicBool,
    event: OwnedFd,
}
impl StopSignal {
    pub fn as_raw_fd(&self) -> RawFd {
        self.event.as_raw_fd()
    }
    pub fn new() -> io::Result<Arc<Self>> {
        let event =
            descriptor(unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) })?;
        Ok(Arc::new(Self {
            requested: AtomicBool::new(false),
            event,
        }))
    }
    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    pub fn request(&self) {
        if self.requested.swap(true, Ordering::AcqRel) {
            return;
        }
        let value = 1u64;
        loop {
            let count =
                unsafe { libc::write(self.event.as_raw_fd(), (&value as *const u64).cast(), 8) };
            if count == 8 {
                return;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() != io::ErrorKind::WouldBlock {
                tracing::error!("Cannot notify encoder cancellation: {error}");
            }
            return;
        }
    }
}

fn descriptor(raw: RawFd) -> io::Result<OwnedFd> {
    if raw < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }
}

/// Register before the first read so an EOF-to-writer-open transition cannot
/// fall between observing no writer and subscribing to inode notifications.
pub(crate) struct FifoReader<T> {
    source: T,
    events: Option<std::fs::File>,
}
impl<T: Read + AsRawFd> FifoReader<T> {
    pub fn new(source: T) -> Self {
        let events = match watch_fifo(source.as_raw_fd()) {
            Ok(events) => Some(events),
            Err(error) => {
                tracing::warn!(
                    "FIFO open notifications unavailable; using cancellable EOF retry: {error}"
                );
                None
            }
        };
        Self { source, events }
    }

    fn descriptors(&self, stop: &StopSignal, waiting: Waiting) -> [libc::pollfd; 3] {
        let data = if waiting == Waiting::Data {
            self.source.as_raw_fd()
        } else {
            -1
        };
        let events = if waiting == Waiting::Writer {
            self.events.as_ref().map_or(-1, AsRawFd::as_raw_fd)
        } else {
            -1
        };
        [
            poll_descriptor(stop.event.as_raw_fd()),
            poll_descriptor(data),
            poll_descriptor(events),
        ]
    }

    fn consume_events(&mut self, ready: bool) -> io::Result<()> {
        if !ready {
            return Ok(());
        }
        if let Some(events) = &mut self.events {
            if !drain_events(events)? {
                self.events = None;
            }
        }
        Ok(())
    }
}
impl<T: Read> Read for FifoReader<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.source.read(bytes)
    }
}
impl<T: Read + AsRawFd> FrameSource for FifoReader<T> {
    fn wait(&mut self, waiting: Waiting, stop: &StopSignal) -> io::Result<bool> {
        let mut descriptors = self.descriptors(stop, waiting);
        // poll(POLLIN) on a writerless FIFO can report HUP forever. Wait only
        // for inode/open events in that state; retain the old 5ms retry if
        // inotify is unavailable/removed. Cancellation remains event-driven.
        let timeout = if waiting == Waiting::Writer && self.events.is_none() {
            5
        } else {
            -1
        };
        if !poll_ready(&mut descriptors, timeout, stop)? {
            return Ok(false);
        }
        self.consume_events(descriptors[2].revents != 0)?;
        Ok(!stop.requested())
    }
}

fn poll_descriptor(fd: RawFd) -> libc::pollfd {
    libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }
}
fn poll_ready(
    descriptors: &mut [libc::pollfd],
    timeout: i32,
    stop: &StopSignal,
) -> io::Result<bool> {
    while !stop.requested() {
        let result = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                timeout,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if descriptors
            .iter()
            .any(|fd| fd.revents & libc::POLLNVAL != 0)
        {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        return Ok(!stop.requested());
    }
    Ok(false)
}

fn watch_fifo(source: RawFd) -> io::Result<std::fs::File> {
    let descriptor =
        descriptor(unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) })?;
    // Follow the owned descriptor's inode, including across path replacement.
    // If procfs is unavailable, retain the cancellable EOF retry fallback.
    let path = std::ffi::CString::new(format!("/proc/self/fd/{source}"))?;
    let mask = libc::IN_OPEN | libc::IN_CLOSE_WRITE | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;
    let watch = unsafe { libc::inotify_add_watch(descriptor.as_raw_fd(), path.as_ptr(), mask) };
    if watch < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(descriptor.into())
}

fn drain_events(events: &mut std::fs::File) -> io::Result<bool> {
    let mut buffer = [0; 4096];
    loop {
        match events.read(&mut buffer) {
            Ok(0) => return Ok(false),
            Ok(count) => {
                if removed_watch(&buffer[..count]) {
                    return Ok(false);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}
fn removed_watch(mut data: &[u8]) -> bool {
    let header = std::mem::size_of::<libc::inotify_event>();
    while data.len() >= header {
        // inotify records need not be aligned in this byte array.
        let event = unsafe { data.as_ptr().cast::<libc::inotify_event>().read_unaligned() };
        if event.mask & (libc::IN_IGNORED | libc::IN_DELETE_SELF | libc::IN_MOVE_SELF) != 0 {
            return true;
        }
        let Some(rest) = data
            .get(header..)
            .and_then(|names| names.get(event.len as usize..))
        else {
            return true;
        };
        data = rest;
    }
    false
}

#[cfg(test)]
#[path = "encoder_fifo_tests.rs"]
mod tests;
