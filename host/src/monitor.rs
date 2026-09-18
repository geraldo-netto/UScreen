//! T390: independently progressing discovery, per-device mutations and teardown.
use crate::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

pub(crate) struct Config {
    pub ports: (u16, u16),
    pub auto_launch: bool,
    pub tablet: attachment::Attachment,
    pub token: Option<String>,
    pub relaunch: Arc<tokio::sync::Notify>,
    pub extra: ExtraSessionTemplate,
    pub adb: String,
}

struct Pending {
    instance: u32,
    session: Option<ExtraSession>,
}

enum Mutation {
    Prepared { ready: bool, retry: RelaunchBackoff },
    Recovered(RelaunchBackoff),
    Token,
}

struct Inventory {
    devices: Vec<String>,
    reconnected: Option<String>,
}

enum Event {
    Stop,
    Tick,
    Probe(Option<(String, Option<String>)>),
    Mutation(
        String,
        std::result::Result<Mutation, tokio::task::JoinError>,
    ),
    Inventory(Option<std::result::Result<Inventory, tokio::task::JoinError>>),
    Retired(Option<std::result::Result<u32, tokio::task::JoinError>>),
    PrimaryToken,
    ExtraToken(String),
}

struct Monitor {
    config: Config,
    current: Option<String>,
    ready: HashSet<String>,
    identities: HashMap<String, String>,
    forwarding: HashMap<String, RelaunchBackoff>,
    recovery: HashMap<String, RelaunchBackoff>,
    extras: HashMap<String, ExtraSession>,
    pending: HashMap<String, Pending>,
    retiring_slots: HashSet<u32>,
    retiring: JoinSet<u32>,
    discovery: discovery::Discovery,
    mutations: device_tasks::DeviceTasks<Mutation>,
    inventory: JoinSet<Inventory>,
    checked_adb: bool,
    last_recovery: Instant,
    last_relaunch: Instant,
    relaunch_wait: Duration,
    relaunches: u32,
    wifi_announced: bool,
}

impl Monitor {
    fn new(config: Config) -> Self {
        let now = Instant::now();
        Self {
            config,
            current: None,
            ready: HashSet::new(),
            identities: HashMap::new(),
            forwarding: HashMap::new(),
            recovery: HashMap::new(),
            extras: HashMap::new(),
            pending: HashMap::new(),
            retiring_slots: HashSet::new(),
            retiring: JoinSet::new(),
            discovery: discovery::Discovery::new(),
            mutations: device_tasks::DeviceTasks::new(4),
            inventory: JoinSet::new(),
            checked_adb: false,
            last_recovery: now,
            last_relaunch: now - Duration::from_secs(60),
            relaunch_wait: Duration::from_secs(5),
            relaunches: 0,
            wifi_announced: false,
        }
    }

