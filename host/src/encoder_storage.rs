//! T402: retain only packet buffers whose complete allocation we created.
use crate::media_storage::MediaBytes;
use ffmpeg_next::codec::packet::{Mut, Ref};
use ffmpeg_next::ffi;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex, Weak};

// Small packets keep the stock allocator/pool and the existing bounded copy.
const MIN_OWNED_BYTES: usize = 65536;

#[derive(Clone, Copy)]
struct Allocation {
    data: usize,
    bytes: usize,
}

#[derive(Default)]
struct Registry {
    allocations: Mutex<HashMap<usize, Allocation>>,
    #[cfg(test)]
    releases: Arc<std::sync::atomic::AtomicUsize>,
}

#[derive(Default)]
pub(super) struct PacketStorage {
    registry: Arc<Registry>,
}

struct Release {
    registry: Weak<Registry>,
    identity: usize,
    #[cfg(test)]
    releases: Arc<std::sync::atomic::AtomicUsize>,
}

unsafe extern "C" fn free_buffer(opaque: *mut c_void, data: *mut u8) {
    // av_buffer_create invokes this exactly once after its final reference.
    // The registry may already be gone when an old viewer releases the buffer.
    let release = Box::from_raw(opaque.cast::<Release>());
    if let Some(registry) = release.registry.upgrade() {
        registry
            .allocations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&release.identity);
    }
    #[cfg(test)]
    release
        .releases
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    ffi::av_free(data.cast());
}

unsafe extern "C" fn get_buffer(
    context: *mut ffi::AVCodecContext,
    packet: *mut ffi::AVPacket,
    flags: i32,
) -> i32 {
    // Only installed for DR1 codecs. FFmpeg owns the context and supplies the
    // validated requested packet length; this opaque points to a shared Arc
    // allocation, independent of moves/reborrows of the containing Encoder.
    if (*packet).size < MIN_OWNED_BYTES as i32 {
        return ffi::avcodec_default_get_encode_buffer(context, packet, flags);
    }
    let storage = &*((*context).opaque.cast::<PacketStorage>());
    storage.allocate(packet)
}

impl PacketStorage {
    /// The caller must retain this shared allocation until the codec is closed.
    pub(super) fn install(self: &Arc<Self>, context: &mut ffmpeg_next::encoder::video::Video) {
        unsafe {
            let context = context.as_mut_ptr();
            if (*(*context).codec).capabilities & ffi::AV_CODEC_CAP_DR1 as i32 != 0 {
                (*context).opaque = Arc::as_ptr(self).cast_mut().cast();
                (*context).get_encode_buffer = Some(get_buffer);
            }
        }
    }

