//! T697: requested capacity is policy; effective capacity requires adapter evidence.
use crate::model::PIPE_CAPACITIES_MIB;
use anyhow::{ensure, Result};

pub use crate::model::MAX_CONVERSION_THREADS as MAX_WORKERS;
const MIB: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub buffer_mib: u32,
    /// Zero means Auto. Capacity includes the calling conversion worker.
    pub workers: u32,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            buffer_mib: 1,
            workers: 0,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    request: Request,
    bytes: usize,
    workers: u32,
}
impl Request {
    pub fn plan(self, available_cpus: u32) -> Result<Plan> {
        ensure!(
            PIPE_CAPACITIES_MIB.contains(&self.buffer_mib),
            "Use 1, 2, 4 or 8 MiB capture capacity"
        );
        ensure!(
            self.workers <= MAX_WORKERS,
            "Use Auto or 1–128 conversion workers"
        );
        ensure!(available_cpus > 0, "Available CPU count is unknown");
        let workers = if self.workers == 0 {
            available_cpus.saturating_sub(2).clamp(1, MAX_WORKERS)
        } else {
            self.workers
        };
        Ok(Plan {
            request: self,
            bytes: self.buffer_mib as usize * MIB,
            workers,
        })
    }
}
/// Native backends own allocation and bounds checks; no kernel FIFO assumption.
pub trait Buffer {
    fn capacity(&self) -> usize;
    fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<()>;
    fn read(&self, offset: usize, length: usize) -> Option<&[u8]>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    pub requested: Request,
    pub buffer_bytes: usize,
    pub worker_capacity: u32,
}
impl Plan {
    pub fn buffer_bytes(self) -> usize {
        self.bytes
    }
    pub fn worker_capacity(self) -> u32 {
        self.workers
    }
    /// Call only after allocating the buffer and starting the reported workers.
    /// Reduced capacity is explicit; impossible evidence is rejected.
    pub fn record(self, buffer: &impl Buffer, started_workers: u32) -> Result<Effective> {
        let capacity = buffer.capacity();
        ensure!(
            capacity > 0 && capacity <= self.bytes,
            "Invalid effective buffer capacity"
        );
        ensure!(
            started_workers > 0 && started_workers <= self.workers,
            "Invalid effective conversion capacity"
        );
        Ok(Effective {
            requested: self.request,
            buffer_bytes: capacity,
            worker_capacity: started_workers,
        })
    }
}
impl Effective {
    /// Idle work uses no workers; dirty work never exceeds started capacity.
    pub fn active_workers(self, work_units: u32) -> u32 {
        self.worker_capacity.min(work_units)
    }
}

#[cfg(test)]
mod tests;
