//! T390: owned, bounded work per transport; completion never waits for peers.
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use tokio::task::{JoinError, JoinHandle};

type Pending<T> = (String, Pin<Box<dyn Future<Output = T> + Send>>);

pub(crate) struct DeviceTasks<T> {
    limit: usize,
    queued: VecDeque<Pending<T>>,
    active: HashMap<String, JoinHandle<T>>,
}

impl<T: Send + 'static> DeviceTasks<T> {
    pub fn new(limit: usize) -> Self {
        assert!(limit > 0);
        Self {
            limit,
            queued: VecDeque::new(),
            active: HashMap::new(),
        }
    }

    pub fn contains(&self, serial: &str) -> bool {
        self.active.contains_key(serial) || self.queued.iter().any(|(key, _)| key == serial)
    }

    pub fn schedule(
        &mut self,
        serial: String,
        work: impl Future<Output = T> + Send + 'static,
    ) -> bool {
        if self.contains(&serial) {
            return false;
        }
        self.queued.push_back((serial, Box::pin(work)));
        self.start_ready();
        true
    }

    fn start_ready(&mut self) {
        while self.active.len() < self.limit {
            let Some((serial, work)) = self.queued.pop_front() else {
                break;
            };
            self.active.insert(serial, tokio::spawn(work));
        }
    }

    /// Poll every owned handle; return the first completion, never a batch
    /// barrier. Polling registers the actual caller's waker, without a racy
    /// notification sent before Tokio publishes the task's finished state.
    pub async fn next(&mut self) -> (String, Result<T, JoinError>) {
        let result = std::future::poll_fn(|context| {
            for (serial, task) in &mut self.active {
                if let Poll::Ready(result) = Pin::new(task).poll(context) {
                    return Poll::Ready((serial.clone(), result));
                }
            }
            Poll::Pending
        })
        .await;
        self.active.remove(&result.0);
        self.start_ready();
        result
    }

    /// Transfer cancellation ownership to a route-retirement barrier.
    pub fn retire(&mut self, serial: &str) -> Option<JoinHandle<T>> {
        self.queued.retain(|(key, _)| key != serial);
        let task = self.active.remove(serial);
        if let Some(task) = &task {
            task.abort();
        }
        self.start_ready();
        task
    }

    pub fn cancel(&mut self, serial: &str) {
        self.queued.retain(|(key, _)| key != serial);
        if let Some(task) = self.active.get(serial) {
            task.abort();
        }
    }

    pub async fn stop(&mut self) {
        self.queued.clear();
        for task in self.active.values() {
            task.abort();
        }
        for (_, task) in self.active.drain() {
            let _ = task.await;
        }
    }
}

impl<T> Drop for DeviceTasks<T> {
    fn drop(&mut self) {
        for task in self.active.values() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[tokio::test]
    async fn t497_dropping_scheduler_cancels_started_work_and_never_starts_the_queue() {
        let (ready, started) = tokio::sync::oneshot::channel();
        let (retired, completed) = tokio::sync::oneshot::channel::<()>();
        let mut tasks = DeviceTasks::new(1);
        tasks.schedule("active".into(), async move {
            let _retirement = retired;
            ready.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        tasks.schedule("queued".into(), async {
            panic!("T497 queued work escaped scheduler")
        });
        started.await.unwrap();
        drop(tasks);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), completed)
                .await
                .unwrap()
                .is_err()
        );
    }
}
