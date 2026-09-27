//! Retain route ownership when an asynchronous mutation is cancelled or fails.
use super::*;
use blent::usb::{Adb, NativeCommands, Routes};

#[derive(Clone, Default)]
pub(crate) struct RouteOwner(Arc<tokio::sync::Mutex<Option<Routes>>>);

impl RouteOwner {
    pub(crate) async fn prepare(&self, serial: &str, ports: (u16, u16), adb: &str) -> Result<()> {
        let mut owned = self.0.lock().await;
        if owned.is_none() {
            *owned = Some(Routes::for_transport(serial, ports)?);
        }
        owned
            .as_mut()
            .unwrap()
            .prepare(&Adb(NativeCommands(adb.into())))
            .await
    }

    pub(super) async fn retire(&self, adb: &str) -> Result<()> {
        let mut owned = self.0.lock().await;
        if let Some(routes) = owned.as_mut() {
            routes.retire(&Adb(NativeCommands(adb.into()))).await?;
        }
        Ok(())
    }
}
