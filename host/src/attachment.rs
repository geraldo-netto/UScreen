//! Producer-owned attachment epochs. Never infer a replacement from a coalescing
//! presence watch; invalidate metadata synchronously before forwarding/launch.
use crate::media::EncoderSettings;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

struct State {
    identity: Option<String>,
    generation: u64,
}
struct Shared {
    state: Mutex<State>,
    presence: watch::Sender<bool>,
    generation: watch::Sender<u64>,
    settings: watch::Sender<EncoderSettings>,
}
#[derive(Clone)]
pub(crate) struct Attachment(Arc<Shared>);
impl Attachment {
    pub fn new(settings: watch::Sender<EncoderSettings>) -> Self {
        Self(Arc::new(Shared {
            state: Mutex::new(State {
                identity: None,
                generation: 0,
            }),
            presence: watch::channel(false).0,
            generation: watch::channel(0).0,
            settings,
        }))
    }

    /// Begin a transport handoff before its first authenticated message can
    /// arrive. Proven same-tablet migration retains geometry; replacement or
    /// absence invalidates it. Every handoff retires earlier control leases.
    pub fn begin(&self, identity: Option<String>) {
        let mut state = self.0.state.lock().unwrap();
        let preserve = identity.is_some() && state.identity == identity;
        state.identity = identity;
        state.generation = state.generation.wrapping_add(1);
        if !preserve {
            self.0.settings.send_if_modified(|settings| {
                let ready = settings.geometry_ready;
                settings.geometry_ready = false;
                ready
            });
        }
        self.0.generation.send_replace(state.generation);
        self.0.presence.send_replace(false);
    }

    /// Publish completed forwarding, or fully detach. Keep the familiar watch
    /// sender result so callers can ignore shutdown receivers disappearing.
    pub fn send(&self, present: bool) -> Result<(), watch::error::SendError<bool>> {
        if !present {
            self.begin(None);
        }
        self.0.presence.send(present)
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.0.presence.subscribe()
    }

    /// Capture at accept time, before WebSocket/authentication can await.
    pub fn lease(&self) -> Lease {
        let state = self.0.state.lock().unwrap();
        Lease {
            attachment: self.clone(),
            generation: state.generation,
            changed: self.0.generation.subscribe(),
        }
    }
}

pub(crate) struct Lease {
    attachment: Attachment,
    generation: u64,
    changed: watch::Receiver<u64>,
}
impl Lease {
    /// Serialize check and action with invalidation. An old socket cannot
    /// restore geometry or inject contacts after a new attachment begins.
    pub fn apply(&self, action: impl FnOnce()) -> bool {
        let state = self.attachment.0.state.lock().unwrap();
        if state.generation != self.generation {
            return false;
        }
        action();
        true
    }
    pub fn retirement(&self) -> Retirement {
        Retirement {
            changed: self.changed.clone(),
            generation: self.generation,
        }
    }
}
pub(crate) struct Retirement {
    changed: watch::Receiver<u64>,
    generation: u64,
}
impl Retirement {
    pub async fn retired(&mut self) {
        if *self.changed.borrow() == self.generation {
            let _ = self.changed.changed().await;
        }
    }
}
