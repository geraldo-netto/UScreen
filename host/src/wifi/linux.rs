//! Linux command/path adapter for shared Wi-Fi setup.
use anyhow::Result;
/// Switch the tablet's adb to TCP and remember where it lives, so the daemon
/// can pick it up over Wi-Fi on its own from then on.
///
/// This is deliberately the adb route rather than a port of our own. The
/// host video and input ports stay on loopback. adb tcpip opens the tablet
/// listener on port 5555; an authorized adb connection carries the tunnel.
/// --off forgets/disconnects that address but does not disable the listener.
pub async fn setup(off: bool) -> Result<()> {
    let address = crate::wifi::setup(
        &crate::usb::Adb(crate::usb::NativeCommands("adb".into())),
        &blent_config::storage::ConfigStore::default(),
        off,
    )
    .await?;
    match address {
        Some(address) => println!("Connected to {address}. USB remains preferred; the daemon reconnects while Wi-Fi is configured."),
        None => println!("Wi-Fi off. Saved address forgotten; tablet network ADB listener is unchanged."),
    }
    Ok(())
}
