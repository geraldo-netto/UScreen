//! Deterministic wakeup assertions for event-driven waits (T405).
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::task::{Context, Poll};

#[derive(Default)]
pub(crate) struct Probe(AtomicUsize);
impl futures_util::task::ArcWake for Probe {
    fn wake_by_ref(probe: &Arc<Self>) {
        probe.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl Probe {
    pub fn poll<F: Future>(self: &Arc<Self>, future: Pin<&mut F>) -> Poll<F::Output> {
        let waker = futures_util::task::waker(self.clone());
        future.poll(&mut Context::from_waker(&waker))
    }
    pub fn take(&self) -> usize {
        self.0.swap(0, Ordering::SeqCst)
    }
}
