//! Portable discovery/slot ownership; native executable and process APIs are injected.
use super::{connection::Connection, *};
use crate::session::Runtime;
use blent_config::{tablets::TabletSession, FileConfig};
use tokio::{sync::watch, time::Instant};

struct Active {
    connection: Connection,
    runtime: Runtime,
    delivery: Instant,
    identity: Option<String>,
}
pub struct Monitor<C> {
    adb: Adb<C>,
    config: FileConfig,
    ports: Vec<(u16, u16)>,
    slots: Vec<Option<Active>>,
    stop: watch::Receiver<bool>,
    failed: std::collections::HashMap<String, Instant>,
    pending: Vec<(Routes, Option<String>)>,
    network: Option<super::network::Network>,
    store: blent_config::storage::ConfigStore,
}
impl<C: Commands> Monitor<C> {
    pub fn new(adb: Adb<C>, config: FileConfig, stop: watch::Receiver<bool>) -> Result<Self> {
        ensure!(
            config.require_token,
            "Windows USB preview requires require_token = true"
        );
        let ports =
            blent_config::slot_ports(config.video_port, config.input_port, config.max_tablets)?;
        let slots = (0..ports.len()).map(|_| None).collect();
        Ok(Self {
            adb,
            config,
            ports,
            slots,
            stop,
            failed: Default::default(),
            pending: Vec::new(),
            network: None,
            store: Default::default(),
        })
    }
    pub fn with_network(mut self, store: blent_config::storage::ConfigStore) -> Self {
        self.store = store.clone();
        self.network = Some(super::network::Network::new(store));
        self
    }
    pub fn sessions(&self) -> Vec<TabletSession> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let active = slot.as_ref()?;
                Some(TabletSession {
                    serial: active.connection.serial()?.into(),
                    instance: index as u32,
                    video_port: self.ports[index].0,
                    input_port: self.ports[index].1,
                })
            })
            .collect()
    }
    /// Unknown inventory is not a disconnect. One broken device cannot stop the others.
    pub async fn poll(&mut self) {
        let observed = if let Some(network) = self.network.as_mut() {
            tokio::time::timeout(
                Duration::from_secs(6),
                network.devices(
                    &self.adb,
                    &self
                        .slots
                        .iter()
                        .flatten()
                        .filter_map(|a| a.connection.serial().map(str::to_string))
                        .collect::<Vec<_>>(),
                ),
            )
            .await
            .unwrap_or(None)
        } else {
            self.adb.inventory().await
        };
        let Some(devices) = observed else {
            return;
        };
        self.failed.retain(|serial, _| devices.contains(serial));
        self.retry_routes(Some(&devices)).await;
        for index in 0..self.slots.len() {
            if *self.stop.borrow() {
                return;
            }
            if let Err(error) = self.update(index, &devices).await {
                tracing::warn!("USB slot {index}: {error:#}");
            }
        }
    }
    async fn update(&mut self, index: usize, devices: &[String]) -> Result<()> {
        if let Some(active) = self.slots[index].as_ref() {
            let present = active
                .connection
                .serial()
                .is_some_and(|serial| devices.iter().any(|device| device == serial));
            let identity_current = self.network.as_ref().is_none_or(|network| {
                active.connection.serial().and_then(|s| network.identity(s))
                    == active.identity.as_ref()
            });
            if present && identity_current {
                return self.maintain(index).await;
            }
            self.retire(index).await;
        }
        if let Some(serial) = self.candidate(devices).await {
            self.attach(index, &serial).await?;
        }
        Ok(())
    }
    async fn candidate(&self, devices: &[String]) -> Option<String> {
        for serial in devices {
            if *self.stop.borrow() {
                return None;
            }
            if self.available(serial) && self.adb.installed(serial).await == Some(true) {
                return Some(serial.clone());
            }
        }
        None
    }
    fn available(&self, serial: &str) -> bool {
        !self
            .failed
            .get(serial)
            .is_some_and(|until| Instant::now() < *until)
            && !self.assigned(serial)
            && !self
                .pending
                .iter()
                .any(|(routes, _)| routes.serial() == serial)
    }
    fn assigned(&self, serial: &str) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|active| active.connection.selected() == Some(serial))
    }
    async fn attach(&mut self, index: usize, serial: &str) -> Result<()> {
        let runtime =
            super::preview::prepare(&self.config, index as u32, self.ports[index], &self.store)?
                .start(self.stop.clone())
                .await?;
        let connection = Connection::new(
            runtime.tablet_tx.clone(),
            self.ports[index],
            self.config.auto_launch_app,
        )?;
        self.slots[index] = Some(Active {
            connection,
            runtime,
            delivery: Instant::now() + Duration::from_secs(5),
            identity: self
                .network
                .as_ref()
                .and_then(|n| n.identity(serial))
                .cloned(),
        });
        let connection = &mut self.slots[index].as_mut().unwrap().connection;
        let result = if let Some(network) = self.network.as_ref() {
            connection
                .connect_network(
                    &self.adb,
                    serial,
                    network.attachment_identity(serial),
                    network.ticket(serial),
                )
                .await
        } else {
            connection.connect(&self.adb, serial).await
        };
        if result.is_err() {
            self.failed
                .insert(serial.into(), Instant::now() + Duration::from_secs(5));
        }
        result
    }
    async fn maintain(&mut self, index: usize) -> Result<()> {
        let active = self.slots[index].as_mut().unwrap();
        if let Err(error) = active.connection.refresh(&self.adb).await {
            let _ = active.connection.disconnect(&self.adb).await;
            return Err(error);
        }
        if Instant::now() >= active.delivery {
            active.delivery = Instant::now() + Duration::from_secs(5);
            active.connection.redeliver(&self.adb).await?;
        }
        Ok(())
    }
    async fn retire(&mut self, index: usize) {
        if let Some(mut active) = self.slots[index].take() {
            let routes = active.connection.release_routes();
            active.runtime.stop().await;
            if let Some(mut routes) = routes {
                let safe = match self.network.as_ref() {
                    Some(network) => {
                        network
                            .can_retire(&self.adb, &mut routes, active.identity.as_ref())
                            .await
                    }
                    None => true,
                };
                if !safe || routes.retire(&self.adb).await.is_err() {
                    self.pending.push((routes, active.identity));
                }
            }
        }
    }

    async fn retry_routes(&mut self, present: Option<&[String]>) {
        let mut index = 0;
        while index < self.pending.len() {
            let (routes, identity) = &mut self.pending[index];
            let safe = match self.network.as_ref() {
                Some(network) => {
                    network
                        .can_retire(&self.adb, routes, identity.as_ref())
                        .await
                }
                None => true,
            };
            let available =
                present.is_none_or(|devices| devices.iter().any(|s| s == routes.serial()));
            if safe && available && routes.retire(&self.adb).await.is_ok() {
                self.pending.swap_remove(index);
            } else {
                index += 1;
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.retry_routes(None).await;
        for index in 0..self.slots.len() {
            self.retire(index).await;
        }
        ensure!(
            self.pending.is_empty(),
            "USB cleanup incomplete: {} device route owners remain",
            self.pending.len()
        );
        Ok(())
    }
}
