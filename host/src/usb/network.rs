use super::*;
use blent_config::storage::ConfigStore;
use std::collections::HashMap;
use tokio::time::Instant;

pub(super) struct Network {
    store: ConfigStore,
    identities: HashMap<String, String>,
    launches: crate::launch_policy::Launches,
    next_retry: Instant,
}
impl Network {
    pub fn new(store: ConfigStore) -> Self {
        Self {
            store,
            identities: HashMap::new(),
            launches: Default::default(),
            next_retry: Instant::now(),
        }
    }
    pub fn identity(&self, serial: &str) -> Option<&String> {
        self.identities.get(serial)
    }
    pub fn attachment_identity(&self, serial: &str) -> String {
        crate::transport::attachment_identity(serial, &self.identities)
    }
    pub fn ticket(&self, serial: &str) -> Option<crate::launch_policy::Ticket> {
        self.launches
            .observe(serial, self.identities.get(serial).map(String::as_str))
    }
    pub fn retarget(&self, routes: &mut Routes, identity: Option<&String>) -> bool {
        let Some(identity) = identity else {
            return true;
        };
        if let Some((serial, _)) = self
            .identities
            .iter()
            .filter(|(_, id)| *id == identity)
            .min_by_key(|(serial, _)| (transport_of(serial) != Transport::Usb, *serial))
        {
            return routes.retarget(serial).is_ok();
        }
        false
    }
    pub async fn can_retire<C: Commands>(
        &self,
        adb: &Adb<C>,
        routes: &mut Routes,
        identity: Option<&String>,
    ) -> bool {
        if self.retarget(routes, identity) {
            return true;
        }
        if self.identities.contains_key(routes.serial()) {
            return false;
        }
        // Inventory absence alone is not proof that a still-reachable route is
        // foreign. Recheck physical identity before touching its owned mapping.
        adb.identity(routes.serial()).await.as_ref() == identity
    }
    pub async fn devices<C: Commands>(
        &mut self,
        adb: &Adb<C>,
        current: &[String],
    ) -> Option<Vec<String>> {
        let mut devices = adb.all_devices().await?;
        if devices.len() > 256 {
            return None;
        }
        let address = self.store.load().wifi_address;
        if !current.iter().any(|s| devices.contains(s))
            && !devices.contains(&address)
            && Instant::now() >= self.next_retry
        {
            self.next_retry = Instant::now() + Duration::from_secs(5);
            if crate::wifi::reconnect(adb, &self.store).await.is_some() {
                devices = adb.all_devices().await?;
            }
        }
        if devices.len() > 256 {
            return None;
        }
        devices.sort_by_key(|serial| {
            (
                transport_of(serial) != Transport::Usb,
                !current.contains(serial),
            )
        });
        self.observe(adb, &devices).await;
        Some(crate::transport::select_device_transports(
            &devices,
            None,
            &self.identities,
        ))
    }
    async fn observe<C: Commands>(&mut self, adb: &Adb<C>, devices: &[String]) {
        self.launches.refresh(devices);
        for serial in devices {
            if let Some(identity) = adb.identity(serial).await {
                self.identities.insert(serial.clone(), identity);
            }
            self.launches
                .observe(serial, self.identities.get(serial).map(String::as_str));
        }
        self.identities.retain(|s, _| devices.contains(s));
        self.launches.retain(devices);
    }
}
