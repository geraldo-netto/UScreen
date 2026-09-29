//! Manual audio ownership: no saved setting restores a capture session.
use anyhow::Result;
use blent_config::audio::{AudioController, AudioOptions, AudioState, AudioStatus};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use tokio::sync::watch;

#[derive(Clone)]
pub struct Report(pub watch::Sender<AudioStatus>);
impl Report {
    pub fn update(&self, state: AudioState, detail: impl Into<String>) {
        self.0.send_replace(AudioStatus {
            state,
            detail: detail.into(),
        });
    }
}

pub struct Controller {
    commands: Option<watch::Sender<Option<AudioOptions>>>,
    status: watch::Receiver<AudioStatus>,
    worker: Option<JoinHandle<()>>,
    admitted: Arc<Mutex<bool>>,
}
impl Controller {
    pub fn new<F, Fut>(run: F) -> Result<Self>
    where
        F: Fn(AudioOptions, watch::Receiver<bool>, Report) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>>,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (commands, receiver) = watch::channel(None);
        let (sender, status) = watch::channel(AudioStatus::default());
        let admitted = Arc::new(Mutex::new(false));
        let worker_admission = admitted.clone();
        let worker = std::thread::Builder::new()
            .name("blent-audio-control".into())
            .spawn(move || {
                runtime.block_on(supervise(receiver, Report(sender), run, worker_admission));
            })?;
        Ok(Self {
            commands: Some(commands),
            status,
            worker: Some(worker),
            admitted,
        })
    }
}
impl AudioController for Controller {
    fn start(&self, options: AudioOptions) -> Result<()> {
        options.profile.validate()?;
        let mut admitted = self.admitted.lock().unwrap();
        anyhow::ensure!(!*admitted, "Stop audio before changing an active session");
        self.commands.as_ref().unwrap().send(Some(options))?;
        *admitted = true;
        Ok(())
    }
    fn stop(&self) {
        let _ = self.commands.as_ref().unwrap().send(None);
    }
    fn status(&self) -> AudioStatus {
        self.status.borrow().clone()
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        drop(self.commands.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn supervise<F, Fut>(
    mut commands: watch::Receiver<Option<AudioOptions>>,
    report: Report,
    run: F,
    admitted: Arc<Mutex<bool>>,
) where
    F: Fn(AudioOptions, watch::Receiver<bool>, Report) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    loop {
        let options = commands.borrow_and_update().clone();
        if let Some(options) = options {
            if !active(options, &mut commands, &report, &run, &admitted).await {
                break;
            }
        } else {
            {
                let mut reservation = admitted.lock().unwrap();
                // Start may have arrived after the earlier empty snapshot.
                if commands.borrow().is_none() {
                    report.update(AudioState::Stopped, "Stopped");
                    *reservation = false;
                }
            }
            if commands.changed().await.is_err() {
                break;
            }
        }
    }
}

async fn active<F, Fut>(
    options: AudioOptions,
    commands: &mut watch::Receiver<Option<AudioOptions>>,
    report: &Report,
    run: &F,
    admitted: &Mutex<bool>,
) -> bool
where
    F: Fn(AudioOptions, watch::Receiver<bool>, Report) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let (stop, stopped) = watch::channel(false);
    let detail = match options.profile.direction {
        blent_config::audio::Direction::Microphone => {
            "Open Blent on the tablet and allow microphone access."
        }
        blent_config::audio::Direction::Speakers => {
            "Open Blent on the tablet for speaker playback."
        }
    };
    report.update(AudioState::Starting, detail);
    let work = run(options, stopped, report.clone());
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => {
            {
                let mut reservation = admitted.lock().unwrap();
                match result {
                    Ok(()) => report.update(AudioState::Stopped, "Stopped"),
                    Err(error) => report.update(AudioState::Failed, format!("{error:#}")),
                }
                *reservation = false;
            }
            commands.changed().await.is_ok()
        }
        changed = commands.changed() => {
            report.update(AudioState::Stopping, "Stopping audio…");
            stop.send_replace(true);
            let _ = work.await;
            changed.is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blent_config::audio::Direction;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };

    #[test]
    fn t718_pending_start_reserves_admission_before_worker_status() {
        let (sender, receiver) = watch::channel(None);
        let (_status, status) = watch::channel(AudioStatus::default());
        let controller = Controller {
            commands: Some(sender),
            status,
            worker: None,
            admitted: Arc::new(Mutex::new(false)),
        };
        let options = AudioOptions::new(Direction::Microphone);
        controller.start(options.clone()).unwrap();
        assert!(receiver.borrow().is_some());
        assert!(
            controller.start(options.clone()).is_err(),
            "T718 second Start was admitted before worker published Starting"
        );
        controller.stop();
        assert!(
            controller.start(options).is_err(),
            "T718 Start was admitted before Stop retirement was acknowledged"
        );
    }
    fn wait(controller: &Controller, state: AudioState) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while controller.status().state != state {
            assert!(Instant::now() < deadline, "{:?}", controller.status());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn t718_explicit_start_stop_failure_and_drop_retire() {
        let retired = Arc::new(AtomicUsize::new(0));
        let count = retired.clone();
        let controller = Controller::new(move |options, mut stop, report| {
            let count = count.clone();
            async move {
                if options.serial.as_deref() == Some("fail") {
                    anyhow::bail!("unavailable");
                }
                report.update(AudioState::Streaming, "ready");
                crate::camera_control::cancelled(&mut stop).await;
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap();
        let options = AudioOptions::new(Direction::Microphone);
        let mut bad = options.clone();
        bad.profile.buffer_ms = 1;
        assert!(controller.start(bad).is_err());
        assert_eq!(controller.status().state, AudioState::Stopped);
        controller.start(options.clone()).unwrap();
        wait(&controller, AudioState::Streaming);
        assert!(controller.start(options.clone()).is_err());
        controller.stop();
        wait(&controller, AudioState::Stopped);
        assert_eq!(retired.load(Ordering::SeqCst), 1);
        let mut failure = options.clone();
        failure.serial = Some("fail".into());
        controller.start(failure).unwrap();
        wait(&controller, AudioState::Failed);
        assert!(controller.status().detail.contains("unavailable"));
        controller.start(options).unwrap();
        wait(&controller, AudioState::Streaming);
        drop(controller);
        assert_eq!(retired.load(Ordering::SeqCst), 2);
        drop(Controller::new(|_, _, _| async { Ok(()) }).unwrap());
    }
}
