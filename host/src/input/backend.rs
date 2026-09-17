//! Input-device contracts; protocol dispatch does not depend on Linux event layout.
use super::InputConfig;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::watch;

pub(super) struct PenSample {
    pub position: (f64, f64, f64),
    pub tilt: (f64, f64),
    pub eraser: bool,
    pub action: u8,
    pub button: Option<bool>,
}

pub(super) trait InputSink: Send + Sync {
    fn release_all(&self);
    fn touch(&self, position: (f64, f64, f64), action: u8, slot: u8);
    fn pen(&self, sample: PenSample, enabled: bool);
}

impl<T: InputSink + ?Sized> InputSink for Arc<T> {
    fn release_all(&self) {
        self.as_ref().release_all();
    }
    fn touch(&self, position: (f64, f64, f64), action: u8, slot: u8) {
        self.as_ref().touch(position, action, slot);
    }
    fn pen(&self, sample: PenSample, enabled: bool) {
        self.as_ref().pen(sample, enabled);
    }
}

pub(super) trait InputBackend: Send + Sync {
    fn sink(&self) -> Arc<dyn InputSink>;
    fn follow(
        &self,
        tablet: watch::Receiver<bool>,
        mode: watch::Receiver<bool>,
        card: watch::Receiver<Option<u32>>,
        config: InputConfig,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}
