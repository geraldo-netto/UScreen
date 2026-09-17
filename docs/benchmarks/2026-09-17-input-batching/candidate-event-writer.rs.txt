//! Native event serialization and SYN-bounded writes (T406).
use std::io::{self, Write};

#[cfg(test)]
pub(super) type LinuxInputEvent = libc::input_event;

pub(super) const EVENT_BYTES: usize = std::mem::size_of::<libc::input_event>();
// Current largest frame is ten touch-slot releases plus legacy axes/SYN (24
// events). Leave bounded headroom without ever splitting an intentional frame.
pub(super) const MAX_EVENTS: usize = 64;
const BUFFER_BYTES: usize = EVENT_BYTES * MAX_EVENTS;

pub(super) struct EventBatch {
    storage: [u8; BUFFER_BYTES],
    pending: usize,
}
impl Default for EventBatch {
    fn default() -> Self {
        Self {
            storage: [0; BUFFER_BYTES],
            pending: 0,
        }
    }
}
impl EventBatch {
    pub(super) fn push(&mut self, kind: u16, code: u16, value: i32) -> io::Result<()> {
        if self.pending == MAX_EVENTS {
            self.pending = 0;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "uinput frame exceeds event bound",
            ));
        }
        let offset = self.pending * EVENT_BYTES;
        let event = &mut self.storage[offset..offset + EVENT_BYTES];
        // Serialize fields into zeroed native-layout storage. Reading Rust
        // struct padding as bytes can expose uninitialized memory; the kernel
        // supplies timestamps, so all timestamp/padding bytes stay zero.
        let kind_at = std::mem::offset_of!(libc::input_event, type_);
        let code_at = std::mem::offset_of!(libc::input_event, code);
        let value_at = std::mem::offset_of!(libc::input_event, value);
        event[kind_at..kind_at + 2].copy_from_slice(&kind.to_ne_bytes());
        event[code_at..code_at + 2].copy_from_slice(&code.to_ne_bytes());
        event[value_at..value_at + 4].copy_from_slice(&value.to_ne_bytes());
        self.pending += 1;
        Ok(())
    }
    pub(super) fn finish(&mut self, writer: &mut impl Write) -> io::Result<()> {
        self.push(super::linux::EV_SYN, super::linux::SYN_REPORT, 0)?;
        // A failed write can have consumed a prefix. Retire the batch before
        // I/O so a later sample cannot replay those already-delivered bytes.
        let bytes = std::mem::take(&mut self.pending) * EVENT_BYTES;
        writer.write_all(&self.storage[..bytes])?;
        writer.flush()
    }
}
