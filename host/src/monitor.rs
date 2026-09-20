//! T390: independently progressing discovery, per-device mutations and teardown.
use crate::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
mod credentials;
mod forwarding;
mod launch_policy;
mod preparation;
mod retirement;

pub(crate) struct Config {
    pub ports: (u16, u16),
    pub auto_launch: bool,
    pub tablet: attachment::Attachment,
    pub token_dir: Option<PathBuf>,
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
    Token,
    Forwarding,
}

struct Inventory {
    devices: Option<Vec<String>>,
    synthetic: Vec<String>,
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
    Retired(Option<std::result::Result<(String, u32), tokio::task::JoinError>>),
    PrimaryToken,
    ExtraToken(String),
}

struct Monitor {
    config: Config,
    current: Option<String>,
    ready: HashSet<String>,
    attachments: HashMap<String, attachment::Attachment>,
    identities: HashMap<String, String>,
    forwarding: HashMap<String, RelaunchBackoff>,
    token_retries: HashMap<String, RelaunchBackoff>,
    launches: launch_policy::Launches,
    extras: HashMap<String, ExtraSession>,
    pending: HashMap<String, Pending>,
    retiring_slots: HashSet<u32>,
    retiring: JoinSet<(String, u32)>,
    retiring_routes: HashMap<String, u32>,
    discovery: discovery::Discovery,
    mutations: device_tasks::DeviceTasks<Mutation>,
    inventory: JoinSet<Inventory>,
    checked_adb: bool,
    last_reconnect: Instant,
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
            attachments: HashMap::new(),
            identities: HashMap::new(),
            forwarding: HashMap::new(),
            token_retries: HashMap::new(),
            launches: Default::default(),
            extras: HashMap::new(),
            pending: HashMap::new(),
            retiring_slots: HashSet::new(),
            retiring: JoinSet::new(),
            retiring_routes: HashMap::new(),
            discovery: discovery::Discovery::new(),
            mutations: device_tasks::DeviceTasks::new(4),
            inventory: JoinSet::new(),
            checked_adb: false,
            last_reconnect: now,
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
        let recover = self.last_reconnect.elapsed() >= Duration::from_secs(10);
        self.queue_inventory(recover);
        if recover {
            self.last_reconnect = Instant::now();
            self.check_forwarding();
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
            let devices = adb_inventory::query(&adb).await;
            let mut synthetic = Vec::new();
            add_fake_tablets(&mut synthetic);
            let reconnected = if let Some(path) = path.ok().filter(|_| reconnect) {
                WifiReconnect::new(path, adb).connect().await
            } else {
                None
            };
            Inventory {
                devices,
                synthetic,
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
        let Some(devices) = adb_inventory::with_synthetic(
            result.devices,
            self.discovery.inventory(),
            result.synthetic,
        ) else {
            return;
        };
        self.launches.refresh(&devices);
        self.identities
            .retain(|serial, _| devices.contains(serial) || self.current.as_ref() == Some(serial));
        self.discovery.refresh(devices, &self.config.adb);
    }

    fn probe_ready(&mut self, serial: String, identity: Option<String>) {
        self.launches.observe(&serial, identity.as_deref());
        let changed = self.identities.get(&serial) != identity.as_ref();
        if let Some(identity) = identity {
            self.identities.insert(serial.clone(), identity);
        } else {
            self.identities.remove(&serial);
        }
        if changed {
            self.revalidate_identity(&serial);
        }
    }

    fn revalidate_identity(&mut self, serial: &str) {
        if !self.attachments.contains_key(serial) {
            return;
        }
        if self.current.as_deref() == Some(serial) {
            self.config.tablet.begin_with_transport(
                Some(attachment_identity(serial, &self.identities)),
                Some(uscreen_config::adb::transport_of(serial)),
            );
        }
        self.remove_assignment(serial);
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
        !self.retiring_routes.contains_key(serial)
            && !self.ready.contains(serial)
            && !self.mutations.contains(serial)
            && self
                .forwarding
                .get(serial)
                .is_none_or(|retry| retry.ready(Instant::now()))
    }

    fn prepare(&mut self, serial: String, pending: Pending, ports: (u16, u16)) {
        let tablet = pending
            .session
            .as_ref()
            .map_or(&self.config.tablet, |session| &session.tablet_tx);
        self.attachments.insert(serial.clone(), tablet.clone());
        let work = preparation::Preparation {
            serial: serial.clone(),
            ports,
            token: tablet.token(),
            token_dir: self.config.token_dir.clone(),
            instance: pending.instance,
            adb: self.config.adb.clone(),
            auto_launch: self.config.auto_launch && !is_fake_serial(&serial),
            ticket: self
                .launches
                .observe(&serial, self.identities.get(&serial).map(String::as_str)),
            retry: self.forwarding.remove(&serial).unwrap_or_default(),
        };
        self.pending.insert(serial.clone(), pending);
        self.mutations.schedule(serial, work.run());
    }

    fn prepare_primary(&mut self) {
        if self.retiring_slots.contains(&0) {
            return;
        }
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
        if self.discovery.initial_probes_finished() {
            self.launches.retain(self.discovery.inventory());
        }
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
            self.retire(serial, pending.instance, pending.session, None);
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
        self.token_retries
            .insert(serial, RelaunchBackoff::default());
    }

    fn mutation_ready(
        &mut self,
        serial: String,
        result: std::result::Result<Mutation, tokio::task::JoinError>,
    ) {
        match result {
            Ok(Mutation::Prepared { ready, retry }) => self.prepared(serial, ready, retry),
            Ok(_) => {}
            Err(error) => {
                if !error.is_cancelled() {
                    warn!("Device operation failed for {serial}: {error}");
                }
                if let Some(pending) = self.pending.remove(&serial) {
                    self.retire(serial, pending.instance, pending.session, None);
                }
            }
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
        let Some(Ok(token)) = self
            .attachments
            .get(&serial)
            .map(attachment::Attachment::token)
        else {
            return;
        };
        let job_serial = serial.clone();
        self.mutations.schedule(serial, async move {
            redeliver_token_using(&job_serial, token.as_deref(), &adb).await;
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

    fn extra_token(&mut self, serial: String, now: Instant) {
        if self.mutations.contains(&serial) {
            return;
        }
        if self
            .token_retries
            .get_mut(&serial)
            .is_some_and(|policy| policy.allow(now))
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
        let serials: HashSet<_> = self
            .current
            .iter()
            .chain(self.pending.keys())
            .chain(self.extras.keys())
            .cloned()
            .collect();
        for serial in serials {
            self.remove_assignment(&serial);
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
        Event::Retired(Some(Ok(route))) => state.retired(route),
        Event::PrimaryToken => state.primary_token(),
        Event::ExtraToken(serial) => state.extra_token(serial, Instant::now()),
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

    pub(super) fn fixture() -> Monitor {
        let (tablet, extra, _stop) = crate::discovery_tests::monitor_inputs(2, (18000, 18001));
        Monitor::new(Config {
            ports: (18000, 18001),
            auto_launch: false,
            tablet,
            token_dir: None,
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
        state.retired(retired);
        assert_eq!(state.available_slot(), Some(1));
        state.stop().await;
    }
}

#[cfg(test)]
#[path = "monitor_test_support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "monitor/probe_tests.rs"]
mod probe_tests;

#[cfg(test)]
#[path = "monitor/inventory_tests.rs"]
mod inventory_tests;

#[cfg(test)]
#[path = "monitor/launch_tests.rs"]
mod launch_tests;

#[cfg(test)]
mod credential_tests;

#[cfg(test)]
mod coverage_tests;
