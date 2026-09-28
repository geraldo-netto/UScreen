//! Owned Windows committed memory for a future capture transport adapter.
//! This is not a named pipe, a frame queue, or an integrated converter.
use crate::capture_capacity::{Buffer, Plan};
use anyhow::{Context, Result};
use std::ptr::NonNull;
use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualFree, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};

pub struct OwnedBuffer {
    address: NonNull<u8>,
    bytes: usize,
}
impl OwnedBuffer {
    pub fn new(plan: Plan) -> Result<Self> {
        let bytes = plan.buffer_bytes();
        // Plan bounds the allocation to 1..8 MiB. VirtualAlloc commits zeroed,
        // writable pages; the allocation is retained until this owner drops.
        let address = unsafe {
            VirtualAlloc(
                std::ptr::null(),
                bytes,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        let address = NonNull::new(address.cast::<u8>())
            .context("Windows capture buffer allocation failed")?;
        Ok(Self { address, bytes })
    }
}
impl Buffer for OwnedBuffer {
    fn capacity(&self) -> usize {
        self.bytes
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .context("Capture buffer offset overflow")?;
        anyhow::ensure!(end <= self.bytes, "Capture buffer write out of bounds");
        // Exclusive borrow, checked range; source cannot alias this owner safely.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.address.as_ptr().add(offset),
                bytes.len(),
            );
        }
        Ok(())
    }
    fn read(&self, offset: usize, length: usize) -> Option<&[u8]> {
        let end = offset.checked_add(length)?;
        if end > self.bytes {
            return None;
        }
        // The borrow prevents mutation/free for the returned slice's lifetime.
        Some(unsafe { std::slice::from_raw_parts(self.address.as_ptr().add(offset), length) })
    }
}
impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // MEM_RELEASE requires the original base and a zero size.
        unsafe {
            VirtualFree(self.address.as_ptr().cast(), 0, MEM_RELEASE);
        }
    }
}

#[cfg(test)]
mod tests;
