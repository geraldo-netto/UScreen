//! T391: count-bounded fanout with one backing-storage budget per session.
use crate::media::VideoPacket;
use crate::media_storage::Budget;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub(crate) const QUEUE_PACKETS: usize = 8;
pub(crate) const RETAINED_BYTES: usize = 32 * 1024 * 1024;
// Android's wire length is at most 8 MiB + 1, including type and sequence.
pub(crate) const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024 - 4;
pub(crate) const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct VideoSender {
    sender: broadcast::Sender<VideoPacket>,
    viewer_epoch: Arc<AtomicU64>,
    budget: Arc<Budget>,
    needs_idr: Arc<Mutex<bool>>,
    idr_wanted: Arc<AtomicBool>,
}

pub(crate) fn channel(
    capacity: usize,
    idr_wanted: Arc<AtomicBool>,
) -> (VideoSender, broadcast::Receiver<VideoPacket>) {
    let (sender, receiver) = broadcast::channel(capacity);
    (
        VideoSender {
            sender,
            viewer_epoch: Default::default(),
            budget: Budget::new(RETAINED_BYTES),
            needs_idr: Default::default(),
            idr_wanted,
        },
        receiver,
    )
}

impl VideoSender {
    pub fn budget(&self) -> Arc<Budget> {
        self.budget.clone()
    }

    #[cfg(test)]
    pub fn usage(&self) -> (usize, usize) {
        self.budget.usage()
    }

    #[cfg(not(feature = "inproc-encoder"))]
    pub(crate) fn viewer_epoch(&self) -> Arc<AtomicU64> {
        self.viewer_epoch.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<VideoPacket> {
        self.viewer_epoch.fetch_add(1, Ordering::AcqRel);
        self.sender.subscribe()
    }
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn send(
        &self,
        packet: VideoPacket,
    ) -> Result<usize, broadcast::error::SendError<VideoPacket>> {
        if self.receiver_count() == 0 {
            return Err(broadcast::error::SendError(packet));
        }
        let mut needs_idr = self.needs_idr.lock().unwrap();
        if (*needs_idr && !packet.is_idr) || !self.admit(&packet) {
            if !*needs_idr {
                tracing::warn!("Encoded storage or packet limit reached; resuming at an IDR");
            }
            *needs_idr = true;
            self.idr_wanted.store(true, Ordering::Release);
            return Err(broadcast::error::SendError(packet));
        }
        *needs_idr = false;
        self.sender.send(packet)
    }

    fn admit(&self, packet: &VideoPacket) -> bool {
        !packet.data.is_empty()
            && packet.data.len() <= MAX_FRAME_BYTES
            && packet.codec_config.as_ref().is_none_or(|config| {
                config.len() <= MAX_CONFIG_BYTES && config.charge(&self.budget)
            })
            && packet.data.charge(&self.budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_storage::MediaBytes;

    fn packet(seq: u32, is_idr: bool, config: &MediaBytes) -> VideoPacket {
        VideoPacket {
            data: MediaBytes::from(vec![0; 1024]),
            seq,
            is_idr,
            codec_config: Some(config.clone()),
            generation: Arc::new(AtomicBool::new(true)),
        }
    }

    #[tokio::test]
    async fn t391_budget_pressure_requires_idr_and_counts_shared_config_once() {
        let wanted = Arc::new(AtomicBool::new(false));
        let (mut tx, mut rx) = channel(8, wanted.clone());
        tx.budget = Budget::new(3 * 1024);
        let budget = tx.budget.clone();
        let config = MediaBytes::from_static(b"csd");
        tx.send(packet(0, true, &config)).ok().unwrap();
        tx.send(packet(1, false, &config)).ok().unwrap();
        assert_eq!(budget.usage().0, 2051);
        assert!(tx.send(packet(2, false, &config)).is_err());
        assert!(wanted.load(Ordering::Acquire));
        let first = rx.recv().await.unwrap();
        let last_slice = first.data.slice(0..1);
        drop(first);
        drop(rx.recv().await.unwrap());
        assert_eq!(budget.usage().0, 1027);
        assert!(tx.send(packet(3, false, &config)).is_err());
        tx.send(packet(4, true, &config)).ok().unwrap();
        assert_eq!(rx.recv().await.unwrap().seq, 4);
        drop(last_slice);
        drop(config);
        assert_eq!(budget.usage(), (0, 2051));
    }

    #[test]
    fn t391_unauthenticated_receivers_and_oversized_packets_retain_nothing() {
        let (tx, rx) = channel(8, Default::default());
        drop(rx);
        let config = MediaBytes::from_static(b"csd");
        assert!(tx.send(packet(0, true, &config)).is_err());
        assert_eq!(tx.budget.usage().0, 0);
        let _rx = tx.subscribe();
        let mut oversized = packet(1, true, &config);
        oversized.data = MediaBytes::from(vec![0; MAX_FRAME_BYTES + 1]);
        assert!(tx.send(oversized).is_err());
        assert_eq!(tx.budget.usage().0, 0);
    }
}
