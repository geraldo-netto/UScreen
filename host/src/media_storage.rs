//! T391: immutable bytes retain their full backing-allocation charge across slices.
use bytes::Bytes;
use std::ops::Deref;
#[cfg(test)]
use std::ops::RangeBounds;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) struct Budget {
    limit: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
}

impl Budget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }

    fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Charge> {
        let old = self
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= self.limit)
            })
            .ok()?;
        self.peak.fetch_max(old + bytes, Ordering::Relaxed);
        Some(Charge {
            budget: self.clone(),
            bytes,
        })
    }

    #[cfg(test)]
    pub fn usage(&self) -> (usize, usize) {
        (
            self.used.load(Ordering::Acquire),
            self.peak.load(Ordering::Relaxed),
        )
    }
}

impl Drop for Budget {
    fn drop(&mut self) {
        tracing::debug!(
            peak_bytes = self.peak.load(Ordering::Relaxed),
            "Encoded storage budget retired"
        );
    }
}

struct Charge {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct Backing {
    bytes: usize,
    charge: Mutex<Option<Charge>>,
}

/// The raw Bytes never escapes this wrapper: cloning or slicing preserves the
/// backing identity and its charge, even when only one byte remains visible.
#[derive(Clone)]
pub(crate) struct MediaBytes {
    data: Bytes,
    backing: Arc<Backing>,
}

impl MediaBytes {
    fn owned(data: Bytes, bytes: usize) -> Self {
        assert!(bytes >= data.len());
        Self {
            data,
            backing: Arc::new(Backing {
                bytes,
                charge: Mutex::new(None),
            }),
        }
    }

    #[cfg(not(feature = "inproc-encoder"))]
    pub fn new() -> Self {
        Self::owned(Bytes::new(), 0)
    }

    #[cfg(test)]
    pub fn from_static(data: &'static [u8]) -> Self {
        // Conservatively count static storage too; do not special-case tests.
        Self::owned(Bytes::from_static(data), data.len())
    }

    #[cfg(feature = "inproc-encoder")]
    pub fn copy_from_slice(data: &[u8]) -> Self {
        Self::owned(Bytes::copy_from_slice(data), data.len())
    }

    /// T402: callers must establish the complete retained data allocation,
    /// including padding; an arbitrary AVBufferRef view is not sufficient.
    #[cfg(feature = "inproc-encoder")]
    pub fn from_owner(owner: impl AsRef<[u8]> + Send + 'static, bytes: usize) -> Self {
        Self::owned(Bytes::from_owner(owner), bytes)
    }

    #[cfg(all(test, feature = "inproc-encoder"))]
    pub fn from_benchmark_owner(owner: impl AsRef<[u8]> + Send + 'static, bytes: usize) -> Self {
        Self::from_owner(owner, bytes)
    }

    #[cfg(test)]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        Self {
            data: self.data.slice(range),
            backing: self.backing.clone(),
        }
    }

    pub fn charge(&self, budget: &Arc<Budget>) -> bool {
        let mut charge = self.backing.charge.lock().unwrap();
        if let Some(charge) = charge.as_ref() {
            return Arc::ptr_eq(&charge.budget, budget);
        }
        *charge = budget.reserve(self.backing.bytes);
        charge.is_some()
    }
}

impl From<Vec<u8>> for MediaBytes {
    fn from(data: Vec<u8>) -> Self {
        let bytes = data.capacity();
        Self::owned(Bytes::from(data), bytes)
    }
}

impl Deref for MediaBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.data
    }
}
impl AsRef<[u8]> for MediaBytes {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}
impl PartialEq for MediaBytes {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}
impl Eq for MediaBytes {}
impl std::fmt::Debug for MediaBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.data.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t391_slice_retains_full_capacity_once_until_final_release() {
        let budget = Budget::new(4096);
        let mut source = Vec::with_capacity(4096);
        source.extend_from_slice(b"small visible data");
        let data = MediaBytes::from(source);
        let slice = data.slice(1..2);
        assert!(slice.charge(&budget));
        assert!(data.charge(&budget));
        assert_eq!(budget.usage(), (4096, 4096));
        drop(data);
        assert_eq!(slice.as_ref(), b"m");
        assert_eq!(budget.usage().0, 4096);
        assert!(!MediaBytes::from_static(b"x").charge(&budget));
        drop(slice);
        assert_eq!(budget.usage(), (0, 4096));
        assert!(MediaBytes::from_static(b"x").charge(&budget));
        assert_eq!(budget.usage(), (0, 4096));
    }

    #[test]
    fn t391_shared_backing_cannot_escape_to_an_unaccounted_session() {
        let data = MediaBytes::from(vec![1; 32]);
        let first = Budget::new(32);
        let second = Budget::new(32);
        assert!(data.charge(&first));
        assert!(!data.clone().charge(&second));
        assert_eq!(first.usage().0, 32);
        assert_eq!(second.usage().0, 0);
    }
}