    unsafe fn allocate(&self, packet: *mut ffi::AVPacket) -> i32 {
        let Ok(size) = usize::try_from((*packet).size) else {
            return -libc::EINVAL;
        };
        let Some(bytes) = size.checked_add(ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize) else {
            return -libc::ENOMEM;
        };
        let data = ffi::av_malloc(bytes).cast::<u8>();
        if data.is_null() {
            return -libc::ENOMEM;
        }
        // Only padding is initialized here. libavcodec initializes the payload
        // before receive_packet exposes it; no extra full-payload zeroing.
        ptr::write_bytes(data.add(size), 0, bytes - size);
        let release = Box::into_raw(Box::new(Release {
            registry: Arc::downgrade(&self.registry),
            identity: 0,
            #[cfg(test)]
            releases: self.registry.releases.clone(),
        }));
        let buffer = ffi::av_buffer_create(data, bytes, Some(free_buffer), release.cast(), 0);
        if buffer.is_null() {
            // Public API leaves data/opaque with the caller on failure.
            free_buffer(release.cast(), data);
            return -libc::ENOMEM;
        }
        let identity = (*buffer).buffer as usize;
        (*release).identity = identity;
        self.registry
            .allocations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                identity,
                Allocation {
                    data: data as usize,
                    bytes,
                },
            );
        (*packet).data = data;
        (*packet).buf = buffer;
        0
    }

    fn allocation(&self, packet: &ffmpeg_next::Packet) -> Option<Allocation> {
        unsafe {
            let packet = &*packet.as_ptr();
            let buffer = packet.buf.as_ref()?;
            // Avoid the registry lock for the common small-packet path. A
            // small view of a large buffer also benefits from detaching it.
            if buffer.size < MIN_OWNED_BYTES {
                return None;
            }
            let allocation = *self
                .registry
                .allocations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&(buffer.buffer as usize))?;
            let size = usize::try_from(packet.size).ok()?;
            if !contains(
                allocation.data,
                allocation.bytes,
                buffer.data as usize,
                buffer.size,
            ) {
                return None;
            }
            contains(
                buffer.data as usize,
                buffer.size,
                packet.data as usize,
                size,
            )
            .then_some(allocation)
        }
    }

    pub(super) fn payload(&self, packet: ffmpeg_next::Packet) -> Option<MediaBytes> {
        let data = packet.data()?;
        if let Some(allocation) = self.allocation(&packet) {
            let owner = PacketOwner(only_buffer(packet));
            Some(MediaBytes::from_owner(owner, allocation.bytes))
        } else {
            // Unknown/non-DR1/reallocated storage may retain a larger opaque
            // backing. Detach by copying, preserving T391's whole-data budget.
            #[cfg(test)]
            crate::allocation_probe::copied(data.len());
            Some(MediaBytes::copy_from_slice(data))
        }
    }
}

fn contains(base: usize, capacity: usize, start: usize, length: usize) -> bool {
    start >= base
        && start
            .checked_add(length)
            .zip(base.checked_add(capacity))
            .is_some_and(|(end, limit)| end <= limit)
}

fn only_buffer(mut source: ffmpeg_next::Packet) -> ffmpeg_next::Packet {
    let mut result = ffmpeg_next::Packet::empty();
    unsafe {
        let source = &mut *source.as_mut_ptr();
        let target = &mut *result.as_mut_ptr();
        target.buf = std::mem::replace(&mut source.buf, ptr::null_mut());
        target.data = std::mem::replace(&mut source.data, ptr::null_mut());
        target.size = std::mem::replace(&mut source.size, 0);
    }
    // The original packet releases any side data and opaque_ref immediately;
    // they cannot silently extend the published byte buffer's retained memory.
    result
}

