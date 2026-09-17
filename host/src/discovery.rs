//! T390: incremental per-transport identity/package observations.
//! Results publish independently; failed or slow checks do not form a barrier.
use crate::device_tasks::DeviceTasks;
use std::collections::HashMap;

struct Observation {
    epoch: u64,
    identity: Option<String>,
    eligible: bool,
}

struct Probe {
    epoch: u64,
    identity: Option<String>,
    eligible: bool,
}

pub(crate) struct Discovery {
    inventory: Vec<String>,
    observed: HashMap<String, Observation>,
    work: DeviceTasks<Probe>,
    next_epoch: u64,
}

impl Discovery {
    pub fn new() -> Self {
        Self {
            inventory: Vec::new(),
            observed: HashMap::new(),
            work: DeviceTasks::new(4),
            next_epoch: 0,
        }
    }

    pub fn refresh(&mut self, devices: Vec<String>, adb: &str) {
        for serial in &self.inventory {
            if !devices.contains(serial) {
                self.work.cancel(serial);
            }
        }
        self.observed.retain(|serial, _| devices.contains(serial));
        self.inventory = devices;
        for serial in &self.inventory {
            let observation = self.observed.entry(serial.clone()).or_insert_with(|| {
                self.next_epoch += 1;
                Observation {
                    epoch: self.next_epoch,
                    identity: None,
                    eligible: false,
                }
            });
            self.work.schedule(
                serial.clone(),
                probe(
                    serial.clone(),
                    adb.to_owned(),
                    observation.epoch,
                    observation.identity.clone(),
                ),
            );
        }
    }

    pub fn eligible(&self) -> Vec<String> {
        self.inventory
            .iter()
            .filter(|serial| {
                self.observed
                    .get(*serial)
                    .is_some_and(|state| state.eligible)
            })
            .cloned()
            .collect()
    }

    pub fn present(&self, serial: &str) -> bool {
        self.observed.contains_key(serial)
    }

    pub async fn next(&mut self) -> Option<(String, Option<String>)> {
        let (serial, result) = self.work.next().await;
        let result = result.ok()?;
        let observation = self.observed.get_mut(&serial)?;
        if observation.epoch != result.epoch {
            return None;
        }
        observation.identity = result.identity.clone();
        observation.eligible = result.eligible;
        Some((serial, result.identity))
    }

    pub async fn stop(&mut self) {
        self.work.stop().await;
    }
}

async fn probe(serial: String, adb: String, epoch: u64, known: Option<String>) -> Probe {
    let identity = match known {
        Some(identity) => Some(identity),
        None => crate::probe_device_identity(&serial, &adb)
            .await
            .map(|(_, identity)| identity),
    };
    let eligible = crate::app_installed_with(&serial, &adb).await;
    Probe {
        epoch,
        identity,
        eligible,
    }
}
