//! T673: negotiated non-stylus sessions. Native resources live on one owned worker.
use super::{
    backend::{InputBackend, InputSink, PenSample},
    InputConfig,
};
use anyhow::{ensure, Context, Result};
use blent_config::{
    direct_input::{Adapter, Config, Event, Mode, Phase, Session},
    input_mapping::Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{mpsc, Arc, Mutex},
    thread,
};
use tokio::sync::watch;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Negotiate { version: u32 },
    Select { mode: Mode },
    Event { event: Event },
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub protocol: u32,
    pub monitor: String,
    pub mode: Mode,
    pub touch: bool,
    pub mouse: bool,
    pub negotiated: bool,
}

pub trait Environment: 'static {
    type Device: Adapter;
    fn snapshot(&self) -> Result<Snapshot>;
    fn create(&self, mode: Mode) -> Result<Self::Device>;
    fn save(&self, config: &Config) -> Result<()>;
}

struct State<E: Environment> {
    environment: E,
    status: Status,
    session: Option<Session<E::Device>>,
}
impl<E: Environment> State<E> {
    fn retire(&mut self) -> Result<()> {
        self.status.negotiated = false;
        if let Some(session) = &mut self.session {
            session.retire()?;
        }
        self.session = None;
        Ok(())
    }
    fn start(&mut self, mode: Mode) -> Result<()> {
        self.retire()?;
        ensure!(
            match mode {
                Mode::Touch => self.status.touch,
                Mode::DirectMouse => self.status.mouse,
            },
            "Input mode disabled"
        );
        let config = Config {
            monitor: self.status.monitor.clone(),
            mode,
        };
        let snapshot = self.environment.snapshot()?;
        snapshot.select(&config.monitor)?;
        let session = Session::new(self.environment.create(mode)?, &config, &snapshot)?;
        self.environment.save(&config)?;
        self.session = Some(session);
        self.status.mode = mode;
        self.status.negotiated = true;
        Ok(())
    }
    fn command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Negotiate { version } => {
                ensure!(version == 1, "Unsupported direct input protocol");
                self.start(self.status.mode)
            }
            Command::Select { mode } => {
                ensure!(self.status.negotiated, "Direct input must negotiate first");
                self.start(mode)
            }
            Command::Event { event } => {
                let snapshot = self.environment.snapshot()?;
                self.session
                    .as_mut()
                    .context("Direct input must negotiate first")?
                    .apply(&snapshot, event)
            }
        }
    }
}

enum Request {
    Command(Command),
    Retire,
}
type Message = (Request, mpsc::SyncSender<Result<Status>>);
pub struct Backend {
    sink: Arc<Sink>,
}
struct Sink {
    worker: Mutex<Worker>,
    status: Mutex<Status>,
}
struct Worker {
    sender: Option<mpsc::SyncSender<Message>>,
    join: Option<thread::JoinHandle<()>>,
}

impl Backend {
    pub fn new<E: Environment>(
        config: Config,
        touch: bool,
        mouse: bool,
        factory: impl FnOnce() -> E + Send + 'static,
    ) -> Result<Self> {
        let status = Status {
            protocol: 1,
            monitor: config.monitor,
            mode: config.mode,
            touch,
            mouse,
            negotiated: false,
        };
        let (sender, receiver) = mpsc::sync_channel(16);
        let initial = status.clone();
        let join = thread::Builder::new()
            .name("blent-direct-input".into())
            .spawn(move || {
                serve(
                    State {
                        environment: factory(),
                        status: initial,
                        session: None,
                    },
                    receiver,
                );
            })?;
        Ok(Self {
            sink: Arc::new(Sink {
                worker: Mutex::new(Worker {
                    sender: Some(sender),
                    join: Some(join),
                }),
                status: Mutex::new(status),
            }),
        })
    }
}

fn serve<E: Environment>(mut state: State<E>, receiver: mpsc::Receiver<Message>) {
    for (request, reply) in receiver {
        let result = match request {
            Request::Command(command) => state.command(command),
            Request::Retire => state.retire(),
        };
        if result.is_err() {
            let _ = state.retire();
        }
        if reply.send(result.map(|()| state.status.clone())).is_err() {
            let _ = state.retire();
        }
    }
    let _ = state.retire();
}
impl Sink {
    fn request(&self, request: Request) -> Result<()> {
        let worker = self.worker.lock().unwrap();
        let (reply, response) = mpsc::sync_channel(1);
        worker
            .sender
            .as_ref()
            .context("Input worker stopped")?
            .send((request, reply))
            .map_err(|_| anyhow::anyhow!("Input worker stopped"))?;
        let result = response.recv().context("Input worker stopped")?;
        match result {
            Ok(status) => {
                *self.status.lock().unwrap() = status;
                Ok(())
            }
            Err(error) => {
                self.status.lock().unwrap().negotiated = false;
                Err(error)
            }
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl InputSink for Sink {
    fn release_all(&self) {
        let _ = self.request(Request::Retire);
    }
    fn touch(&self, (x, y, _): (f64, f64, f64), action: u8, slot: u8) {
        if let Err(error) =
            legacy_touch(x, y, action, slot).and_then(|command| self.direct(command))
        {
            self.release_all();
            tracing::warn!("Touch rejected: {error}");
        }
    }
    fn pen(&self, _: PenSample, _: bool) {
        self.release_all();
    }
    fn direct(&self, command: Command) -> Result<()> {
        self.request(Request::Command(command))
    }
    fn direct_status(&self) -> Option<Status> {
        Some(self.status.lock().unwrap().clone())
    }
}

pub(super) fn legacy_touch(x: f64, y: f64, action: u8, slot: u8) -> Result<Command> {
    let phase = match action {
        0 => Phase::Down,
        1 => Phase::Up,
        2 => Phase::Move,
        _ => anyhow::bail!("Invalid touch action"),
    };
    Ok(Command::Event {
        event: Event::Touch { x, y, slot, phase },
    })
}
impl InputBackend for Backend {
    fn sink(&self) -> Arc<dyn InputSink> {
        self.sink.clone()
    }
    fn follow(
        &self,
        mut tablet: watch::Receiver<bool>,
        mut mode: watch::Receiver<bool>,
        _: InputConfig,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let sink = self.sink.clone();
        Box::pin(async move {
            loop {
                let changed = tokio::select! {
                    result = tablet.changed() => { if result.is_ok() && *tablet.borrow_and_update() { continue; } result },
                    result = mode.changed() => result
                };
                sink.release_all();
                if changed.is_err() {
                    break;
                }
            }
        })
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::native;
#[cfg(test)]
mod tests;
