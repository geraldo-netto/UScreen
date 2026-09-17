//! One daemon-owned filesystem worker; a bounded queue admits at most one
//! waiting transaction. Settings/mode watch channels coalesce later updates.
use anyhow::{Context, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{mpsc, oneshot};
use uscreen_config::{model::FileConfig, storage::ConfigStore};

type Edit = Box<dyn FnOnce(&mut FileConfig) -> Result<()> + Send>;
enum Message {
    Update(Edit, oneshot::Sender<Result<FileConfig>>),
    #[cfg(test)]
    Barrier(oneshot::Sender<()>),
    Stop,
}

#[derive(Clone)]
pub struct Writer(mpsc::Sender<Message>);

impl Writer {
    pub async fn update(
        &self,
        edit: impl FnOnce(&mut FileConfig) -> Result<()> + Send + 'static,
    ) -> Result<FileConfig> {
        let (reply, result) = oneshot::channel();
        self.0
            .send(Message::Update(Box::new(edit), reply))
            .await
            .map_err(|_| anyhow::anyhow!("config worker stopped"))?;
        result.await.context("config worker completion")?
    }

    #[cfg(test)]
    pub async fn barrier(&self) {
        let (sender, receiver) = oneshot::channel();
        self.0.send(Message::Barrier(sender)).await.unwrap();
        receiver.await.unwrap();
    }
}

pub struct Worker {
    writer: Writer,
    cancelled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn new(store: ConfigStore) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let thread = std::thread::Builder::new()
            .name("uscreen-config".into())
            .spawn(move || run(store, stop, receiver))?;
        Ok(Self {
            writer: Writer(sender),
            cancelled,
            thread: Some(thread),
        })
    }

    pub fn writer(&self) -> Writer {
        self.writer.clone()
    }

    pub async fn shutdown(mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.writer.0.send(Message::Stop).await;
        // Cancel lock acquisition, then join the owned thread. Do not pretend
        // cancelling an async waiter stops an already committing transaction.
        let thread = self.thread.take().unwrap();
        let _ = tokio::task::spawn_blocking(move || thread.join()).await;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Startup failure/early future drop still wakes an idle or lock-waiting
        // worker. Normal daemon shutdown additionally joins it above.
        self.cancelled.store(true, Ordering::Release);
        let _ = self.writer.0.try_send(Message::Stop);
    }
}

fn run(store: ConfigStore, cancelled: Arc<AtomicBool>, mut queue: mpsc::Receiver<Message>) {
    while let Some(message) = queue.blocking_recv() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        match message {
            Message::Update(edit, reply) => {
                if !reply.is_closed() {
                    let _ = reply.send(store.update_cancellable(&cancelled, edit));
                }
            }
            #[cfg(test)]
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
            Message::Stop => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread")]
    async fn t385_shutdown_cancels_lock_wait_and_queued_work_then_joins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = ConfigStore::new(path.clone());
        let original = store.update(|_| Ok(())).unwrap();
        let lock = std::fs::File::open(path.with_extension("lock")).unwrap();
        lock.lock().unwrap();
        let worker = Worker::new(store.clone()).unwrap();
        let writer = worker.writer();
        let first = writer.update(|cfg| {
            cfg.fps = 30;
            Ok(())
        });
        tokio::pin!(first);
        assert!(futures_util::poll!(&mut first).is_pending());
        // The single queue slot becomes available only when the worker has
        // accepted the first transaction, which cannot acquire the held lock.
        let slot = writer.0.reserve().await.unwrap();
        drop(slot);
        let second = writer.update(|cfg| {
            cfg.pen_only = true;
            Ok(())
        });
        tokio::pin!(second);
        assert!(futures_util::poll!(&mut second).is_pending());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), writer.0.reserve())
                .await
                .is_err(),
            "T385: queue admitted more than one waiting transaction"
        );
        tokio::time::timeout(Duration::from_secs(2), worker.shutdown())
            .await
            .unwrap();
        assert!(first.await.is_err());
        assert!(second.await.is_err());
        assert_eq!(
            store.load(),
            original,
            "T385: cancelled writes changed config"
        );
        drop(lock);
    }

    #[tokio::test]
    async fn t385_settings_and_mode_transactions_share_one_ordered_worker() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let worker = Worker::new(store.clone()).unwrap();
        let first = worker.writer();
        let second = worker.writer();
        let (a, b) = tokio::join!(
            first.update(|cfg| {
                cfg.fps = 30;
                Ok(())
            }),
            second.update(|cfg| {
                cfg.pen_only = true;
                Ok(())
            }),
        );
        assert!(a.is_ok());
        assert!(b.is_ok());
        let saved = store.load();
        assert_eq!(saved.fps, 30);
        assert!(saved.pen_only);
        worker.shutdown().await;
    }
}
