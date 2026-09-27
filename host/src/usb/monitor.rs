//! Portable discovery/slot ownership; native executable and process APIs are injected.
use super::{connection::Connection, *};
use crate::session::Runtime;
use blent_config::{tablets::TabletSession, FileConfig};
use tokio::{sync::watch, time::Instant};

struct Active {
    connection: Connection,
    runtime: Runtime,
    delivery: Instant,
}
pub struct Monitor<C> {
    adb: Adb<C>,
    config: FileConfig,
    ports: Vec<(u16, u16)>,
    slots: Vec<Option<Active>>,
    stop: watch::Receiver<bool>,
    failed: std::collections::HashMap<String, Instant>,
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
        })
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
        let Some(devices) = self.adb.inventory().await else {
            return;
        };
        self.failed.retain(|serial, _| devices.contains(serial));
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
            if present {
                return self.maintain(index).await;
            }
            self.retire(index).await?;
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
    }
    fn assigned(&self, serial: &str) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|active| active.connection.selected() == Some(serial))
    }
    async fn attach(&mut self, index: usize, serial: &str) -> Result<()> {
        let runtime = super::preview::prepare(&self.config, index as u32, self.ports[index])?
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
        });
        let result = self.slots[index]
            .as_mut()
            .unwrap()
            .connection
            .connect(&self.adb, serial)
            .await;
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
    async fn retire(&mut self, index: usize) -> Result<()> {
        if let Some(active) = self.slots[index].as_mut() {
            active.connection.disconnect(&self.adb).await?;
        }
        if let Some(active) = self.slots[index].take() {
            active.runtime.stop().await;
        }
        Ok(())
    }
    pub async fn shutdown(&mut self) -> Result<()> {
        let mut errors = Vec::new();
        for index in 0..self.slots.len() {
            if let Err(error) = self.retire(index).await {
                errors.push(format!("slot {index}: {error:#}"));
            }
        }
        ensure!(
            errors.is_empty(),
            "USB cleanup incomplete: {}",
            errors.join("; ")
        );
        Ok(())
    }
}
