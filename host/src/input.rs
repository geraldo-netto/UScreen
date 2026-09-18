//! Control transport and controller ownership; platform work lives in adapters.
#[cfg(test)]
mod wake_tests;

mod backend;
#[cfg(test)]
mod batching_tests;
mod config;
#[cfg(test)]
mod contracts;
mod event_writer;
mod linux;
mod mapping;
mod settings;
mod wire;

use crate::media::EncoderSettings;
use anyhow::{Context, Result};
use backend::{InputBackend, InputSink, PenSample};
pub use config::InputConfig;
use futures_util::{SinkExt, StreamExt};
#[cfg(test)]
use linux::*;
#[cfg(test)]
use mapping::*;
#[cfg(test)]
pub(crate) use settings::negotiated_geometry;
use settings::*;
#[cfg(test)]
use std::os::fd::AsRawFd;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{accept_async_with_config, tungstenite::protocol::WebSocketConfig};
use tracing::{error, info, warn};
pub use wire::InputEvent;

struct Controllers {
    devices: Arc<dyn InputSink>,
    generation: watch::Sender<u64>,
}
impl Controllers {
    fn new(devices: Arc<dyn InputSink>) -> Self {
        Self {
            devices,
            generation: watch::channel(0).0,
        }
    }
    #[cfg(test)]
    fn claim(self: &Arc<Self>) -> ControllerLease {
        self.claim_with(|| {})
    }

    fn claim_with(self: &Arc<Self>, initialize: impl FnOnce()) -> ControllerLease {
        let mut id = 0;
        self.generation.send_modify(|generation| {
            initialize();
            self.devices.release_all();
            *generation = generation.wrapping_add(1);
            id = *generation;
        });
        ControllerLease {
            controllers: self.clone(),
            id,
            settings: None,
        }
    }
}
struct ControllerLease {
    controllers: Arc<Controllers>,
    id: u64,
    settings: Option<watch::Sender<EncoderSettings>>,
}
impl Drop for ControllerLease {
    fn drop(&mut self) {
        // Keep the generation read lock through release, so a new claim cannot
        // interleave after the ownership check and before device cleanup.
        let generation = self.controllers.generation.borrow();
        if *generation == self.id {
            self.controllers.devices.release_all();
            if let Some(settings) = &self.settings {
                settings.send_modify(EncoderSettings::clear_decoders);
            }
        }
    }
}

pub struct InputServer {
    attachment: Option<crate::attachment::Attachment>,
    backend: Arc<dyn InputBackend>,
    config: InputConfig,
    settings_tx: Option<watch::Sender<EncoderSettings>>,
    /// Which mode the daemon is in, and how the tablet changes it. Owned as a
    /// channel rather than a field because the mode is switchable at runtime
    /// and several parts of the daemon have to follow it.
    mode_tx: watch::Sender<bool>,
    latency: crate::latency::LatencyTracker,
    /// Woken when a client fails to authenticate, so the daemon can push the
    /// token to the app again over adb (a manually launched app never got
    /// one).
    relaunch: Arc<tokio::sync::Notify>,
    /// The EVDI card this tablet's helper opened, once known. The devices are
    /// mapped onto that card's connector, not onto "the first EVDI output" -
    /// with two tablets that would put both pens on one screen.
    card_rx: watch::Receiver<Option<u32>>,
    /// Whether this tablet is attached at all. The virtual input devices
    /// exist exactly while it is.
    tablet_rx: watch::Receiver<bool>,
}

impl InputServer {
    pub fn new(
        config: InputConfig,
        settings_tx: Option<watch::Sender<EncoderSettings>>,
        mode_tx: watch::Sender<bool>,
        latency: crate::latency::LatencyTracker,
        relaunch: Arc<tokio::sync::Notify>,
        card_rx: watch::Receiver<Option<u32>>,
        tablet_rx: watch::Receiver<bool>,
    ) -> Self {
        Self::with_backend(
            config,
            settings_tx,
            mode_tx,
            latency,
            relaunch,
            (card_rx, tablet_rx),
            Arc::new(linux::Backend::default()),
        )
    }

    fn with_backend(
        config: InputConfig,
        settings_tx: Option<watch::Sender<EncoderSettings>>,
        mode_tx: watch::Sender<bool>,
        latency: crate::latency::LatencyTracker,
        relaunch: Arc<tokio::sync::Notify>,
        presence: (watch::Receiver<Option<u32>>, watch::Receiver<bool>),
        backend: Arc<dyn InputBackend>,
    ) -> Self {
        let (card_rx, tablet_rx) = presence;
        Self {
            attachment: None,
            backend,
            config,
            settings_tx,
            mode_tx,
            latency,
            relaunch,
            card_rx,
            tablet_rx,
        }
    }

    pub(crate) fn with_attachment(mut self, attachment: crate::attachment::Attachment) -> Self {
        self.attachment = Some(attachment);
        self
    }

    pub async fn bind(&self) -> Result<TcpListener> {
        let addr = format!("127.0.0.1:{}", self.config.port);
        let listener = TcpListener::bind(&addr)
            .await
            .context(format!("Failed to bind input server to {}", addr))?;
        info!("Input server on ws://{}", addr);
        Ok(listener)
    }

    pub async fn run_with_listener(&self, listener: TcpListener) -> Result<()> {
        // The devices exist only while a tablet is attached. Created for the
        // daemon's whole lifetime they left a touchscreen and a pen tablet on
        // the desktop with nothing behind them, and merely having those
        // present changes desktop behaviour (Cinnamon and GNOME on X11 hide
        // the mouse cursor around touch devices). Recreating them on attach is
        // safe: DeviceIdentity is fixed per instance, names and product ids
        // alike, so the desktop's per-device settings and the output mapping
        // below find the same device every time.
        if !self.config.any_device() {
            info!(
                "Virtual input devices are all off (input_touch / input_pen / \
                 input_pointer in config.toml) — the tablet is display-only"
            );
        }
        let controllers = Arc::new(Controllers::new(self.backend.sink()));
        let mut tasks = tokio::task::JoinSet::new();
        let slots = Arc::new(tokio::sync::Semaphore::new(16));

        // Follow the tablet, the mode and the card for as long as the daemon
        // runs. Attach creates the devices and maps them; detach destroys
        // them; a mode or card switch moves them onto the other output and
        // drops anything held at that moment — a finger or pen tip that was
        // down would otherwise stay down on a screen no longer listening.
        tasks.spawn(self.backend.follow(
            self.tablet_rx.clone(),
            self.mode_tx.subscribe(),
            self.card_rx.clone(),
            self.config.clone(),
        ));

        let config = self.config.clone();

        loop {
            let accept = tokio::select! {
                res = listener.accept() => res,
                _ = tasks.join_next(), if !tasks.is_empty() => continue,
            };

            let (socket, peer) = match accept {
                Ok(s) => s,
                Err(e) => {
                    error!("Input accept failed: {}", e);
                    continue;
                }
            };

            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let mut incoming = PendingInput::new(socket);
            incoming.attachment = self
                .attachment
                .as_ref()
                .map(|attachment| attachment.lease());
            info!("Input client: {}", peer);
            let cfg = config.clone();
            let settings = self.settings_tx.clone();
            let mode_tx = self.mode_tx.clone();
            let latency = self.latency.clone();
            let devices = controllers.clone();
            let relaunch = self.relaunch.clone();
            tasks.spawn(async move {
                let _permit = permit;
                if let Err(e) =
                    handle_connection(incoming, cfg, settings, mode_tx, latency, devices, relaunch)
                        .await
                {
                    warn!("Input handler {}: {}", peer, e);
                }
            });
        }
    }
}

struct PendingInput {
    attachment: Option<crate::attachment::Lease>,
    stream: tokio::net::TcpStream,
    deadline: tokio::time::Instant,
}
impl PendingInput {
    fn new(stream: tokio::net::TcpStream) -> Self {
        Self {
            attachment: None,
            stream,
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(3),
        }
    }
}

async fn handle_connection(
    incoming: PendingInput,
    config: InputConfig,
    settings_tx: Option<watch::Sender<EncoderSettings>>,
    mode_tx: watch::Sender<bool>,
    latency: crate::latency::LatencyTracker,
    controllers: Arc<Controllers>,
    relaunch: Arc<tokio::sync::Notify>,
) -> Result<()> {
    // Input events are a few hundred bytes. The library default of 64 MiB
    // per message is a memory bill nobody on this socket should be able to
    // run up.
    let ws_cfg = WebSocketConfig {
        max_message_size: Some(64 * 1024),
        max_frame_size: Some(64 * 1024),
        ..Default::default()
    };
    let deadline = incoming.deadline;
    let ws_stream = tokio::time::timeout_at(
        deadline,
        accept_async_with_config(incoming.stream, Some(ws_cfg)),
    )
    .await
    .context("WebSocket handshake timed out")?
    .context("WebSocket handshake failed")?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    if !authenticate_input(
        &mut ws_sender,
        &mut ws_receiver,
        config.token.as_deref(),
        deadline,
        &relaunch,
    )
    .await
    {
        return Ok(());
    }

    let channels = (settings_tx, mode_tx);
    let attachment = incoming.attachment;
    let mut retirement = attachment
        .as_ref()
        .map(crate::attachment::Lease::retirement);
    let controller = serve_controller(
        ws_sender,
        ws_receiver,
        config,
        channels,
        attachment.as_ref(),
        latency,
        controllers,
    );
    match retirement.as_mut() {
        Some(lease) => {
            // The lease also wraps each dispatch below; cancellation alone
            // would leave a race between readiness and a final old message.
            tokio::select! {
                biased;
                _ = lease.retired() => Ok(()),
                result = controller => result,
            }
        }
        None => controller.await,
    }
}