    async fn event(
        &mut self,
        interval: &mut tokio::time::Interval,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Event {
        let requests = self
            .extras
            .iter()
            .map(|(serial, session)| (serial.clone(), session.relaunch.clone()))
            .collect();
        tokio::select! {
            _ = shutdown.changed() => Event::Stop,
            _ = interval.tick() => Event::Tick,
            result = self.discovery.next() => Event::Probe(result),
            (serial, result) = self.mutations.next() => Event::Mutation(serial, result),
            result = self.inventory.join_next(), if !self.inventory.is_empty() => Event::Inventory(result),
            result = self.retiring.join_next(), if !self.retiring.is_empty() => Event::Retired(result),
            _ = self.config.relaunch.notified() => Event::PrimaryToken,
            serial = wait_extra_relaunch(requests) => Event::ExtraToken(serial),
        }
    }

    fn tick(&mut self) {
        let recover = self.last_recovery.elapsed() >= Duration::from_secs(10);
        self.queue_inventory(recover);
        if recover {
            self.last_recovery = Instant::now();
            self.recover_apps();
        }
    }

    fn queue_inventory(&mut self, recover: bool) {
        if !self.inventory.is_empty() {
            return;
        }
        let adb = self.config.adb.clone();
        let check_version = !self.checked_adb;
        self.checked_adb = true;
        let reconnect = recover
            && self
                .current
                .as_ref()
                .is_none_or(|serial| !self.ready.contains(serial));
        let path = config::config_path();
        // One global ADB job: list/connect/disconnect never overlap each other.
        self.inventory.spawn(async move {
            if check_version
                && tokio::process::Command::new(&adb)
                    .arg("version")
                    .output_bounded()
                    .await
                    .is_err()
            {
                error!("adb is not installed; install android-tools (or adb) and restart");
            }
            let mut devices = adb_devices_using(&adb).await;
            add_fake_tablets(&mut devices);
            let reconnected = if reconnect {
                WifiReconnect::new(path, adb).connect().await
            } else {
                None
            };
            Inventory {
                devices,
                reconnected,
            }
        });
    }

    fn inventory_ready(&mut self, result: Inventory) {
        if let Some(address) = result.reconnected {
            if !self.wifi_announced {
                info!("Reconnected to the tablet over Wi-Fi ({address})");
                self.wifi_announced = true;
            }
        }
        self.identities.retain(|serial, _| {
            result.devices.contains(serial) || self.current.as_ref() == Some(serial)
        });
        self.discovery.refresh(result.devices, &self.config.adb);
    }

    fn probe_ready(&mut self, serial: String, identity: Option<String>) {
        if let Some(identity) = identity {
            self.identities.insert(serial, identity);
        } else {
            self.identities.remove(&serial);
        }
    }

    fn retire(&mut self, session: ExtraSession) {
        let instance = session.instance;
        self.retiring_slots.insert(instance);
        self.retiring.spawn(async move {
            session.stop().await;
            instance
        });
    }

    fn remove_assignment(&mut self, serial: &str) {
        self.ready.remove(serial);
        self.mutations.cancel(serial);
        self.recovery.remove(serial);
        if let Some(pending) = self.pending.remove(serial) {
            if let Some(session) = pending.session {
                self.retire(session);
            }
        }
        if let Some(session) = self.extras.remove(serial) {
            self.retire(session);
        }
    }

    fn change_primary(&mut self, found: &Option<String>) {
        if self.current == *found {
            return;
        }
        // Invalidate before a forwarding operation can deliver new metadata.
        self.config.tablet.begin_with_transport(
            found
                .as_ref()
                .map(|serial| attachment_identity(serial, &self.identities)),
            found.as_deref().map(uscreen_config::adb::transport_of),
        );
        let old = self.current.clone();
        disconnected_primary(&mut self.current, &mut self.wifi_announced);
        if let Some(old) = old {
            self.remove_assignment(&old);
        }
        // A surviving extra may become primary. Retire its previous slot before
        // reusing the serial's forwarding owner; ready state belongs to a slot.
        if let Some(serial) = found {
            self.remove_assignment(serial);
        }
        self.current = found.clone();
    }

    fn can_prepare(&self, serial: &str) -> bool {
        !self.ready.contains(serial)
            && !self.mutations.contains(serial)
            && self
                .forwarding
                .get(serial)
                .is_none_or(|retry| retry.ready(Instant::now()))
    }

    fn prepare(&mut self, serial: String, pending: Pending, ports: (u16, u16)) {
        let mut retry = self.forwarding.remove(&serial).unwrap_or_default();
        let adb = self.config.adb.clone();
        let token = self.config.token.clone();
        let auto_launch = self.config.auto_launch;
        self.pending.insert(serial.clone(), pending);
        let job_serial = serial.clone();
        self.mutations.schedule(serial, async move {
            let request = TabletConnection {
                serial: &job_serial,
                video_port: ports.0,
                input_port: ports.1,
                auto_launch,
                token: token.as_deref(),
                adb: &adb,
            };
            let ready = request.prepare(&mut retry, Instant::now()).await;
            Mutation::Prepared { ready, retry }
        });
    }

    fn prepare_primary(&mut self) {
        let Some(serial) = self
            .current
            .clone()
            .filter(|serial| self.can_prepare(serial))
        else {
            return;
        };
        self.config.tablet.begin_with_transport(
            Some(attachment_identity(&serial, &self.identities)),
            Some(uscreen_config::adb::transport_of(&serial)),
        );
        self.prepare(
            serial,
            Pending {
                instance: 0,
                session: None,
            },
            self.config.ports,
        );
    }

    fn available_slot(&self) -> Option<u32> {
        let used: HashSet<_> = self
            .extras
            .values()
            .map(|session| session.instance)
            .chain(self.pending.values().map(|pending| pending.instance))
            .chain(self.retiring_slots.iter().copied())
            .collect();
        (1..self.config.extra.max_tablets).find(|slot| !used.contains(slot))
    }

    async fn prepare_extra(&mut self, serial: String) {
        if !self.can_prepare(&serial) {
            return;
        }
        let Some(instance) = self.available_slot() else {
            return;
        };
        // Only local listener binding awaits here. Device commands run in the
        // owned mutation job, while this owner retains the unactivated runtime.
        let session = match spawn_extra_session(&self.config.extra, instance).await {
            Ok(session) => session,
            Err(error) => {
                warn!("Could not start tablet {}: {error}", instance + 1);
                self.forwarding
                    .entry(serial)
                    .or_default()
                    .allow(Instant::now());
                return;
            }
        };
        session.tablet_tx.begin_with_transport(
            Some(attachment_identity(&serial, &self.identities)),
            Some(uscreen_config::adb::transport_of(&serial)),
        );
        let ports = (session.video_port, session.input_port);
        self.prepare(
            serial,
            Pending {
                instance,
                session: Some(session),
            },
            ports,
        );
    }

    async fn reconcile(&mut self) {
        let devices = select_device_transports(
            &self.discovery.eligible(),
            self.current.as_deref(),
            &self.identities,
        );
        let preferred = current_transport(&devices, self.current.as_deref(), &self.identities);
        let found = select_tablet(&devices, preferred.as_deref());
        self.change_primary(&found);
        let others = extra_devices(&devices, found.as_deref());
        self.remove_gone(&others);
        self.forwarding
            .retain(|serial, _| self.discovery.present(serial));
        self.prepare_primary();
        for serial in others {
            self.prepare_extra(serial).await;
        }
    }

    fn remove_gone(&mut self, others: &[String]) {
        let gone: Vec<_> = self
            .extras
            .keys()
            .chain(self.pending.keys())
            .filter(|serial| self.current.as_ref() != Some(*serial) && !others.contains(*serial))
            .cloned()
            .collect();
        for serial in gone {
            self.remove_assignment(&serial);
        }
    }

    fn prepared(&mut self, serial: String, ready: bool, retry: RelaunchBackoff) {
        let Some(pending) = self.pending.remove(&serial) else {
            return;
        };
        self.forwarding.insert(serial.clone(), retry);
        if !ready {
            if let Some(session) = pending.session {
                self.retire(session);
            }
            return;
        }
        if let Some(session) = pending.session {
            let _ = session.tablet_tx.send(true);
            self.extras.insert(serial.clone(), session);
        } else {
            let _ = self.config.tablet.send(true);
            self.relaunches = 0;
            self.relaunch_wait = Duration::from_secs(5);
        }
        announce_transport(&serial);
        info!(
            "Tablet {} connected over {} ({serial})",
            pending.instance + 1,
            transport_of(&serial).label()
        );
        self.ready.insert(serial.clone());
        self.recovery.insert(serial, RelaunchBackoff::default());
    }

    fn mutation_ready(
        &mut self,
        serial: String,
        result: std::result::Result<Mutation, tokio::task::JoinError>,
    ) {
        match result {
            Ok(Mutation::Prepared { ready, retry }) => self.prepared(serial, ready, retry),
            Ok(Mutation::Recovered(retry)) if self.ready.contains(&serial) => {
                self.recovery.insert(serial, retry);
            }
            Ok(_) => {}
            Err(error) => {
                if !error.is_cancelled() {
                    warn!("Device operation failed for {serial}: {error}");
                }
                if let Some(pending) = self.pending.remove(&serial) {
                    if let Some(session) = pending.session {
                        self.retire(session);
                    }
                }
            }
        }
    }

    fn recover_apps(&mut self) {
        if !self.config.auto_launch {
            return;
        }
        let assigned: Vec<_> = self
            .ready
            .iter()
            .filter(|serial| !is_fake_serial(serial) && !self.mutations.contains(serial))
            .cloned()
            .collect();
        for serial in assigned {
            let policy = self.recovery.remove(&serial).unwrap_or_default();
            let adb = self.config.adb.clone();
            let token = self.config.token.clone();
            let job_serial = serial.clone();
            self.mutations.schedule(serial, async move {
                Mutation::Recovered(
                    recover_app(job_serial, policy, token.as_deref(), Instant::now(), &adb)
                        .await
                        .1,
                )
            });
        }
    }

    fn redeliver(&mut self, serial: String) {
        if !self.ready.contains(&serial)
            || self.mutations.contains(&serial)
            || is_fake_serial(&serial)
        {
            return;
        }
        let adb = self.config.adb.clone();
        let token = self.config.token.clone();
        let job_serial = serial.clone();
        self.mutations.schedule(serial, async move {
            launch_app_using(&job_serial, token.as_deref(), &adb).await;
            Mutation::Token
        });
    }

    fn primary_token(&mut self) {
        let Some(serial) = self.current.clone() else {
            return;
        };
        if self.mutations.contains(&serial) || self.last_relaunch.elapsed() < self.relaunch_wait {
            return;
        }
        self.last_relaunch = Instant::now();
        self.relaunches += 1;
        if self.relaunches == 3 {
            warn!("Repeated missing-token connections; update the Android app. Token retries back off to ten minutes.");
        }
        self.relaunch_wait = (self.relaunch_wait * 2).min(Duration::from_secs(600));
        self.redeliver(serial);
    }

    fn extra_token(&mut self, serial: String) {
        if self.mutations.contains(&serial) {
            return;
        }
        if self
            .recovery
            .get_mut(&serial)
            .is_some_and(|policy| policy.allow(Instant::now()))
        {
            self.redeliver(serial);
        }
    }

    fn publish(&self, ledger: &mut Option<runtime::SessionLedger>) {
        let Some(ledger) = ledger else {
            return;
        };
        let mut sessions: Vec<_> = self
            .current
            .iter()
            .filter(|serial| self.ready.contains(*serial))
            .map(|serial| runtime::TabletSession {
                serial: serial.clone(),
                instance: 0,
                video_port: self.config.ports.0,
                input_port: self.config.ports.1,
            })
            .collect();
        sessions.extend(
            self.extras
                .iter()
                .map(|(serial, session)| runtime::TabletSession {
                    serial: serial.clone(),
                    instance: session.instance,
                    video_port: session.video_port,
                    input_port: session.input_port,
                }),
        );
        if let Err(error) = ledger.update(sessions) {
            warn!("Could not update tablet sessions: {error}");
        }
    }

    async fn stop(mut self) {
        let _ = self.config.tablet.send(false);
        self.inventory.abort_all();
        while self.inventory.join_next().await.is_some() {}
        self.discovery.stop().await;
        self.mutations.stop().await;
        let pending = std::mem::take(&mut self.pending);
        for (_, pending) in pending {
            if let Some(session) = pending.session {
                self.retire(session);
            }
        }
        let extras = std::mem::take(&mut self.extras);
        for (_, session) in extras {
            self.retire(session);
        }
        while self.retiring.join_next().await.is_some() {}
    }
}

async fn apply_event(state: &mut Monitor, event: Event) {
    match event {
        Event::Tick => state.tick(),
        Event::Probe(Some((serial, identity))) => state.probe_ready(serial, identity),
        Event::Mutation(serial, result) => state.mutation_ready(serial, result),
        Event::Inventory(Some(Ok(result))) => state.inventory_ready(result),
        Event::Retired(Some(Ok(slot))) => {
            state.retiring_slots.remove(&slot);
        }
        Event::PrimaryToken => state.primary_token(),
        Event::ExtraToken(serial) => state.extra_token(serial),
        _ => {}
    }
}

pub(crate) async fn run(config: Config) {
    let mut shutdown = config.extra.shutdown_rx.clone();
    let mut state = Monitor::new(config);
    let mut ledger = session_ledger();
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    while !*shutdown.borrow() {
        let event = state.event(&mut interval, &mut shutdown).await;
        if matches!(event, Event::Stop) {
            break;
        }
        apply_event(&mut state, event).await;
        state.reconcile().await;
        state.publish(&mut ledger);
    }
    state.stop().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn t388_route_follows_adb_transport_not_physical_identity() {
        let mut state = fixture();
        let mut previous = None;
        for (serial, expected) in [
            ("8002RH1010011900", "usb"),
            ("[::1]:5555", "network"),
            ("tablet._adb-tls-connect._tcp.local.", "network"),
            ("8002RH1010011900", "usb"),
        ] {
            state
                .identities
                .insert(serial.into(), "same-physical-tablet".into());
            state.change_primary(&Some(serial.into()));
            if let Some(lease) = previous.take() {
                let lease: attachment::Lease = lease;
                assert!(!lease.apply(|| panic!("T388: retired route used")));
            }
            let lease = state.config.tablet.lease();
            assert_eq!(lease.transport(), Some(expected));
            previous = Some(lease);
            state.prepare_primary();
            assert_eq!(state.config.tablet.lease().transport(), Some(expected));
        }
        state.change_primary(&None);
        assert_eq!(state.config.tablet.lease().transport(), None);
        state.stop().await;
    }

    fn fixture() -> Monitor {
        let (tablet, extra, _stop) = crate::discovery_tests::monitor_inputs(2, (18000, 18001));
        Monitor::new(Config {
            ports: (18000, 18001),
            auto_launch: false,
            tablet,
            token: None,
            relaunch: Default::default(),
            extra,
            adb: "/missing-test-adb".into(),
        })
    }

    #[tokio::test]
    async fn t390_promoting_an_extra_retires_its_previous_slot() {
        let mut state = fixture();
        let runtime = session::Spec {
            capture: capture::CaptureConfig {
                instance: 1,
                helper_path: "/missing-t390-helper".into(),
                ..Default::default()
            },
            ports: (0, 0),
            token: None,
            devices: (false, false, false),
        }
        .prepare(state.config.extra.mode_tx.clone())
        .start(state.config.extra.shutdown_rx.clone())
        .await
        .unwrap();
        state.current = Some("OLD".into());
        state.ready.insert("SURVIVOR".into());
        state.extras.insert("SURVIVOR".into(), runtime);
        state.change_primary(&Some("SURVIVOR".into()));
        assert!(
            !state.extras.contains_key("SURVIVOR"),
            "T390: promoted tablet still owns its old extra slot"
        );
        assert!(
            !state.ready.contains("SURVIVOR"),
            "T390: old slot readiness leaked into primary"
        );
        assert!(state.retiring_slots.contains(&1));
        assert_eq!(
            state.available_slot(),
            None,
            "T390: reused ports before prior owner joined"
        );
        let retired = state.retiring.join_next().await.unwrap().unwrap();
        state.retiring_slots.remove(&retired);
        assert_eq!(state.available_slot(), Some(1));
        state.stop().await;
    }
}
