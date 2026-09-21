//! Producer-owned attachment epochs. Never infer a replacement from a coalescing
//! presence watch; invalidate metadata synchronously before forwarding/launch.
use crate::media::EncoderSettings;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

mod auth;
use auth::Authentication;

struct State {
    authentication: Authentication,
    identity: Option<String>,
    generation: u64,
    transport: Option<uscreen_config::adb::Transport>,
}
struct Shared {
    state: Mutex<State>,
    presence: watch::Sender<bool>,
    generation: watch::Sender<u64>,
    settings: watch::Sender<EncoderSettings>,
}
#[derive(Clone)]
pub struct Attachment(Arc<Shared>);
impl Attachment {
    pub fn new(settings: watch::Sender<EncoderSettings>) -> Self {
        Self(Arc::new(Shared {
            state: Mutex::new(State {
                authentication: Authentication::Unmanaged,
                identity: None,
                generation: 0,
                transport: None,
            }),
            presence: watch::channel(false).0,
            generation: watch::channel(0).0,
            settings,
        }))
    }

    pub fn with_token(settings: watch::Sender<EncoderSettings>, token: Option<String>) -> Self {
        let attachment = Self::new(settings);
        attachment.0.state.lock().unwrap().authentication = Authentication::configured(token);
        attachment
    }

    pub fn token(&self) -> anyhow::Result<Option<String>> {
        self.0
            .state
            .lock()
            .unwrap()
            .authentication
            .expected(None)
            .map(|token| token.map(str::to_owned))
    }

    /// Begin a transport handoff before its first authenticated message can
    /// arrive. Proven same-tablet migration retains geometry; replacement or
    /// absence invalidates it. Every handoff retires earlier control leases.
    pub fn begin(&self, identity: Option<String>) {
        self.begin_with_transport(identity, None);
    }

    pub fn begin_with_transport(
        &self,
        identity: Option<String>,
        transport: Option<uscreen_config::adb::Transport>,
    ) {
        let mut state = self.0.state.lock().unwrap();
        let preserve = identity.is_some() && state.identity == identity;
        state
            .authentication
            .rotate(preserve, uscreen_config::credentials::random_token);
        state.transport = transport.filter(|_| identity.is_some());
        state.identity = identity;
        state.generation = state.generation.wrapping_add(1);
        if !preserve {
            self.0.settings.send_if_modified(|settings| {
                let ready = settings.geometry_ready
                    || settings.decoders.is_some()
                    || settings.selection.is_some();
                settings.clear_decoders();
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
            authentication: state.authentication.clone(),
            identity: state.identity.clone(),
            attachment: self.clone(),
            generation: state.generation,
            transport: state.transport,
            changed: self.0.generation.subscribe(),
        }
    }
}

pub struct Lease {
    #[cfg_attr(feature = "inproc-encoder", allow(dead_code))]
    identity: Option<String>,
    authentication: Authentication,
    attachment: Attachment,
    generation: u64,
    transport: Option<uscreen_config::adb::Transport>,
    changed: watch::Receiver<u64>,
}
impl Lease {
    /// Identity and route are captured atomically with the attachment generation.
    #[cfg(not(feature = "inproc-encoder"))]
    pub fn profile_identity(&self) -> Option<(String, &'static str)> {
        let identity = self.identity.as_ref()?.strip_prefix("device:")?;
        if identity.is_empty() {
            return None;
        }
        Some((identity.to_string(), self.transport()?))
    }

    pub fn token<'a>(&'a self, fallback: Option<&'a str>) -> anyhow::Result<Option<&'a str>> {
        self.authentication.expected(fallback)
    }

    /// Poll video I/O under the generation check. Retirement cannot race a
    /// synchronous socket write; a pending poll never holds the mutex asleep.
    pub async fn run(
        &self,
        work: impl std::future::Future<Output = anyhow::Result<()>>,
    ) -> anyhow::Result<()> {
        let mut work = std::pin::pin!(work);
        let checked = std::future::poll_fn(|cx| {
            let mut result = std::task::Poll::Ready(Ok(()));
            self.apply(|| result = work.as_mut().poll(cx));
            result
        });
        let mut retirement = self.retirement();
        tokio::select! {
            biased;
            _ = retirement.retired() => Ok(()),
            result = checked => result,
        }
    }

    /// Immutable accepted route: never read a replacement tablet's transport.
    pub fn transport(&self) -> Option<&'static str> {
        self.transport.map(|route| match route {
            uscreen_config::adb::Transport::Usb => "usb",
            uscreen_config::adb::Transport::Network => "network",
        })
    }

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
pub struct Retirement {
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