async fn serve_controller(
    mut ws_sender: futures_util::stream::SplitSink<InputSocket, Message>,
    mut ws_receiver: futures_util::stream::SplitStream<InputSocket>,
    config: InputConfig,
    channels: (Option<watch::Sender<EncoderSettings>>, watch::Sender<bool>),
    attachment: Option<&crate::attachment::Lease>,
    latency: crate::latency::LatencyTracker,
    controllers: Arc<Controllers>,
) -> Result<()> {
    let (settings_tx, mode_tx) = channels;
    let settings = SessionSettings::new(&settings_tx, &mode_tx, config.pen);
    let mut mode_rx = mode_tx.subscribe();
    let mut ownership = controllers.generation.subscribe();
    let Some(lease) = claim_controller(&controllers, attachment, &settings) else {
        return Ok(());
    };
    if *ownership.borrow_and_update() != lease.id {
        return Ok(());
    }
    // Subscribe after the claim: its capability reset is already in the greeting.
    let mut settings_rx = settings_tx.as_ref().map(watch::Sender::subscribe);

    let dispatch = ControllerDispatch {
        controllers: &controllers,
        controller: lease.id,
        settings: &settings,
        latency: &latency,
        pen_enabled: config.pen,
        attachment,
    };

    let mut resp = config.response("connected", *mode_rx.borrow_and_update(), &settings_tx);
    resp.transport = attachment.and_then(crate::attachment::Lease::transport);

    if !send_controller_message(
        &mut ws_sender,
        Message::Text(serde_json::to_string(&resp)?),
        &mut ownership,
    )
    .await?
    {
        return Ok(());
    }

    loop {
        let msg = tokio::select! {
            biased;
            _ = ownership.changed() => break,
            incoming = ws_receiver.next() => match incoming {
                Some(m) => m,
                None => break,
            },
            changed = async {
                match settings_rx.as_mut() {
                    Some(rx) => rx.changed().await,
                    None => std::future::pending().await,
                }
            } => {
                if changed.is_err() { settings_rx = None; continue; }
                let response = config.response("mode", *mode_rx.borrow(), &settings_tx);
                if !send_controller_message(&mut ws_sender,
                    Message::Text(serde_json::to_string(&response)?), &mut ownership)
                    .await.unwrap_or(false) {
                    break;
                }
                continue;
            }
            // The mode changed — here, from the GUI, or from the command line.
            // Whoever changed it, the tablet has to hear about it: it decides
            // from this whether to expect a video stream at all.
            changed = mode_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let pen_only = *mode_rx.borrow();
                let resp = config.response("mode", pen_only, &settings_tx);
                if !send_controller_message(&mut ws_sender,
                    Message::Text(serde_json::to_string(&resp)?), &mut ownership)
                    .await.unwrap_or(false)
                {
                    break;
                }
                continue;
            }
        };

        if !handle_controller_message(msg, &dispatch, &settings, &mut ws_sender, &mut ownership)
            .await?
        {
            break;
        }
    }

    Ok(())
}

async fn handle_controller_message(
    message: std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
    dispatch: &ControllerDispatch<'_>,
    settings: &SessionSettings<'_>,
    sender: &mut futures_util::stream::SplitSink<InputSocket, Message>,
    ownership: &mut watch::Receiver<u64>,
) -> Result<bool> {
    match message {
        Ok(Message::Text(text)) => {
            if !dispatch.text(&text) {
                return Ok(false);
            }
            send_settings_rejection(settings, sender, ownership).await
        }
        Ok(Message::Close(_)) | Err(_) => Ok(false),
        Ok(Message::Ping(data)) => {
            Ok(
                send_controller_message(sender, Message::Pong(data), ownership)
                    .await
                    .unwrap_or(false),
            )
        }
        _ => Ok(true),
    }
}

async fn send_settings_rejection(
    settings: &SessionSettings<'_>,
    sender: &mut futures_util::stream::SplitSink<InputSocket, Message>,
    ownership: &mut watch::Receiver<u64>,
) -> Result<bool> {
    match settings.rejection_reply() {
        Some(reply) => send_controller_message(sender, Message::Text(reply), ownership).await,
        None => Ok(true),
    }
}

fn claim_controller(
    controllers: &Arc<Controllers>,
    attachment: Option<&crate::attachment::Lease>,
    settings: &SessionSettings<'_>,
) -> Option<ControllerLease> {
    let mut lease = None;
    let mut claim = || {
        let mut claimed = controllers.claim_with(|| settings.forget_decoders());
        claimed.settings = settings.sender();
        lease = Some(claimed);
    };
    match attachment {
        Some(attachment) => {
            attachment.apply(claim);
        }
        None => claim(),
    }
    lease
}

struct ControllerDispatch<'a> {
    controllers: &'a Controllers,
    controller: u64,
    settings: &'a dyn SettingsSink,
    latency: &'a crate::latency::LatencyTracker,
    pen_enabled: bool,
    attachment: Option<&'a crate::attachment::Lease>,
}
impl ControllerDispatch<'_> {
    fn text(&self, text: &str) -> bool {
        let mut accepted = false;
        let mut dispatch = || {
            accepted = dispatch_controller_text(
                text,
                self.controllers,
                self.controller,
                self.settings,
                self.latency,
                self.pen_enabled,
            )
        };
        match self.attachment {
            Some(lease) => {
                lease.apply(dispatch);
            }
            None => dispatch(),
        }
        accepted
    }
}

fn dispatch_controller_text(
    text: &str,
    controllers: &Controllers,
    lease: u64,
    settings: &dyn SettingsSink,
    latency: &crate::latency::LatencyTracker,
    pen_enabled: bool,
) -> bool {
    match serde_json::from_str::<InputEvent>(text) {
        Ok(event) => {
            let generation = controllers.generation.borrow();
            if *generation != lease {
                return false;
            }
            handle_event(event, &controllers.devices, settings, latency, pen_enabled);
        }
        Err(e) => warn!("Invalid input: {} - {}", e, text),
    }
    true
}

type InputSocket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn send_controller_message(
    sender: &mut futures_util::stream::SplitSink<InputSocket, Message>,
    message: Message,
    ownership: &mut watch::Receiver<u64>,
) -> Result<bool> {
    // A non-reading retired peer must not retain an admission slot merely
    // because its socket is full. Cancellation drops this connection too.
    tokio::select! {
        biased;
        _ = ownership.changed() => Ok(false),
        result = sender.send(message) => {
            result?;
            Ok(true)
        }
    }
}

async fn authenticate_input(
    ws_sender: &mut futures_util::stream::SplitSink<InputSocket, Message>,
    ws_receiver: &mut futures_util::stream::SplitStream<InputSocket>,
    expected: Option<&str>,
    deadline: tokio::time::Instant,
    relaunch: &tokio::sync::Notify,
) -> bool {
    // Authenticate before anything else happens: no greeting, no events.
    if let Some(expected) = expected {
        let first = tokio::time::timeout_at(deadline, ws_receiver.next()).await;
        let ok = match first {
            Ok(Some(Ok(Message::Text(text)))) => matches!(
                serde_json::from_str::<InputEvent>(&text),
                Ok(InputEvent::Auth { token }) if crate::runtime::token_matches(expected, &token)
            ),
            _ => false,
        };
        if !ok {
            // The app reconnects every two seconds; after a handful of these
            // the log has made its point.
            static DROPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = DROPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 5 {
                warn!(
                    "Input client did not authenticate — dropped. Re-sending the token to the app."
                );
            } else if n == 5 {
                warn!("Further unauthenticated clients will be dropped quietly.");
            }
            // Most likely the app was started by hand and never received a
            // token. Launching it again over adb delivers one.
            relaunch.notify_one();
            let _ = tokio::time::timeout_at(deadline, ws_sender.send(Message::Close(None))).await;
            return false;
        }
    }

    true
}

