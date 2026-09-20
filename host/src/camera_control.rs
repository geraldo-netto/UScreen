//! T543 manual host ownership. Preferences never start capture by themselves.
use anyhow::Result;
use std::{future::Future, sync::Arc, thread::JoinHandle};
use tokio::sync::watch;
use uscreen_config::camera::{CameraBackend, CameraPreview, CameraProfile, CameraState as State};

#[derive(Clone)]
pub struct Report {
    pub state: watch::Sender<State>,
    pub preview: watch::Sender<Option<Arc<CameraPreview>>>,
}
impl Report {
    pub fn update(&self, state: State) {
        if state != State::Streaming {
            self.preview.send_replace(None);
        }
        self.state.send_replace(state);
    }
}

pub struct Controller {
    command: Option<watch::Sender<Option<CameraProfile>>>,
    state: watch::Receiver<State>,
    preview: watch::Receiver<Option<Arc<CameraPreview>>>,
    worker: Option<JoinHandle<()>>,
}

impl Controller {
    pub fn new<F, Fut>(session: F) -> Result<Self>
    where
        F: Fn(CameraProfile, watch::Receiver<bool>, Report) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>>,
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (command, commands) = watch::channel(None);
        let (status, state) = watch::channel(State::Stopped);
        let (images, preview) = watch::channel(None);
        let report = Report {
            state: status,
            preview: images,
        };
        let worker = std::thread::Builder::new()
            .name("uscreen-camera".into())
            .spawn(move || {
                runtime.block_on(supervise(commands, report, session));
            })?;
        Ok(Self {
            command: Some(command),
            state,
            preview,
            worker: Some(worker),
        })
    }
}

impl CameraBackend for Controller {
    fn start(&self, options: CameraProfile) -> Result<()> {
        options.validate()?;
        self.command.as_ref().unwrap().send(Some(options))?;
        Ok(())
    }

    fn stop(&self) {
        let _ = self.command.as_ref().unwrap().send(None);
    }

    fn state(&self) -> State {
        self.state.borrow().clone()
    }
    fn preview(&self) -> Option<Arc<CameraPreview>> {
        self.preview.borrow().clone()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        drop(self.command.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn supervise<F, Fut>(
    mut commands: watch::Receiver<Option<CameraProfile>>,
    status: Report,
    session: F,
) where
    F: Fn(CameraProfile, watch::Receiver<bool>, Report) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    loop {
        let options = commands.borrow_and_update().clone();
        if let Some(options) = options {
            if !manage_session(options, &mut commands, &status, &session).await {
                break;
            }
        } else {
            status.update(State::Stopped);
            if commands.changed().await.is_err() {
                break;
            }
        }
    }
    status.update(State::Stopped);
}

async fn manage_session<F, Fut>(
    options: CameraProfile,
    commands: &mut watch::Receiver<Option<CameraProfile>>,
    status: &Report,
    session: &F,
) -> bool
where
    F: Fn(CameraProfile, watch::Receiver<bool>, Report) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let (stop, stopped) = watch::channel(false);
    status.update(State::Starting);
    let work = session(options, stopped, status.clone());
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => {
            status.update(match result {
                Ok(()) => State::Stopped,
                Err(error) => State::Failed(format!("{error:#}")),
            });
            // Failure never restarts a lens without another desktop action.
            commands.changed().await.is_ok()
        }
        changed = commands.changed() => {
            status.update(State::Stopping);
            stop.send_replace(true);
            let _ = work.await; // owns final black frames, child retirement and ADB cleanup
            changed.is_ok()
        }
    }
}

/// Native adapters use this bounded ownership signal to begin graceful cleanup.
pub async fn cancelled(stop: &mut watch::Receiver<bool>) {
    if !*stop.borrow_and_update() {
        let _ = stop.changed().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[tokio::test]
    async fn t543_manual_start_switch_stop_and_owner_exit_retire_in_order() {
        let (command, commands) = watch::channel(None);
        let (status, mut state) = watch::channel(State::Stopped);
        let status = Report {
            state: status,
            preview: watch::channel(None).0,
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let recording = events.clone();
        let owner = tokio::spawn(supervise(
            commands,
            status,
            move |profile, mut stop, status| {
                let events = recording.clone();
                async move {
                    events
                        .lock()
                        .unwrap()
                        .push(format!("start {:?}", profile.lens));
                    status.update(State::Streaming);
                    cancelled(&mut stop).await;
                    events
                        .lock()
                        .unwrap()
                        .push(format!("stop {:?}", profile.lens));
                    Ok(())
                }
            },
        ));
        tokio::task::yield_now().await;
        assert!(
            events.lock().unwrap().is_empty(),
            "T543 preferences must not start capture"
        );
        command.send(Some(CameraProfile::default())).unwrap();
        state.wait_for(|s| *s == State::Streaming).await.unwrap();
        command
            .send(Some(CameraProfile {
                lens: uscreen_config::camera::Lens::Rear,
                ..Default::default()
            }))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        command.send(None).unwrap();
        state.wait_for(|s| *s == State::Stopped).await.unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["start Front", "stop Front", "start Rear", "stop Rear"]
        );
        command.send(Some(CameraProfile::default())).unwrap();
        state.wait_for(|s| *s == State::Streaming).await.unwrap();
        drop(command);
        owner.await.unwrap();
        assert_eq!(events.lock().unwrap().last().unwrap(), "stop Front");
    }

    #[tokio::test]
    async fn t543_failed_session_waits_for_explicit_retry() {
        let (command, commands) = watch::channel(None);
        let (status, mut state) = watch::channel(State::Stopped);
        let status = Report {
            state: status,
            preview: watch::channel(None).0,
        };
        let owner = tokio::spawn(supervise(commands, status, |_, _, _| async {
            anyhow::bail!("missing webcam")
        }));
        command.send(Some(CameraProfile::default())).unwrap();
        state
            .wait_for(|s| matches!(s, State::Failed(_)))
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(*state.borrow(), State::Failed("missing webcam".into()));
        command.send(None).unwrap();
        state.wait_for(|s| *s == State::Stopped).await.unwrap();
        drop(command);
        owner.await.unwrap();
        let (_sender, mut already_stopped) = watch::channel(true);
        cancelled(&mut already_stopped).await;
    }

    #[test]
    fn t543_controller_owns_worker_and_validates_before_start() {
        let owner = Controller::new(|_, _, _| async { anyhow::bail!("missing webcam") }).unwrap();
        assert_eq!(owner.state(), State::Stopped);
        assert!(owner.preview().is_none());
        assert!(owner
            .start(CameraProfile {
                fps: 0,
                ..Default::default()
            })
            .is_err());
        owner
            .start(CameraProfile {
                ..Default::default()
            })
            .unwrap();
        for _ in 0..100 {
            if matches!(owner.state(), State::Failed(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(owner.state(), State::Failed(_)));
        owner.stop();
    }
}
