use super::*;
use crate::usb::{CommandFuture, NativeCommands};
use std::{
    process::Output,
    sync::{Arc, Mutex},
};
#[derive(Default)]
struct State {
    calls: Vec<Vec<String>>,
    fail: Option<String>,
    inventory: String,
    ip: String,
    reply: String,
    change: Option<ConfigStore>,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);
fn output(code: u32, text: &str) -> Output {
    #[cfg(unix)]
    let status = {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw((code as i32) << 8)
    };
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code)
    };
    Output {
        status,
        stdout: text.as_bytes().to_vec(),
        stderr: vec![],
    }
}
impl Commands for Fake {
    fn execute(&self, args: Vec<String>, _: Option<Vec<u8>>) -> CommandFuture<'_> {
        Box::pin(async move {
            let mut s = self.0.lock().unwrap();
            s.calls.push(args.clone());
            if s.fail
                .as_ref()
                .is_some_and(|part| args.join(" ").contains(part))
            {
                return Ok(output(1, ""));
            }
            let text = match args[0].as_str() {
                "devices" => s.inventory.clone(),
                "connect" => {
                    if let Some(store) = s.change.take() {
                        store
                            .update(|c| {
                                c.wifi_address.clear();
                                Ok(())
                            })
                            .unwrap();
                    }
                    s.reply.clone()
                }
                "disconnect" => String::new(),
                _ => device_reply(&s, &args),
            };
            Ok(output(0, &text))
        })
    }
}
fn device_reply(s: &State, args: &[String]) -> String {
    if args.contains(&"pm".into()) {
        "package:/data/app/blent/base.apk".into()
    } else if args.contains(&"ip".into()) {
        s.ip.clone()
    } else {
        String::new()
    }
}
fn fixture() -> (tempfile::TempDir, ConfigStore, Adb<Fake>) {
    let root = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(root.path().join("config.toml"));
    let fake = Fake::default();
    {
        let mut s = fake.0.lock().unwrap();
        s.inventory = "List of devices attached\nUSB\tdevice\n".into();
        s.ip = "inet 192.0.2.1/24".into();
        s.reply = "connected to 192.0.2.1:5555".into();
    }
    (root, store, Adb(fake))
}
#[tokio::test(start_paused = true)]
async fn t691_setup_off_and_stale_connect_preserve_configuration_ownership() {
    let (_root, store, adb) = fixture();
    store
        .update(|c| {
            c.quality = 24;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        setup(&adb, &store, false).await.unwrap().as_deref(),
        Some("192.0.2.1:5555")
    );
    assert_eq!(store.load().quality, 24);
    assert_eq!(
        reconnect(&adb, &store).await.as_deref(),
        Some("192.0.2.1:5555")
    );
    adb.0 .0.lock().unwrap().change = Some(store.clone());
    assert!(reconnect(&adb, &store).await.is_none());
    assert!(adb
        .0
         .0
        .lock()
        .unwrap()
        .calls
        .contains(&vec!["disconnect".into(), "192.0.2.1:5555".into()]));
    setup(&adb, &store, false).await.unwrap();
    assert!(setup(&adb, &store, true).await.unwrap().is_none());
    assert!(store.load().wifi_address.is_empty());
    assert!(reconnect(&adb, &store).await.is_none());
}
#[tokio::test(start_paused = true)]
async fn t691_setup_rejects_unavailable_devices_addresses_and_false_success() {
    for failed in ["devices", "pm", "tcpip", "connect"] {
        let (_root, store, adb) = fixture();
        adb.0 .0.lock().unwrap().fail = Some(failed.into());
        assert!(setup(&adb, &store, false).await.is_err());
        assert!(store.load().wifi_address.is_empty());
    }
    for inventory in [
        "bad",
        "List of devices attached\nUSB\tunauthorized\n",
        "List of devices attached\n192.0.2.1:5555\tdevice\n",
    ] {
        let (_root, store, adb) = fixture();
        adb.0 .0.lock().unwrap().inventory = inventory.into();
        assert!(setup(&adb, &store, false).await.is_err());
    }
    let (_root, store, adb) = fixture();
    adb.0 .0.lock().unwrap().ip = "inet 127.0.0.1/8".into();
    assert!(setup(&adb, &store, false).await.is_err());
    adb.0 .0.lock().unwrap().ip = "inet 192.0.2.1/24".into();
    adb.0 .0.lock().unwrap().reply = "disconnected".into();
    assert!(setup(&adb, &store, false).await.is_err());
    store
        .update(|c| {
            c.wifi_address = "192.0.2.1:5555".into();
            Ok(())
        })
        .unwrap();
    assert!(reconnect(&adb, &store).await.is_none());
}
#[test]
fn t691_address_and_reply_bounds_are_permanent() {
    for good in ["192.0.2.1:5555", "[2001:db8::1]:5555", "tablet.local:5555"] {
        assert!(valid_address(good));
    }
    for bad in [
        "",
        "--all",
        ":5555",
        "host:0",
        "host:65536",
        "host:-1",
        "bad host:5555",
        "-host:5",
        "host-:5",
        "a..b:5",
        "0.0.0.0:5",
        "224.0.0.1:5",
        "host\0:5",
    ] {
        assert!(!valid_address(bad), "{bad:?}");
    }
    assert!(!valid_address(&format!("{}:5", "a".repeat(254))));
    for byte in 0..=255 {
        let text = String::from_utf8_lossy(&[byte; 64]).into_owned();
        let _ = valid_address(&text);
    }
    for good in [
        "connected",
        "connected to host:5",
        "already connected to host:5",
    ] {
        assert!(connected(good.as_bytes(), "host:5"));
    }
    for bad in [
        "disconnected",
        "not connected",
        "connected to other:5",
        "failed to connect",
    ] {
        assert!(!connected(bad.as_bytes(), "host:5"));
    }
}
#[path = "../../tests/support/usb_adb.rs"]
mod native_fixture;
#[tokio::test]
async fn t691_native_adb_setup_and_off_use_owned_commands_and_store() {
    let root = tempfile::tempdir().unwrap();
    native_fixture::fixture(root.path());
    let adb = Adb(NativeCommands(
        root.path()
            .join(blent_config::platform::executable_name("adb")),
    ));
    let store = ConfigStore::new(root.path().join("config.toml"));
    assert!(setup(&adb, &store, false).await.unwrap().is_some());
    assert_eq!(adb.identity("USB").await.as_deref(), Some("owned-tablet"));
    assert!(Reconnect::new(
        store.path().unwrap().to_owned(),
        root.path()
            .join(blent_config::platform::executable_name("adb"))
            .to_str()
            .unwrap()
            .into()
    )
    .connect()
    .await
    .is_some());
    assert!(setup(&adb, &store, true).await.unwrap().is_none());
    assert!(store.load().wifi_address.is_empty());
}
#[test]
fn t691_transport_policy_preserves_known_usb_and_unknown_devices() {
    use crate::transport::*;
    let identities = std::collections::HashMap::from([
        ("USB".into(), "one".into()),
        ("net:5".into(), "one".into()),
        ("mdns._adb._tcp".into(), "one".into()),
    ]);
    let devices = vec!["net:5".into(), "USB".into(), "OTHER".into()];
    assert_eq!(
        select_device_transports(&devices, Some("net:5"), &identities),
        ["USB", "OTHER"]
    );
    assert_eq!(attachment_identity("OTHER", &identities), "transport:OTHER");
    assert_eq!(
        current_transport(&devices, Some("USB"), &identities).as_deref(),
        Some("net:5")
    );
    assert_eq!(
        current_transport(&devices, Some("OTHER"), &identities).as_deref(),
        Some("OTHER")
    );
    assert!(current_transport(&devices, None, &identities).is_none());
    assert_eq!(
        select_device_transports(
            &["net:5".into(), "mdns._adb._tcp".into()],
            Some("mdns._adb._tcp"),
            &identities
        ),
        ["mdns._adb._tcp"]
    );
    for text in [
        "",
        "inet",
        "inet invalid",
        "src 127.0.0.1",
        "src 999.999.999.999",
    ] {
        assert!(parse_tablet_ip(text).is_none());
    }
    assert_eq!(
        parse_tablet_ip("garbage inet invalid src 192.0.2.1").as_deref(),
        Some("192.0.2.1")
    );
}

#[test]
fn t691_tray_distinguishes_network_and_usb_and_bounds_session_counts() {
    use crate::tray_state::State;
    let session = blent_config::tablets::TabletSession {
        serial: "192.0.2.1:5555".into(),
        instance: 0,
        video_port: 8890,
        input_port: 8891,
    };
    assert_eq!(State::connections(&[]), State::Waiting);
    assert_eq!(State::connections(&[session.clone()]), State::Network(1));
    assert!(State::connections(&[session.clone()])
        .line()
        .contains("Network ADB"));
    assert_eq!(State::connections(&vec![session; 5]), State::Unavailable);
    let usb = blent_config::tablets::TabletSession {
        serial: "USB".into(),
        instance: 0,
        video_port: 8890,
        input_port: 8891,
    };
    assert_eq!(State::connections(&[usb]), State::Prepared(1));
}

#[tokio::test]
async fn t691_off_without_commands_still_forgets_saved_address() {
    let (_root, store, _adb) = fixture();
    store
        .update(|c| {
            c.wifi_address = "192.0.2.1:5555".into();
            Ok(())
        })
        .unwrap();
    setup(&Adb(None::<NativeCommands>), &store, true)
        .await
        .unwrap();
    assert!(store.load().wifi_address.is_empty());
}