struct PacketOwner(ffmpeg_next::Packet);
impl AsRef<[u8]> for PacketOwner {
    fn as_ref(&self) -> &[u8] {
        self.0.data().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_storage::Budget;
    use std::sync::atomic::Ordering;

    fn packet(storage: &PacketStorage, size: i32) -> ffmpeg_next::Packet {
        let mut packet = ffmpeg_next::Packet::empty();
        unsafe {
            (*packet.as_mut_ptr()).size = size;
            assert_eq!(storage.allocate(packet.as_mut_ptr()), 0);
        }
        packet.data_mut().unwrap().fill(42);
        packet
    }

    fn one_byte_view(packet: &mut ffmpeg_next::Packet, reference_bytes: Option<usize>) {
        unsafe {
            let packet = &mut *packet.as_mut_ptr();
            (*packet.buf).data = (*packet.buf).data.add(128);
            (*packet.buf).size = reference_bytes.unwrap_or((*packet.buf).size - 128);
            packet.data = (*packet.buf).data;
            packet.size = 1;
        }
    }

    #[test]
    fn t402_subrange_charges_complete_allocation_and_releases_once_after_retirement() {
        let storage = PacketStorage::default();
        let releases = storage.registry.releases.clone();
        let mut packet = packet(&storage, 100000);
        one_byte_view(&mut packet, None);
        let data = storage.payload(packet).unwrap();
        let budget = Budget::new(100064);
        assert!(data.charge(&budget));
        assert_eq!(budget.usage().0, 100064);
        let delayed = data.clone();
        drop(data);
        drop(storage);
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        std::thread::spawn(move || {
            assert_eq!(delayed.as_ref(), &[42]);
            drop(delayed);
        })
        .join()
        .unwrap();
        assert_eq!(budget.usage().0, 0);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn t402_unknown_buffer_views_copy_and_detach() {
        let storage = PacketStorage::default();
        let mut packet = ffmpeg_next::Packet::new(100000);
        packet.data_mut().unwrap().fill(7);
        one_byte_view(&mut packet, Some(1));
        let (data, counts) = crate::allocation_probe::measure(|| storage.payload(packet).unwrap());
        assert_eq!(counts.explicit_copy_bytes, 1);
        let budget = Budget::new(1);
        assert!(data.charge(&budget));
        assert_eq!(data.as_ref(), &[7]);
    }

    #[test]
    fn t402_tiny_buffer_views_detach_without_retaining_large_allocations() {
        let storage = PacketStorage::default();
        let releases = storage.registry.releases.clone();
        let mut packet = packet(&storage, 100000);
        one_byte_view(&mut packet, Some(1));
        let (data, counts) = crate::allocation_probe::measure(|| storage.payload(packet).unwrap());
        assert_eq!(
            counts.explicit_copy_bytes, 1,
            "T402: a tiny view should detach"
        );
        assert!(data.charge(&Budget::new(1)));
        assert_eq!(releases.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn t402_publication_releases_unrelated_packet_owned_buffers() {
        let storage = PacketStorage::default();
        let releases = storage.registry.releases.clone();
        let mut data = packet(&storage, 100000);
        let mut extra = packet(&storage, 200000);
        unsafe {
            let extra = &mut *extra.as_mut_ptr();
            (*data.as_mut_ptr()).opaque_ref = std::mem::replace(&mut extra.buf, ptr::null_mut());
            extra.data = ptr::null_mut();
            extra.size = 0;
        }
        drop(extra);
        let output = storage.payload(data).unwrap();
        assert_eq!(
            releases.load(Ordering::SeqCst),
            1,
            "T402: opaque storage must not follow the payload"
        );
        assert_eq!(output.len(), 100000);
        drop(output);
        assert_eq!(releases.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn t402_multiple_packets_keep_independent_charges_during_cancellation() {
        let storage = PacketStorage::default();
        let releases = storage.registry.releases.clone();
        let first = storage.payload(packet(&storage, 100000)).unwrap();
        let second = storage.payload(packet(&storage, 100000)).unwrap();
        let capacity = 100000 + ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
        let budget = Budget::new(capacity * 2);
        assert!(first.charge(&budget));
        assert!(second.charge(&budget));
        let delayed = first.clone();
        drop(first);
        drop(second);
        drop(storage);
        assert_eq!(budget.usage().0, capacity);
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        drop(delayed);
        assert_eq!(budget.usage().0, 0);
        assert_eq!(releases.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn t402_slow_broadcast_consumer_cancellation_releases_after_encoder_retirement() {
        let storage = PacketStorage::default();
        let releases = storage.registry.releases.clone();
        let (sender, mut fast) = crate::video_queue::channel(8, Default::default());
        let mut slow = sender.subscribe();
        let budget = sender.budget();
        sender
            .send(crate::media::VideoPacket {
                data: storage.payload(packet(&storage, 100000)).unwrap(),
                is_idr: true,
                seq: 7,
                codec_config: None,
                generation: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            })
            .ok()
            .unwrap();
        drop(fast.recv().await.unwrap());
        drop(fast);
        let (ready, started) = tokio::sync::oneshot::channel();
        let (_continue, delayed) = tokio::sync::oneshot::channel::<()>();
        let consumer = tokio::spawn(async move {
            let data = slow.recv().await.unwrap();
            ready.send(()).unwrap();
            let _ = delayed.await;
            assert_eq!(data.data.len(), 100000);
        });
        started.await.unwrap();
        drop(storage);
        drop(sender);
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        assert_eq!(
            budget.usage().0,
            100000 + ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize
        );
        consumer.abort();
        assert!(consumer.await.unwrap_err().is_cancelled());
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        assert_eq!(budget.usage().0, 0);
    }
}
