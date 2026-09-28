//! T691: shared setup/reconnect policy; injected ADB and configuration adapters.
use crate::usb::{args, Adb, Commands};
use anyhow::{ensure, Context, Result};
use blent_config::{
    adb::{transport_of, Transport},
    storage::ConfigStore,
};

pub fn valid_address(address: &str) -> bool {
    if address.len() > 253 || address.chars().any(char::is_whitespace) {
        return false;
    }
    if let Ok(endpoint) = address.parse::<std::net::SocketAddr>() {
        return endpoint.port() != 0
            && !endpoint.ip().is_unspecified()
            && !endpoint.ip().is_multicast();
    }
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    valid_host(host) && valid_port(port)
}
fn valid_port(port: &str) -> bool {
    port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|port| port > 0)
}

fn valid_host(host: &str) -> bool {
    host.bytes().any(|b| b.is_ascii_alphabetic()) && host.split('.').all(valid_label)
}
fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub async fn reconnect<C: Commands>(adb: &Adb<C>, store: &ConfigStore) -> Option<String> {
    let address = store.load().wifi_address;
    if !valid_address(&address) {
        return None;
    }
    let output = adb
        .checked(vec!["connect".into(), address.clone()], None)
        .await
        .ok()?;
    if store.load().wifi_address != address {
        let _ = adb.checked(vec!["disconnect".into(), address], None).await;
        return None;
    }
    connected(&output, &address).then_some(address)
}
fn connected(bytes: &[u8], address: &str) -> bool {
    let text = String::from_utf8_lossy(bytes);
    // Exact ADB success prefixes; "failed to connect" cannot authorize a route.
    text.trim() == "connected"
        || text.trim() == format!("connected to {address}")
        || text.trim() == format!("already connected to {address}")
}
pub async fn setup<C: Commands>(
    adb: &Adb<C>,
    store: &ConfigStore,
    off: bool,
) -> Result<Option<String>> {
    if off {
        let old = store.load().wifi_address;
        store.update(|config| {
            config.wifi_address.clear();
            Ok(())
        })?;
        if valid_address(&old) {
            let _ = adb.checked(vec!["disconnect".into(), old], None).await;
        }
        return Ok(None);
    }
    let devices = adb.inventory().await.context("ADB inventory unavailable")?;
    let serial = usb_tablet(adb, &devices)
        .await
        .context("No authorized USB tablet with Blent installed")?;
    adb.checked(args(&serial, &["tcpip", "5555"]), None).await?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let ip = tablet_ip(adb, &serial)
        .await
        .context("Tablet address unavailable after enabling network ADB")?;
    let address = format!("{ip}:5555");
    ensure!(valid_address(&address), "Invalid tablet address");
    let bytes = adb
        .checked(vec!["connect".into(), address.clone()], None)
        .await?;
    ensure!(
        connected(&bytes, &address),
        "ADB did not confirm network connection"
    );
    store.update(|config| {
        config.wifi_address = address.clone();
        Ok(())
    })?;
    Ok(Some(address))
}
async fn usb_tablet<C: Commands>(adb: &Adb<C>, devices: &[String]) -> Option<String> {
    for serial in devices {
        if transport_of(serial) == Transport::Usb && adb.installed(serial).await == Some(true) {
            return Some(serial.clone());
        }
    }
    None
}
async fn tablet_ip<C: Commands>(adb: &Adb<C>, serial: &str) -> Option<String> {
    for command in [
        vec!["shell", "ip", "-f", "inet", "addr", "show", "wlan0"],
        vec!["shell", "ip", "route", "get", "1.1.1.1"],
        vec!["shell", "ip", "-f", "inet", "addr"],
    ] {
        if let Ok(output) = adb.checked(args(serial, &command), None).await {
            if let Some(ip) = crate::transport::parse_tablet_ip(&String::from_utf8_lossy(&output)) {
                return Some(ip);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;

pub struct Reconnect {
    path: std::path::PathBuf,
    adb: String,
}
impl Reconnect {
    pub fn new(path: std::path::PathBuf, adb: String) -> Self {
        Self { path, adb }
    }
    pub async fn connect(&self) -> Option<String> {
        reconnect(
            &crate::usb::Adb(crate::usb::NativeCommands(self.adb.clone().into())),
            &blent_config::storage::ConfigStore::new(self.path.clone()),
        )
        .await
    }
}

#[cfg(target_os = "linux")]
pub mod linux;
