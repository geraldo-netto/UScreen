//! Linux sealed memfd adapter. Pixel mappings are immutable in this process.
use crate::raw_socket::Socket;
use anyhow::{ensure, Context, Result};
use std::os::fd::{AsRawFd, OwnedFd};
use std::ptr::NonNull;
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc,
};
use uscreen_config::raw_frame::*;

pub(crate) struct Mapping {
    pointer: NonNull<u8>,
    pub descriptor: Message,
    socket: Socket,
}
// Shared control uses process-shared lock-free atomics. Only leased slots expose
// pixel bytes, after acquire of READY; no producer writes until final release.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn open(descriptor: Message, fd: OwnedFd, socket: Socket) -> Result<Arc<Self>> {
        ensure!(descriptor.kind == DESCRIPTOR, "expected raw descriptor");
        Message::decode(&descriptor.encode())?;
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } == 0,
            "stat raw memfd failed"
        );
        ensure!(
            stat.st_size as u64 == descriptor.layout.total_bytes,
            "wrong raw memfd size"
        );
        let seals = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GET_SEALS) };
        let required = libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL;
        ensure!(
            seals >= 0 && seals & required == required,
            "raw memfd lacks immutable size seals"
        );
        let size = descriptor.layout.total_bytes as usize;
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        ensure!(
            pointer != libc::MAP_FAILED,
            "map raw memfd: {}",
            std::io::Error::last_os_error()
        );
        let mapping = Self {
            pointer: NonNull::new(pointer.cast()).context("null raw mapping")?,
            descriptor,
            socket,
        };
        let result = unsafe {
            libc::mprotect(
                mapping.pointer.as_ptr().add(CONTROL_BYTES).cast(),
                size - CONTROL_BYTES,
                libc::PROT_READ,
            )
        };
        ensure!(
            result == 0,
            "protect raw pixels: {}",
            std::io::Error::last_os_error()
        );
        Ok(Arc::new(mapping))
    }

    fn state(&self, slot: u32) -> &AtomicU32 {
        // Callers validated slot bounds before accessing either control field.
        unsafe {
            &*self
                .pointer
                .as_ptr()
                .add(slot as usize * SLOT_CONTROL_BYTES)
                .cast::<AtomicU32>()
        }
    }

    fn sequence(&self, slot: u32) -> &AtomicU64 {
        unsafe {
            &*self
                .pointer
                .as_ptr()
                .add(slot as usize * SLOT_CONTROL_BYTES + 8)
                .cast::<AtomicU64>()
        }
    }

    pub fn lease(self: &Arc<Self>, message: Message) -> Result<Lease> {
        ensure!(
            message.kind == FRAME && message.layout == self.descriptor.layout,
            "mismatched raw frame layout"
        );
        ensure!(
            message.generation == self.descriptor.generation
                && message.nonce == self.descriptor.nonce,
            "stale raw generation"
        );
        message.layout.slot_offset(message.slot)?;
        ensure!(
            self.state(message.slot)
                .compare_exchange(READY, READING, Ordering::Acquire, Ordering::Relaxed)
                .is_ok(),
            "raw slot not ready"
        );
        if self.sequence(message.slot).load(Ordering::Relaxed) != message.sequence {
            self.state(message.slot).store(READY, Ordering::Release);
            anyhow::bail!("stale raw slot sequence");
        }
        Ok(Lease {
            mapping: self.clone(),
            message,
        })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(
                self.pointer.as_ptr().cast(),
                self.descriptor.layout.total_bytes as usize,
            );
        }
    }
}

pub(crate) struct Lease {
    mapping: Arc<Mapping>,
    pub message: Message,
}

impl Lease {
    pub fn bytes(&self) -> &[u8] {
        let offset =
            CONTROL_BYTES + self.message.slot as usize * self.message.layout.slot_bytes as usize;
        unsafe {
            std::slice::from_raw_parts(
                self.mapping.pointer.as_ptr().add(offset),
                self.message.layout.frame_bytes(),
            )
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.mapping
            .state(self.message.slot)
            .store(FREE, Ordering::Release);
        // Never block libavcodec's last-reference callback. Control polling also
        // notices free slots, so a full notification socket loses no capacity.
        let _ = self.mapping.socket.send(
            Message {
                kind: RELEASE,
                ..self.message
            },
            None,
        );
    }
}

pub(crate) struct Reader {
    socket: Socket,
    mapping: Option<Arc<Mapping>>,
    dimensions: (u32, u32),
    last_sequence: u64,
    nonce: u64,
}

impl Reader {
    pub fn new(socket: Socket, dimensions: (u32, u32), slots: u32) -> Result<Self> {
        static NEXT_NONCE: AtomicU64 = AtomicU64::new(1);
        let nonce = NEXT_NONCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("raw handshake nonce exhausted"))?;
        let layout = Layout::new(dimensions.0, dimensions.1, slots)?;
        socket.send(
            Message {
                kind: HELLO,
                generation: 1,
                nonce,
                layout,
                sequence: 0,
                slot: 0,
                captured_us: 0,
            },
            None,
        )?;
        Ok(Self {
            socket,
            mapping: None,
            dimensions,
            last_sequence: 0,
            nonce,
        })
    }

    pub fn next(&mut self, stop: &crate::encoder_io::StopSignal) -> Result<Option<Lease>> {
        while self.socket.wait(stop)? {
            let Some((message, mut fds)) = self.socket.receive()? else {
                continue;
            };
            if message.nonce != self.nonce {
                continue;
            }
            if message.kind == DESCRIPTOR {
                ensure!(fds.len() == 1, "raw descriptor requires exactly one fd");
                ensure!(
                    self.mapping
                        .as_ref()
                        .is_none_or(|old| message.generation > old.descriptor.generation),
                    "replayed raw descriptor"
                );
                self.mapping = Some(Mapping::open(message, fds.remove(0), self.socket.clone())?);
                self.last_sequence = 0;
            } else {
                ensure!(fds.is_empty(), "unexpected raw frame fd");
                if let Some(lease) = self.frame(message)? {
                    return Ok(Some(lease));
                }
            }
        }
        Ok(None)
    }

    fn frame(&mut self, message: Message) -> Result<Option<Lease>> {
        let mapping = self
            .mapping
            .as_ref()
            .context("raw frame before descriptor")?;
        ensure!(message.sequence > self.last_sequence, "replayed raw frame");
        let lease = mapping.lease(message)?;
        self.last_sequence = message.sequence;
        if (message.layout.width, message.layout.height) != self.dimensions {
            return Ok(None); // Old mode frames are released while supervisor restarts.
        }
        Ok(Some(lease))
    }
}