fn handle_event(
    event: InputEvent,
    uinput: &dyn InputSink,
    settings: &dyn SettingsSink,
    latency: &crate::latency::LatencyTracker,
    pen_enabled: bool,
) {
    match event {
        InputEvent::Touch {
            x,
            y,
            pressure,
            action,
            slot,
        } => {
            latency.note_interaction();
            uinput.touch((x, y, pressure), action, slot);
        }
        InputEvent::Pen {
            x,
            y,
            pressure,
            tilt_x,
            tilt_y,
            eraser,
            button,
            action,
        } => {
            latency.note_interaction();
            uinput.pen(
                PenSample {
                    position: (x, y, pressure),
                    tilt: (tilt_x, tilt_y),
                    eraser,
                    action,
                    button,
                },
                pen_enabled,
            );
        }
        InputEvent::Resolution {
            width,
            height,
            width_mm,
            height_mm,
        } => {
            settings.resolution((width, height), (width_mm, height_mm));
        }
        InputEvent::Decoders { capabilities } => settings.decoders(capabilities),
        InputEvent::Rendered {
            seq,
            decode_us,
            decoder,
        } => latency.on_rendered_from(seq, decode_us, decoder.as_deref()),
        InputEvent::Config {
            bitrate,
            fps,
            encoder,
        } => {
            settings.configure(bitrate, fps, encoder);
        }
        // Already consumed by handle_connection; a second one is harmless.
        InputEvent::Auth { .. } => {}
        InputEvent::Mode { pen_only } => settings.mode(pen_only),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn t299_primary_selection_reaches_kwin_mapping_arguments() {
        const NAME: &str = "input::tests::t299_primary_selection_reaches_kwin_mapping_arguments";
        if std::env::var_os("USCREEN_T299_CHILD").is_none() {
            run_t299_mapping_fixture(NAME);
            return;
        }
        let selected = super::primary_non_evdi_output().await.unwrap();
        assert!(
            super::map_kwin_device(
                "event-fixture",
                &super::DeviceIdentity::for_instance(0),
                &selected
            )
            .await
        );
        let trace =
            std::fs::read_to_string(std::env::var_os("USCREEN_T299_TRACE").unwrap()).unwrap();
        assert!(
            trace.contains("outputName s HDMI-A-1"),
            "T299: wrong mapping arguments: {trace}"
        );
    }

    fn run_t299_mapping_fixture(name: &str) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let scripts = [
            (
                "kscreen-doctor",
                r#"#!/bin/sh
printf '%s' '{"outputs":[{"name":"eDP-1","enabled":true,"priority":2},{"name":"HDMI-A-1","enabled":true,"priority":1}]}'
"#,
            ),
            (
                "busctl",
                r#"#!/bin/sh
printf '%s\n' "$*" >> "$USCREEN_T299_TRACE"
if [ "$2" = set-property ]; then printf '%s' "$8" > "$USCREEN_T299_VALUE"; exit 0; fi
case "$6" in
 available) printf 'b true\n';;
 name) printf 's "UScreen Pen"\n';;
 outputName) printf 's "%s"\n' "$(/bin/cat "$USCREEN_T299_VALUE")";;
 *) exit 43;;
esac
"#,
            ),
        ];
        for (program, source) in scripts {
            let path = dir.path().join(program);
            std::fs::write(&path, source).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env("USCREEN_T299_CHILD", "1")
            .env("PATH", dir.path())
            .env("USCREEN_T299_TRACE", dir.path().join("trace"))
            .env("USCREEN_T299_VALUE", dir.path().join("value"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn t299_primary_selection_accepts_current_and_legacy_schemas() {
        use serde_json::json;
        let secondary = json!({"name":"eDP-1","enabled":true,"priority":2});
        let primary = json!({"name":"HDMI-A-1","enabled":true,"priority":1});
        let cases = [
            (vec![secondary.clone(), primary], "HDMI-A-1"),
            (
                vec![
                    secondary.clone(),
                    json!({"name":"DP-1","enabled":true,"primary":true}),
                ],
                "DP-1",
            ),
            (
                vec![
                    json!({"name":"DVI-I-1","enabled":true,"priority":1}),
                    secondary.clone(),
                ],
                "eDP-1",
            ),
            (
                vec![
                    json!({"name":"DP-1","enabled":false,"priority":1}),
                    secondary.clone(),
                ],
                "eDP-1",
            ),
            (
                vec![json!({"name":"DP-1","enabled":true}), secondary.clone()],
                "DP-1",
            ),
        ];
        for (raw, expected) in cases {
            let outputs =
                crate::kscreen::parse(&serde_json::to_vec(&json!({"outputs":raw})).unwrap())
                    .unwrap();
            assert_eq!(
                super::primary_physical_output(&outputs, &["DVI-I-1".into()]).as_deref(),
                Some(expected),
                "T299"
            );
        }
        for priority in [
            json!(null),
            json!("1"),
            json!(-1),
            json!(1.5),
            json!(true),
            json!(0),
        ] {
            let raw =
                json!({"outputs":[secondary, {"name":"DP-1","enabled":true,"priority":priority}]});
            let outputs = crate::kscreen::parse(&serde_json::to_vec(&raw).unwrap()).unwrap();
            assert_eq!(
                super::primary_physical_output(&outputs, &[]).as_deref(),
                Some("eDP-1")
            );
        }
    }

    #[test]
    fn t372_mapping_selects_from_the_shared_inventory() {
        let outputs =
            crate::kscreen::parse(include_bytes!("../../testdata/kscreen-inventory.json")).unwrap();
        assert_eq!(
            super::primary_physical_output(&outputs, &["DVI-I-1".into()]),
            Some("DP-1".into())
        );
        assert_eq!(
            super::enabled_named_output(&outputs, Some("DVI-I-1"))
                .unwrap()
                .id,
            3
        );
        assert!(super::enabled_named_output(&outputs, Some("HDMI-1")).is_none());
        let malformed =
            crate::kscreen::parse(br#"{"outputs":[{}, {"name":"DP-1","enabled":true}]}"#).unwrap();
        assert!(
            super::primary_physical_output(&malformed, &[]).is_none(),
            "T372: preserve rejection of an absent connector name"
        );
    }

    #[test]
    fn t375_android_motion_fixture_matches_host_fields() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../testdata/input-motion.json")).unwrap();
        for case in cases {
            let event: super::InputEvent = serde_json::from_value(case["wire"].clone()).unwrap();
            assert_eq!(
                serde_json::to_value(event).unwrap(),
                case["wire"],
                "T375: {}",
                case["name"]
            );
        }
    }

    #[test]
    fn t247_connected_fixture_matches_production_response() {
        let config = super::InputConfig {
            virtual_width: 2960,
            virtual_height: 1848,
            ..Default::default()
        };
        let response = config.response("connected", true, &None);
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../testdata/control-connected.json")).unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), fixture, "T247");
    }

    #[test]
    fn t145_fallback_preserves_card_ownership() {
        let first = crate::vdisplay::EvdiConnector {
            card: 2,
            name: "DVI-I-1".into(),
            connected: true,
        };
        let second = crate::vdisplay::EvdiConnector {
            card: 3,
            name: "DVI-I-2".into(),
            connected: true,
        };
        let connectors = [first, second];
        assert_eq!(
            super::fallback_output(&connectors, Some(4)),
            None,
            "missing assigned card must not map another tablet"
        );
        assert_eq!(
            super::fallback_output(&connectors, None),
            None,
            "unknown card must not guess between tablets"
        );
        assert_eq!(
            super::fallback_output(&connectors, Some(3)).as_deref(),
            Some("DVI-I-2")
        );
        assert_eq!(
            super::fallback_output(&connectors[..1], None).as_deref(),
            Some("DVI-I-1")
        );
    }

    fn last_input_value(path: &std::path::Path, code: u16) -> i32 {
        let bytes = std::fs::read(path).unwrap();
        bytes
            .as_chunks::<{ std::mem::size_of::<LinuxInputEvent>() }>()
            .0
            .iter()
            .filter(|event| {
                u16::from_ne_bytes(event[18..20].try_into().unwrap()) == code
                    && u16::from_ne_bytes(event[16..18].try_into().unwrap())
                        == if code >= 0x100 { EV_KEY } else { EV_ABS }
            })
            .map(|event| i32::from_ne_bytes(event[20..24].try_into().unwrap()))
            .next_back()
            .unwrap()
    }

    fn input_frames(path: &std::path::Path) -> Vec<Vec<(u16, u16, i32)>> {
        let bytes = std::fs::read(path).unwrap();
        let mut frames = Vec::new();
        let mut frame = Vec::new();
        for event in bytes
            .as_chunks::<{ std::mem::size_of::<LinuxInputEvent>() }>()
            .0
        {
            let kind = u16::from_ne_bytes(event[16..18].try_into().unwrap());
            let code = u16::from_ne_bytes(event[18..20].try_into().unwrap());
            let value = i32::from_ne_bytes(event[20..24].try_into().unwrap());
            if kind == EV_SYN && code == SYN_REPORT {
                frames.push(std::mem::take(&mut frame));
            } else {
                frame.push((kind, code, value));
            }
        }
        assert!(frame.is_empty(), "input frame was not synchronized");
        frames
    }

    // libinput 1.26.2 evdev-tablet.c:adjust_tilt interprets resolved axes as
    // radians; unresolved axes are normalized to its historical +/-64 degrees.
    // https://gitlab.freedesktop.org/libinput/libinput/-/blob/1.26.2/src/evdev-tablet.c#L371
    // Both the Xorg libinput driver and Wayland consumers use this conversion.
    fn libinput_tilt_degrees(info: InputAbsInfo, value: i32) -> f64 {
        if info.resolution != 0 && info.minimum < 0 && info.maximum > 0 {
            (f64::from(value) / f64::from(info.resolution)).to_degrees()
        } else {
            let position = f64::from(value - info.minimum) / f64::from(info.maximum - info.minimum);
            (position.clamp(0.0, 1.0) * 2.0 - 1.0) * 64.0
        }
    }

    #[test]
    fn t287_wire_tilt_reconstructs_physical_degrees_from_declared_axes() {
        for eraser in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let devices = Arc::new(std::sync::Mutex::new(InjectDevices {
                pen: Some(UInputDevice::from_writer(file.reopen().unwrap())),
                ..InjectDevices::empty()
            }));
            let (mode, _rx) = watch::channel(false);
            let tracker = crate::latency::LatencyTracker::new();
            for degrees in [
                45.0_f64, -45.0, 0.0, 0.25, -0.25, 89.9, -89.9, 90.0, -90.0, 100.0, -100.0,
            ] {
                let wire = serde_json::json!({
                    "type": "pen", "x": 0.5, "y": 0.5, "pressure": 0.0,
                    "tilt_x": degrees, "tilt_y": -degrees, "eraser": eraser, "action": 3,
                });
                handle_event(
                    serde_json::from_value(wire).unwrap(),
                    &devices,
                    &SessionSettings::new(&None, &mode, true),
                    &tracker,
                    true,
                );
                assert_t287_tilt(file.path(), degrees);
            }
            assert!(
                pen_tilt_info().resolution > 0,
                "T287: avoid fallback assumptions"
            );
        }
    }

    fn assert_t287_tilt(path: &std::path::Path, degrees: f64) {
        let info = pen_tilt_info();
        for (axis, expected) in [(ABS_TILT_X, degrees), (ABS_TILT_Y, -degrees)] {
            let value = last_input_value(path, axis);
            assert!(
                (info.minimum..=info.maximum).contains(&value),
                "T287: axis bounds"
            );
            let physical = libinput_tilt_degrees(info, value);
            assert!(
                (physical - expected.clamp(-90.0, 90.0)).abs() <= 0.03,
                "T287: {expected} degrees became {physical} degrees (raw={value})"
            );
        }
    }

    // T318: Android motion/button fixtures drive the real wire decoder and
    // Linux event writer, using regular files rather than host uinput devices.
    #[test]
    fn t318_pen_lifecycle_matches_android_samples() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../testdata/pen-lifecycle.json")).unwrap();
        for case in cases {
            let file = tempfile::NamedTempFile::new().unwrap();
            let devices = Arc::new(std::sync::Mutex::new(InjectDevices {
                pen: Some(UInputDevice::from_writer(file.reopen().unwrap())),
                ..InjectDevices::empty()
            }));
            let (mode, _rx) = watch::channel(false);
            let tracker = crate::latency::LatencyTracker::new();
            for step in case["events"].as_array().unwrap() {
                handle_event(
                    serde_json::from_value(step["wire"].clone()).unwrap(),
                    &devices,
                    &SessionSettings::new(&None, &mode, true),
                    &tracker,
                    true,
                );
                assert_t318_pen_state(file.path(), &devices, &case, step);
                if step["name"] == "down-held" {
                    assert_t318_modifier_precedes_tip(
                        file.path(),
                        case["eraser"].as_bool().unwrap(),
                    );
                }
            }
            // The same release operation is used by the controller lease on
            // disconnect/replacement; no stale tool or modifier can survive it.
            devices.lock().unwrap().release_all();
            for key in [BTN_TOUCH, BTN_TOOL_PEN, BTN_TOOL_RUBBER, BTN_STYLUS] {
                assert_eq!(t318_key_state(file.path(), key), 0);
            }
        }
    }

    fn assert_t318_modifier_precedes_tip(path: &std::path::Path, eraser: bool) {
        let frames = input_frames(path);
        let tail = &frames[frames.len() - 3..];
        let tool = if eraser {
            BTN_TOOL_RUBBER
        } else {
            BTN_TOOL_PEN
        };
        assert!(
            tail[0].contains(&(EV_KEY, tool, 1)),
            "T318 tool must enter first"
        );
        assert!(!tail[0].contains(&(EV_KEY, BTN_TOUCH, 1)));
        assert_eq!(
            tail[1],
            [(EV_KEY, BTN_STYLUS, 1)],
            "T318 modifier before tip"
        );
        assert!(tail[2].contains(&(EV_KEY, BTN_TOUCH, 1)));
    }

    fn t318_key_state(path: &std::path::Path, key: u16) -> i32 {
        input_frames(path)
            .iter()
            .flatten()
            .filter(|&&(kind, code, _)| kind == EV_KEY && code == key)
            .map(|&(_, _, value)| value)
            .next_back()
            .unwrap_or(0)
    }

    fn assert_t318_pen_state(
        path: &std::path::Path,
        devices: &std::sync::Mutex<InjectDevices>,
        case: &serde_json::Value,
        step: &serde_json::Value,
    ) {
        let expected = &step["expected"];
        let proximity = expected["proximity"].as_bool().unwrap();
        let tool = if case["eraser"].as_bool().unwrap() {
            BTN_TOOL_RUBBER
        } else {
            BTN_TOOL_PEN
        };
        for (key, value) in [
            (BTN_TOUCH, expected["tip"].as_bool().unwrap()),
            (tool, proximity),
            (BTN_STYLUS, expected["button"].as_bool().unwrap()),
        ] {
            assert_eq!(
                t318_key_state(path, key),
                i32::from(value),
                "T318 {} key {key}",
                step["name"]
            );
        }
        let devices = devices.lock().unwrap();
        assert_eq!(devices.pen_proximity, proximity, "T318 {}", step["name"]);
        assert_eq!(devices.pen_button, expected["button"].as_bool().unwrap());
    }

    #[test]
    fn t317_proximity_exit_releases_stylus_button_for_next_press() {
        for eraser in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let mut devices = InjectDevices {
                pen: Some(UInputDevice::from_writer(file.reopen().unwrap())),
                ..InjectDevices::empty()
            };
            for action in [3, 5, 0, 1] {
                devices.apply_pen(
                    AbsoluteContact {
                        x: 100,
                        y: 200,
                        pressure: 1000,
                    },
                    (0.0, 0.0),
                    eraser,
                    action,
                    None,
                );
            }
            // Lifting the tip alone must preserve a physically held button.
            assert!(devices.pen_button);
            assert_eq!(last_input_value(file.path(), BTN_STYLUS), 1);
            devices.apply_pen(
                AbsoluteContact {
                    x: 0,
                    y: 0,
                    pressure: 0,
                },
                (0.0, 0.0),
                eraser,
                4,
                None,
            );
            assert_eq!(
                last_input_value(file.path(), BTN_STYLUS),
                0,
                "T317: proximity-out left the kernel key state pressed"
            );
            assert!(!devices.pen_button, "T317: stale controller button state");
            assert!(!devices.pen_proximity);
            for action in [3, 5, 6, 4] {
                devices.apply_pen(
                    AbsoluteContact {
                        x: 300,
                        y: 400,
                        pressure: 0,
                    },
                    (0.0, 0.0),
                    eraser,
                    action,
                    None,
                );
            }
            // Model Linux input_get_disposition's duplicate-key filtering.
            // Both gestures must contain a distinct press and release.
            let mut state = 0;
            let transitions: Vec<_> = input_frames(file.path())
                .into_iter()
                .flatten()
                .filter(|&(kind, code, _)| kind == EV_KEY && code == BTN_STYLUS)
                .filter_map(|(_, _, value)| {
                    if value == state {
                        return None;
                    }
                    state = value;
                    Some(value)
                })
                .collect();
            assert_eq!(transitions, [1, 0, 1, 0], "T317: next click was lost");
            assert!(!devices.pen_button);
        }
    }

    #[test]
    fn t286_release_updates_pen_axes_before_leaving_proximity() {
        for eraser in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let mut pen = UInputDevice::from_writer(file.reopen().unwrap());
            pen.inject_pen(100, 200, 1000, 10, 20, 0, eraser, None)
                .unwrap();
            pen.inject_pen(300, 400, 123, 30, -40, 1, eraser, None)
                .unwrap();
            let tip_frames = input_frames(file.path());
            let tool = if eraser {
                BTN_TOOL_RUBBER
            } else {
                BTN_TOOL_PEN
            };
            assert!(
                !tip_frames
                    .iter()
                    .any(|frame| frame.contains(&(EV_KEY, tool, 0))),
                "T318 tip-up must retain proximity"
            );
            pen.inject_pen(0, 0, 0, 0, 0, 4, eraser, None).unwrap();
            let frames = input_frames(file.path());
            let release = frames
                .iter()
                .position(|frame| frame.contains(&(EV_KEY, BTN_TOUCH, 0)))
                .unwrap();
            for (code, value) in [
                (ABS_X, 300),
                (ABS_Y, 400),
                (ABS_TILT_X, 30),
                (ABS_TILT_Y, -40),
                (ABS_PRESSURE, 0),
            ] {
                assert!(
                    frames[release].contains(&(EV_ABS, code, value)),
                    "T286 release lost final axis {code}: {:?}",
                    frames[release]
                );
            }
            let tool = if eraser {
                BTN_TOOL_RUBBER
            } else {
                BTN_TOOL_PEN
            };
            assert!(
                !frames[release].contains(&(EV_KEY, tool, 0)),
                "libinput discards updated axes in a proximity-out frame"
            );
            assert_eq!(
                frames[release + 1],
                [
                    (EV_KEY, BTN_STYLUS, 0),
                    (EV_KEY, BTN_TOUCH, 0),
                    (EV_KEY, BTN_TOOL_PEN, 0),
                    (EV_KEY, BTN_TOOL_RUBBER, 0),
                    (EV_ABS, ABS_PRESSURE, 0),
                ]
            );
        }
    }

    #[test]
    fn t286_hover_exit_parks_pointer_at_final_release_position() {
        for eraser in [false, true] {
            let pen = tempfile::NamedTempFile::new().unwrap();
            let pointer = tempfile::NamedTempFile::new().unwrap();
            let mut devices = InjectDevices {
                pen: Some(UInputDevice::from_writer(pen.reopen().unwrap())),
                pointer: Some(UInputDevice::from_writer(pointer.reopen().unwrap())),
                ..InjectDevices::empty()
            };
            for (action, x, y) in [(0, 100, 200), (1, 300, 400), (4, 0, 0)] {
                devices.apply_pen(
                    AbsoluteContact {
                        x,
                        y,
                        pressure: 123,
                    },
                    (0.0, 0.0),
                    eraser,
                    action,
                    None,
                );
            }
            assert_eq!(
                last_input_value(pointer.path(), ABS_X),
                300,
                "T286 stale cursor X"
            );
            assert_eq!(
                last_input_value(pointer.path(), ABS_Y),
                400,
                "T286 stale cursor Y"
            );
            assert!(!devices.pen_proximity);
            assert_eq!(last_input_value(pen.path(), BTN_TOUCH), 0);
            assert_eq!(last_input_value(pen.path(), ABS_PRESSURE), 0);
        }
    }

    #[test]
    fn t146_normalized_input_stays_in_axes_and_releases() {
        for pen in [false, true] {
            let file = tempfile::NamedTempFile::new().unwrap();
            let device = UInputDevice::from_writer(file.reopen().unwrap());
            let mut devices = InjectDevices::empty();
            if pen {
                devices.pen = Some(device);
            } else {
                devices.touch = Some(device);
            }
            let devices = Arc::new(std::sync::Mutex::new(devices));
            let (mode, _rx) = watch::channel(false);
            let tracker = crate::latency::LatencyTracker::new();
            let send = |action, x, y, pressure| {
                let event = if pen {
                    InputEvent::Pen {
                        x,
                        y,
                        pressure,
                        action,
                        tilt_x: 0.0,
                        tilt_y: 0.0,
                        eraser: false,
                        button: None,
                    }
                } else {
                    InputEvent::Touch {
                        x,
                        y,
                        pressure,
                        action,
                        slot: 0,
                    }
                };
                handle_event(
                    event,
                    &devices,
                    &SessionSettings::new(&None, &mode, pen),
                    &tracker,
                    pen,
                );
            };
            let last = |code| last_input_value(file.path(), code);
            send(0, -0.2, 1.4, 2.0);
            assert_eq!(last(ABS_X), 0, "T146 pen={pen}");
            assert_eq!(last(ABS_Y), COORD_MAX);
            assert_eq!(last(ABS_PRESSURE), 4096);
            send(2, 1.0, 0.0, -0.2);
            assert_eq!(last(ABS_X), COORD_MAX);
            assert_eq!(last(ABS_Y), 0);
            assert_eq!(last(ABS_PRESSURE), 0);
            send(1, 1.5, -0.5, 0.0);
            assert_eq!(last(BTN_TOUCH), 0, "out-of-bounds release must survive");
        }
    }

    #[test]
    fn t086_touch_stays_down_until_last_contact_lifts() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let devices = Arc::new(std::sync::Mutex::new(InjectDevices {
            touch: Some(UInputDevice::from_writer(file.reopen().unwrap())),
            ..InjectDevices::empty()
        }));
        let (mode, _rx) = watch::channel(false);
        let tracker = crate::latency::LatencyTracker::new();
        let send = |slot, action, x, y, pressure| {
            handle_event(
                InputEvent::Touch {
                    slot,
                    action,
                    x,
                    y,
                    pressure,
                },
                &devices,
                &SessionSettings::new(&None, &mode, false),
                &tracker,
                false,
            )
        };
        let last = |code| last_input_value(file.path(), code);
        send(0, 0, 0.25, 0.25, 0.5);
        send(1, 0, 0.75, 0.75, 0.75);
        send(0, 1, 0.25, 0.25, 0.0);
        assert_eq!(last(BTN_TOUCH), 1);
        assert_eq!(last(BTN_TOOL_FINGER), 1);
        assert_eq!(last(ABS_PRESSURE), 3072);
        assert_eq!(last(ABS_X), (0.75 * COORD_MAX as f64) as i32);
        send(1, 2, 0.5, 0.5, 0.5);
        assert_eq!(last(ABS_PRESSURE), 2048);
        send(1, 1, 0.5, 0.5, 0.0);
        assert_eq!(last(BTN_TOUCH), 0);
        assert_eq!(last(BTN_TOOL_FINGER), 0);
        assert_eq!(last(ABS_PRESSURE), 0);
    }

    #[tokio::test]
    async fn t092_idle_and_partial_upgrades_expire() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for partial in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (socket, _) = listener.accept().await.unwrap();
            let (mode, _rx) = watch::channel(false);
            let task = tokio::spawn(handle_connection(
                PendingInput::new(socket),
                InputConfig::default(),
                None,
                mode,
                crate::latency::LatencyTracker::new(),
                Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
                    InjectDevices::empty(),
                )))),
                Arc::new(tokio::sync::Notify::new()),
            ));
            if partial {
                client.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
            }
            let mut byte = [0];
            let closed = tokio::time::timeout(
                std::time::Duration::from_millis(3500),
                client.read(&mut byte),
            )
            .await;
            assert!(
                matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
                "upgrade outlives accept-to-auth deadline"
            );
            assert!(task.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn t092_pending_connections_are_bounded() {
        use tokio::io::AsyncReadExt;
        let (mode, _mode_rx) = watch::channel(false);
        let (_card_tx, card) = watch::channel(None);
        let (_tablet_tx, tablet) = watch::channel(false);
        let server = InputServer::new(
            InputConfig {
                port: 0,
                touch: false,
                pen: false,
                pointer: false,
                ..InputConfig::default()
            },
            None,
            mode,
            crate::latency::LatencyTracker::new(),
            Arc::new(tokio::sync::Notify::new()),
            card,
            tablet,
        );
        let listener = server.bind().await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { server.run_with_listener(listener).await });
        let mut clients = Vec::new();
        for _ in 0..17 {
            clients.push(tokio::net::TcpStream::connect(addr).await.unwrap());
        }
        let mut byte = [0];
        let closed = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            clients.last_mut().unwrap().read(&mut byte),
        )
        .await;
        task.abort();
        let _ = task.await;
        assert!(
            matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
            "unbounded pending handlers"
        );
    }

    #[tokio::test]
    async fn t323_replacement_cancels_a_backpressured_control_writer() {
        use tokio::io::AsyncWriteExt;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        // Bound both directions so a non-reading peer deterministically fills
        // the server's Pong send buffer without a large memory/network load.
        for stream in [&client, &socket] {
            for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
                let size: libc::c_int = 4096;
                assert_eq!(
                    unsafe {
                        libc::setsockopt(
                            stream.as_raw_fd(),
                            libc::SOL_SOCKET,
                            option,
                            (&size as *const libc::c_int).cast(),
                            std::mem::size_of_val(&size) as libc::socklen_t,
                        )
                    },
                    0
                );
            }
        }
        let controllers = Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
            InjectDevices::empty(),
        ))));
        let (mode, _mode_rx) = watch::channel(false);
        let mut task = tokio::spawn(handle_connection(
            PendingInput::new(socket),
            InputConfig::default(),
            None,
            mode,
            crate::latency::LatencyTracker::new(),
            controllers.clone(),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let (mut client, _) = tokio_tungstenite::client_async("ws://localhost/", client)
            .await
            .unwrap();
        assert!(matches!(client.next().await, Some(Ok(Message::Text(_)))));
        // Valid masked 125-byte Ping frames; do not consume the Pong replies.
        let mut ping = vec![0x89, 0xfd, 0, 0, 0, 0];
        ping.extend_from_slice(&[1; 125]);
        let traffic = ping.repeat(4096);
        let stalled = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            client.get_mut().write_all(&traffic),
        )
        .await
        .is_err();
        let _replacement = controllers.claim();
        let retired = tokio::time::timeout(std::time::Duration::from_millis(500), &mut task).await;
        task.abort();
        if retired.is_err() {
            let _ = task.await;
        }
        assert!(stalled, "T323: fixture did not reach socket backpressure");
        assert!(
            matches!(retired, Ok(Ok(Ok(())))),
            "T323: replaced controller retained its blocked writer"
        );
    }

    // T085: a retired socket must not release the replacement controller's contact.
    #[tokio::test]
    async fn t085_reconnect_preserves_current_controller_contacts() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let devices = Arc::new(std::sync::Mutex::new(InjectDevices {
            touch: Some(UInputDevice::from_writer(file.reopen().unwrap())),
            ..InjectDevices::empty()
        }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (mode, _rx) = watch::channel(false);
        let controllers = Arc::new(Controllers::new(devices.clone()));
        let mut clients = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let connection = tokio::spawn(connect_async(format!("ws://{addr}")));
            let (socket, _) = listener.accept().await.unwrap();
            tasks.push(tokio::spawn(handle_connection(
                PendingInput::new(socket),
                InputConfig::default(),
                None,
                mode.clone(),
                crate::latency::LatencyTracker::new(),
                controllers.clone(),
                Arc::new(tokio::sync::Notify::new()),
            )));
            let (mut client, _) = connection.await.unwrap().unwrap();
            response(&mut client).await;
            clients.push(client);
        }
        clients[1]
            .send(Message::Text(
                r#"{"type":"touch","x":0.5,"y":0.5,"pressure":0.5,"action":0,"slot":0}"#.into(),
            ))
            .await
            .unwrap();
        clients[1].send(Message::Ping(vec![1])).await.unwrap();
        assert!(matches!(
            clients[1].next().await,
            Some(Ok(Message::Pong(_)))
        ));
        let _ = clients[0].close(None).await;
        tasks.remove(0).await.unwrap().unwrap();
        assert_eq!(devices.lock().unwrap().active_slots, 1);
        let events = std::fs::read(file.path()).unwrap();
        let keys: Vec<i32> = events
            .as_chunks::<{ std::mem::size_of::<LinuxInputEvent>() }>()
            .0
            .iter()
            .filter_map(|event| {
                let code = u16::from_ne_bytes(event[18..20].try_into().unwrap());
                (code == BTN_TOUCH).then(|| i32::from_ne_bytes(event[20..24].try_into().unwrap()))
            })
            .collect();
        assert_eq!(keys.last(), Some(&1));
        clients[1].close(None).await.unwrap();
        tasks.remove(0).await.unwrap().unwrap();
        assert_eq!(devices.lock().unwrap().active_slots, 0);
    }

    #[tokio::test]
    async fn t109_state_changes_cancel_pending_mapping() {
        for changed in 0..3 {
            let (tablet_tx, mut tablet) = watch::channel(true);
            let (mode_tx, mut mode) = watch::channel(false);
            let (card_tx, mut card) = watch::channel(Some(1));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let mapping = async move {
                started_tx.send(()).unwrap();
                std::future::pending::<()>().await;
                panic!("stale mapping completed");
            };
            let task = tokio::spawn(async move {
                wait_for_mapping_change(&mut tablet, &mut mode, &mut card, mapping).await
            });
            started_rx.await.unwrap();
            match changed {
                0 => {
                    tablet_tx.send(false).unwrap();
                }
                1 => {
                    mode_tx.send(true).unwrap();
                }
                _ => {
                    card_tx.send(Some(2)).unwrap();
                }
            }
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(100), task)
                    .await
                    .expect("mapping must be interrupted")
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn t084_stopping_server_closes_clients_and_watcher() {
        let (mode_tx, _mode_rx) = watch::channel(false);
        let (_card_tx, card_rx) = watch::channel(None);
        let (tablet_tx, tablet_rx) = watch::channel(false);
        let server = InputServer::new(
            InputConfig {
                port: 0,
                touch: false,
                pen: false,
                pointer: false,
                ..InputConfig::default()
            },
            None,
            mode_tx.clone(),
            crate::latency::LatencyTracker::new(),
            Arc::new(tokio::sync::Notify::new()),
            card_rx,
            tablet_rx,
        );
        let listener = server.bind().await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { server.run_with_listener(listener).await });
        let (mut client, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        response(&mut client).await;
        task.abort();
        let _ = task.await;
        let _ = client.send(Message::Ping(vec![42])).await;
        let next = tokio::time::timeout(std::time::Duration::from_millis(300), client.next()).await;
        assert!(
            !matches!(next, Ok(Some(Ok(Message::Pong(_))))),
            "old controller remains alive"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(tablet_tx.receiver_count(), 0, "watcher survives server");
        assert_eq!(
            mode_tx.receiver_count(),
            1,
            "old session retains mode receiver"
        );
    }

    #[tokio::test]
    async fn t029_x11_maps_only_this_tablets_devices_and_card() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("uscreen-x11-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let xinput = root.join("xinput");
        let xrandr = root.join("xrandr");
        std::fs::write(&xinput, r#"#!/bin/sh
cd "$(dirname "$0")"
if [ "$1" = list ]; then
    printf '%s\n' '↳ UScreen Touch id=10 [slave pointer]' '↳ UScreen Pen Pen (0) id=11 [slave pointer]' '↳ UScreen Touch 2 id=20 [slave pointer]' '↳ UScreen Pen 2 Pen (0) id=21 [slave pointer]' '↳ UScreen Pen 2 Eraser (0) id=22 [slave pointer]' '↳ UScreen Pointer 2 id=23 [slave pointer]'
else
    printf '%s %s %s\n' "$1" "$2" "$3" >> mapped
fi
"#).unwrap();
        std::fs::write(&xrandr, "#!/bin/sh\nprintf '%s\n' 'eDP-1 connected primary 1920x1080+0+0' 'DVI-I-1-1 connected 1920x1080+1920+0' 'DVI-I-2-1 connected 1920x1080+3840+0'\n").unwrap();
        for path in [&xinput, &xrandr] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let connectors = [
            crate::vdisplay::EvdiConnector {
                name: "DVI-I-1".into(),
                card: 8,
                connected: true,
            },
            crate::vdisplay::EvdiConnector {
                name: "DVI-I-2".into(),
                card: 9,
                connected: true,
            },
        ];
        map_devices_using(
            false,
            &DeviceIdentity::for_instance(1),
            Some(9),
            3,
            "x11",
            xinput.to_str().unwrap(),
            xrandr.to_str().unwrap(),
            Some(&connectors),
        )
        .await;
        let mapped = std::fs::read_to_string(root.join("mapped")).unwrap_or_default();
        assert_eq!(
            mapped.lines().collect::<Vec<_>>(),
            [
                "map-to-output 20 DVI-I-2-1",
                "map-to-output 21 DVI-I-2-1",
                "map-to-output 22 DVI-I-2-1",
                "map-to-output 23 DVI-I-2-1"
            ]
        );
        std::fs::remove_file(root.join("mapped")).unwrap();
        map_devices_using(
            true,
            &DeviceIdentity::for_instance(0),
            Some(8),
            2,
            "x11",
            xinput.to_str().unwrap(),
            xrandr.to_str().unwrap(),
            Some(&connectors),
        )
        .await;
        let mapped = std::fs::read_to_string(root.join("mapped")).unwrap();
        assert_eq!(
            mapped.lines().collect::<Vec<_>>(),
            ["map-to-output 10 eDP-1", "map-to-output 11 eDP-1"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    use super::*;
    use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

    fn settings(encoder: &str) -> EncoderSettings {
        EncoderSettings {
            encoder: encoder.into(),
            fps: 60,
            bitrate: 20_000,
            width: 1920,
            height: 1080,
            quality: 18,
            width_mm: 310,
            height_mm: 194,
            stream_scale: 1,
            geometry_ready: true,
            decoders: None,
            decoder_epoch: 0,
            selection: None,
        }
    }

    #[cfg(not(feature = "inproc-encoder"))]
    struct T472Publisher {
        tx: watch::Sender<EncoderSettings>,
        observed: std::sync::Arc<std::sync::Mutex<watch::Receiver<EncoderSettings>>>,
        fired: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[cfg(not(feature = "inproc-encoder"))]
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for T472Publisher {
        fn on_event(&self, _: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            if self.fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            self.tx.send_modify(|current| {
                current.decoder_epoch += 1;
                current.selection = Some(crate::selection::Selected {
                    key: crate::selection::Key::new(current),
                    encoder: "h264_vaapi".into(),
                    reason: "T472 concurrent selector".into(),
                    verified: true,
                    decoder: None,
                });
            });
            self.observed.lock().unwrap().borrow_and_update();
        }
    }

    #[cfg(not(feature = "inproc-encoder"))]
    #[test]
    fn t472_clamped_config_preserves_concurrent_selection_without_restart() {
        use tracing_subscriber::prelude::*;
        let mut initial = settings("auto");
        initial.bitrate = crate::config::MAX_BITRATE_KBPS;
        let (tx, rx) = watch::channel(initial);
        let observed = std::sync::Arc::new(std::sync::Mutex::new(rx));
        let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let subscriber = tracing_subscriber::registry().with(T472Publisher {
            tx: tx.clone(),
            observed: observed.clone(),
            fired: fired.clone(),
        });
        // Clamp logging gives a deterministic concurrent publication at the old
        // read/modify/write gap; the fixed path normalizes before taking the lock.
        tracing::subscriber::with_default(subscriber, || {
            settings::apply_tablet_config(&Some(tx.clone()), Some(u32::MAX), None, None);
        });
        assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
        let current = tx.borrow();
        assert_eq!(
            current.effective_encoder(),
            "h264_vaapi",
            "T472: lost selector publication"
        );
        assert_eq!(current.decoder_epoch, 1);
        assert!(
            !observed.lock().unwrap().has_changed().unwrap(),
            "T472: duplicate config restarted stream"
        );
    }

    #[test]
    fn t472_repeated_geometry_and_config_retain_unrelated_state() {
        let mut initial = settings("libx264");
        initial.decoder_epoch = 7;
        initial.decoders = Some(crate::media::DecoderCapabilities {
            protocol: 1,
            width: 1920,
            height: 1080,
            fps: 60,
            codecs: vec!["h264".into()],
            hardware: vec![],
            ..Default::default()
        });
        let (tx, mut rx) = watch::channel(initial.clone());
        let source = Some(tx.clone());
        settings::apply_tablet_resolution(&source, (1920, 1080), (310, 194), true);
        settings::apply_tablet_config(&source, Some(20_000), Some(60), None);
        assert!(!rx.has_changed().unwrap());
        settings::apply_tablet_config(&source, Some(25_000), None, None);
        assert_eq!(tx.borrow().decoder_epoch, initial.decoder_epoch);
        assert_eq!(tx.borrow().decoders, initial.decoders);
        rx.borrow_and_update();
        settings::apply_tablet_resolution(&source, (1280, 800), (220, 138), true);
        let updated = rx.borrow_and_update().clone();
        assert_eq!(
            (updated.width, updated.height, updated.bitrate),
            (1280, 800, 25_000)
        );
        // T478: actual format changes retire scope; bitrate-only updates and
        // duplicate geometry above/below must still preserve unrelated state.
        assert_eq!(updated.decoder_epoch, initial.decoder_epoch + 1);
        assert!(updated.decoders.is_none());
        settings::apply_tablet_resolution(&source, (1280, 800), (220, 138), true);
        assert!(!rx.has_changed().unwrap());
    }

    #[test]
    fn t332_invalid_fps_or_geometry_never_retires_working_settings() {
        let mut initial = settings("libx264");
        initial.width = 3840;
        initial.height = 2160;
        let (tx, rx) = watch::channel(initial.clone());
        let source = Some(tx.clone());
        settings::apply_tablet_config(&source, Some(25_000), Some(90), None);
        assert_eq!(
            *tx.borrow(),
            initial,
            "T332: reject the whole incompatible command"
        );
        assert!(
            !rx.has_changed().unwrap(),
            "T332: invalid FPS must not restart capture"
        );
        settings::apply_tablet_resolution(&source, (4095, 4095), (300, 190), true);
        assert_eq!(
            *tx.borrow(),
            initial,
            "T332: invalid geometry must preserve the stream"
        );
        assert!(!rx.has_changed().unwrap());
        let reply = InputConfig::default().response("mode", false, &source);
        assert_eq!(
            (reply.width, reply.height, reply.fps),
            (3840, 2160, Some(60))
        );
    }

    #[test]
    fn t332_manual_geometry_validates_selected_dimensions_at_current_fps() {
        let mut current = settings("libx264");
        current.fps = 90;
        assert!(negotiated_geometry(&current, (3840, 2160), (300, 190), true).is_none());
        let manual = negotiated_geometry(&current, (3840, 2160), (300, 190), false).unwrap();
        assert_eq!((manual.width, manual.height), (1920, 1080));
        current.width = 3840;
        current.height = 2160;
        assert!(negotiated_geometry(&current, (1920, 1080), (300, 190), false).is_none());
    }

    #[test]
    fn t276_replies_follow_geometry_and_scale_across_reconnect() {
        let cfg = InputConfig::default();
        let (tx, _rx) = watch::channel(settings("h264_nvenc"));
        let source = Some(tx.clone());
        for (width, height, scale, ready) in [
            (1920, 1080, 1, false),
            (1280, 800, 2, true),
            (2560, 1600, 4, true),
        ] {
            tx.send_modify(|s| {
                s.width = width;
                s.height = height;
                s.stream_scale = scale;
                s.geometry_ready = ready;
            });
            for status in ["mode", "connected"] {
                let reply = cfg.response(status, false, &source);
                assert_eq!(
                    (reply.width, reply.height),
                    (width, height),
                    "T276: {status}"
                );
                assert_eq!(
                    (reply.video_width, reply.video_height),
                    (width / scale, height / scale)
                );
            }
        }
        let fallback = cfg.response("connected", false, &None);
        assert_eq!((fallback.width, fallback.height), (2960, 1848));
        assert_eq!((fallback.video_width, fallback.video_height), (2960, 1848));
    }

    #[test]
    fn t276_scaled_greeting_matches_android_fixture() {
        let cfg = InputConfig::default();
        let mut current = settings("h264_nvenc");
        current.width = 1280;
        current.height = 800;
        current.stream_scale = 2;
        current.fps = 30;
        let (tx, _rx) = watch::channel(current);
        let actual = serde_json::to_value(cfg.response("connected", false, &Some(tx))).unwrap();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../testdata/control-scaled.json")).unwrap();
        assert_eq!(actual, fixture, "T276");
    }

    #[test]
    fn t346_greeting_uses_one_encoder_settings_snapshot() {
        let cfg = InputConfig::default();
        let fallback = cfg.response("connected", false, &None);
        assert_eq!(fallback.codec, cfg.codec);
        assert_eq!(fallback.fps, None);
        let (tx, _rx) = watch::channel(settings("h264_nvenc"));
        let source = Some(tx.clone());
        let start = std::sync::Barrier::new(2);
        let mixed = std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                for i in 0..50_000 {
                    tx.send_modify(|s| {
                        let (encoder, fps) = if i % 2 == 0 {
                            ("hevc_nvenc", 30)
                        } else {
                            ("h264_nvenc", 60)
                        };
                        s.encoder = encoder.into();
                        s.fps = fps;
                    });
                }
            });
            start.wait();
            let mut mixed = 0;
            for _ in 0..50_000 {
                let response = cfg.response("connected", false, &source);
                if !matches!(
                    (response.codec.as_str(), response.fps),
                    ("h264", Some(60)) | ("hevc", Some(30))
                ) {
                    mixed += 1;
                }
            }
            mixed
        });
        assert_eq!(mixed, 0, "T346: greeting mixed codec and FPS revisions");
    }

    #[tokio::test]
    async fn t388_greeting_reports_accepted_route_and_omits_unknown() {
        use uscreen_config::adb::Transport;
        for (route, expected) in [
            (Some(Transport::Usb), Some("usb")),
            (Some(Transport::Network), Some("network")),
            (None, None),
        ] {
            let (settings, _) = watch::channel(settings("libx264"));
            let attachment = crate::attachment::Attachment::new(settings);
            attachment.begin_with_transport(Some("same-physical-tablet".into()), route);
            let (mut client, _, task) =
                connection_with_attachment("libx264", Some(attachment.clone())).await;
            let greeting = response(&mut client).await;
            assert_eq!(greeting["status"], "connected");
            assert_eq!(greeting.get("transport").and_then(|v| v.as_str()), expected);
            attachment.begin_with_transport(
                Some("same-physical-tablet".into()),
                Some(Transport::Network),
            );
            let retired = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;
            assert!(
                retired.is_ok(),
                "T388: migrated route retained old controller"
            );
        }
    }

    async fn connection(
        encoder: &str,
    ) -> (
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        watch::Sender<EncoderSettings>,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        connection_with_attachment(encoder, None).await
    }

    async fn connection_with_attachment(
        encoder: &str,
        attachment: Option<crate::attachment::Attachment>,
    ) -> (
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        watch::Sender<EncoderSettings>,
        tokio::task::JoinHandle<Result<()>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (settings_tx, _settings_rx) = watch::channel(settings(encoder));
        let tx = settings_tx.clone();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mode_tx, _rx) = watch::channel(false);
            let mut incoming = PendingInput::new(socket);
            incoming.attachment = attachment
                .as_ref()
                .map(crate::attachment::Attachment::lease);
            handle_connection(
                incoming,
                InputConfig {
                    touch: false,
                    pen: false,
                    ..InputConfig::default()
                },
                Some(tx),
                mode_tx,
                crate::latency::LatencyTracker::new(),
                Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
                    InjectDevices::empty(),
                )))),
                Arc::new(tokio::sync::Notify::new()),
            )
            .await
        });
        let (client, _) = connect_async(format!("ws://{addr}")).await.unwrap();
        (client, settings_tx, task)
    }

    async fn response(
        client: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    ) -> serde_json::Value {
        let msg = tokio::time::timeout(std::time::Duration::from_millis(300), client.next())
            .await
            .expect("server must publish settings changes")
            .unwrap()
            .unwrap();
        serde_json::from_str(msg.to_text().unwrap()).unwrap()
    }

    #[test]
    fn t223_fixed_resolution_keeps_pixels_but_negotiates_physical_size() {
        let mut initial = settings("libx264");
        initial.geometry_ready = false;
        for auto in [false, true] {
            let next = negotiated_geometry(&initial, (1280, 800), (220, 138), auto).unwrap();
            assert!(next.geometry_ready);
            assert_eq!((next.width_mm, next.height_mm), (220, 138));
            assert_eq!(
                (next.width, next.height),
                if auto { (1280, 800) } else { (1920, 1080) }
            );
        }
        assert!(negotiated_geometry(&initial, (0, 0), (220, 138), false).is_none());
    }

    #[test]
    fn t275_manual_geometry_is_independent_of_native_edid_limits() {
        let initial = settings("libx264");
        for native in [(4096, 2160), (5120, 3200), (320, 240)] {
            let selected =
                negotiated_geometry(&initial, native, (300, 190), false).unwrap_or_else(|| {
                    panic!("T275: rejected valid manual mode for native {native:?}")
                });
            assert!(selected.geometry_ready);
            assert_eq!((selected.width, selected.height), (1920, 1080));
            assert_eq!((selected.width_mm, selected.height_mm), (300, 190));
        }
        let fallback = negotiated_geometry(&initial, (5120, 3200), (0, 10), false).unwrap();
        assert_eq!(
            (fallback.width_mm, fallback.height_mm),
            (
                crate::edid::DEFAULT_WIDTH_MM,
                crate::edid::DEFAULT_HEIGHT_MM
            )
        );
        for native in [(0, 0), (0, 2160), (4096, 0)] {
            assert!(negotiated_geometry(&initial, native, (300, 190), false).is_none());
        }
    }

    #[test]
    fn t275_selected_manual_and_automatic_modes_keep_edid_bounds() {
        let initial = settings("libx264");
        for pixels in [(1920, 1080), (640, 480)] {
            let next = negotiated_geometry(&initial, pixels, (300, 190), true).unwrap();
            assert_eq!((next.width, next.height), pixels);
        }
        for pixels in [(4096, 2160), (5120, 3200), (320, 240)] {
            assert!(negotiated_geometry(&initial, pixels, (300, 190), true).is_none());
            let invalid = EncoderSettings {
                width: pixels.0,
                height: pixels.1,
                ..initial.clone()
            };
            assert!(
                negotiated_geometry(&invalid, (1920, 1080), (300, 190), false).is_none(),
                "T275: invalid manual mode accepted"
            );
        }
    }

    #[tokio::test]
    async fn t120_greeting_and_apply_report_effective_frame_rate() {
        let (mut client, tx, task) = connection("libx264").await;
        assert_eq!(response(&mut client).await["fps"], 60);
        let mut next = settings("libx264");
        next.fps = 30;
        tx.send_replace(next);
        assert_eq!(response(&mut client).await["fps"], 30);
        client
            .send(Message::Text(r#"{"type":"config","fps":90}"#.into()))
            .await
            .unwrap();
        assert_eq!(response(&mut client).await["fps"], 90);
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
    }

    #[test]
    fn t432_new_controller_cannot_reuse_old_apk_capabilities() {
        let mut initial = settings("libvpx-vp9");
        let (width, height) = initial.video_dimensions();
        initial.decoders = Some(crate::media::DecoderCapabilities {
            protocol: 1,
            width,
            height,
            fps: initial.fps,
            codecs: vec!["vp9".into()],
            hardware: vec![],
            ..Default::default()
        });
        assert_eq!(initial.effective_encoder(), "libvpx-vp9");
        let (tx, rx) = watch::channel(initial);
        let source = Some(tx);
        let (mode, _) = watch::channel(false);
        let session = SessionSettings::new(&source, &mode, false);
        let controllers = Arc::new(Controllers::new(Arc::new(std::sync::Mutex::new(
            InjectDevices::empty(),
        ))));
        let _lease = claim_controller(&controllers, None, &session).unwrap();
        assert!(rx.borrow().decoders.is_none());
        assert_eq!(rx.borrow().effective_encoder(), "libx264");
    }

    #[tokio::test]
    async fn t432_vp9_requires_current_peer_format_support() {
        framed_peer_support("libvpx-vp9", "vp9").await;
    }

    #[test]
    fn t478_malformed_stale_and_away_back_reports_cannot_replace_current_scope() {
        use super::settings::SettingsSink;
        let initial = settings("auto");
        let (tx, _) = watch::channel(initial.clone());
        let source = Some(tx.clone());
        let (mode, _) = watch::channel(false);
        let session = SessionSettings::new(&source, &mode, false);
        let mut caps: crate::media::DecoderCapabilities =
            serde_json::from_str(include_str!("../../testdata/decoder-capabilities-v2.json"))
                .unwrap();
        (caps.width, caps.height) = initial.video_dimensions();
        caps.scope = Some(initial.decoder_epoch.to_string());
        session.decoders(caps.clone());
        assert_eq!(tx.borrow().decoders.as_ref(), Some(&caps));
        let mut malformed = caps.clone();
        malformed.protocol = 99;
        session.decoders(malformed);
        assert_eq!(tx.borrow().decoders.as_ref(), Some(&caps));
        session.configure(None, Some(30), None);
        session.configure(None, Some(60), None);
        assert_ne!(tx.borrow().decoder_epoch, initial.decoder_epoch);
        session.decoders(caps.clone());
        assert!(
            tx.borrow().decoders.is_none(),
            "T478: away/back reused retired report"
        );
        caps.scope = Some(tx.borrow().decoder_epoch.to_string());
        session.decoders(caps);
        assert!(tx.borrow().decoder_supports(crate::media::Codec::H264));
    }

    #[tokio::test]
    async fn t434_disconnect_retires_capability_epoch_without_persisting_fallback() {
        let (mut client, tx, task) = connection("auto").await;
        let greeting = response(&mut client).await;
        assert_eq!(greeting["requested_encoder"], "auto");
        assert_eq!(greeting["effective_encoder"], "libx264");
        let epoch = tx.borrow().decoder_epoch;
        client.send(Message::Text(serde_json::json!({"type":"decoders", "capabilities": {
            "protocol": 1, "width": greeting["video_width"], "height": greeting["video_height"],
            "fps": greeting["fps"], "codecs": ["h264"], "hardware": []
        }}).to_string())).await.unwrap();
        response(&mut client).await;
        assert!(tx.borrow().decoders.is_some());
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert!(tx.borrow().decoders.is_none());
        assert_ne!(tx.borrow().decoder_epoch, epoch);
        assert_eq!(tx.borrow().encoder, "auto");
    }

    #[tokio::test]
    async fn t433_av1_requires_current_peer_format_support() {
        framed_peer_support("libaom-av1", "av1").await;
    }

    async fn framed_peer_support(encoder: &str, codec: &str) {
        let (mut client, tx, task) = connection(encoder).await;
        let greeting = response(&mut client).await;
        assert_eq!(
            greeting["codec"], "h264",
            "T432: old/unknown peer needs fallback"
        );
        let caps = serde_json::json!({ "protocol": 1, "width": greeting["video_width"],
            "height": greeting["video_height"], "fps": greeting["fps"], "codecs": [codec] });
        client
            .send(Message::Text(
                serde_json::json!({"type":"decoders", "capabilities":caps}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(response(&mut client).await["codec"], codec);
        assert_eq!(
            tx.borrow().encoder,
            encoder,
            "T432/T433: do not persist fallback over preference"
        );
        tx.send_modify(|settings| settings.fps = 30);
        assert_eq!(
            response(&mut client).await["codec"],
            "h264",
            "T432: stale capability must not authorize changed rate"
        );
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t332_rejected_command_returns_current_settings_without_restart() {
        let (mut client, tx, task) = connection("libx264").await;
        response(&mut client).await;
        tx.send_modify(|s| {
            s.width = 3840;
            s.height = 2160;
        });
        response(&mut client).await;
        let mut unchanged = tx.subscribe();
        unchanged.borrow_and_update();
        client
            .send(Message::Text(
                r#"{"type":"config","fps":90,"bitrate":25000}"#.into(),
            ))
            .await
            .unwrap();
        let reply = response(&mut client).await;
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../testdata/settings-rejected.json")).unwrap();
        assert_eq!(reply, fixture, "T332");
        assert!(
            !unchanged.has_changed().unwrap(),
            "T332: rejection restarted capture"
        );
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn t063_greeting_reports_live_encoder_codec() {
        let (mut client, _tx, task) = connection("hevc_nvenc").await;
        let greeting = response(&mut client).await;
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(greeting["codec"], "hevc");
    }

    #[tokio::test]
    async fn t063_codec_changes_are_pushed_to_connected_clients() {
        let (mut client, tx, task) = connection("h264_nvenc").await;
        assert_eq!(response(&mut client).await["codec"], "h264");
        tx.send_replace(settings("hevc_nvenc"));
        let update = response(&mut client).await;
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(update["codec"], "hevc");
    }

    #[cfg(feature = "inproc-encoder")]
    #[test]
    fn t284_tablet_cannot_switch_inproc_capture_to_vaapi() {
        for encoder in [
            "h264_vaapi",
            "h264_vaapi_baseline",
            "hevc_vaapi",
            "vaapih264enc",
        ] {
            let (tx, rx) = watch::channel(settings("libx264"));
            apply_tablet_config(&Some(tx), Some(2000), Some(45), Some(encoder.into()));
            assert_eq!(
                rx.borrow().encoder,
                "libx264",
                "T284: unsupported live encoder accepted"
            );
            assert_eq!((rx.borrow().bitrate, rx.borrow().fps), (2000, 45));
        }
    }

    #[test]
    fn t063_unknown_encoder_from_tablet_is_rejected() {
        let (tx, rx) = watch::channel(settings("libx264"));
        let (mode_tx, _mode_rx) = watch::channel(false);
        handle_event(
            InputEvent::Config {
                bitrate: None,
                fps: None,
                encoder: Some("unknown".into()),
            },
            &Arc::new(std::sync::Mutex::new(InjectDevices::empty())),
            &SessionSettings::new(&Some(tx), &mode_tx, true),
            &crate::latency::LatencyTracker::new(),
            true,
        );
        assert_eq!(rx.borrow().encoder, "libx264");
    }

    #[tokio::test]
    async fn t062_greeting_advertises_disabled_input_devices() {
        let (mut client, _tx, task) = connection("h264_nvenc").await;
        let greeting = response(&mut client).await;
        client.close(None).await.unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(greeting["touch"], false);
        assert_eq!(greeting["pen"], false);
    }
}
