use super::*;
use crate::{attachment::Attachment, media::EncoderSettings};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Default)]
struct State {
    routes: BTreeMap<u16, u16>,
    other_routes: BTreeMap<String, BTreeMap<u16, u16>>,
    calls: Vec<(Vec<String>, Option<Vec<u8>>)>,
    inventory: String,
    inventory_code: u32,
    fail: Option<String>,
    installed: bool,
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
        stderr: Vec::new(),
    }
}
impl Commands for Fake {
    fn execute(&self, arguments: Vec<String>, input: Option<Vec<u8>>) -> CommandFuture<'_> {
        Box::pin(async move {
            let mut state = self.0.lock().unwrap();
            state.calls.push((arguments.clone(), input));
            if state
                .fail
                .as_ref()
                .is_some_and(|failure| arguments.join(" ").contains(failure))
            {
                return Ok(output(7, ""));
            }
            Ok(fake_reply(&mut state, &arguments))
        })
    }
}
fn fake_reply(state: &mut State, args: &[String]) -> Output {
    if args == ["devices"] {
        return output(state.inventory_code, &state.inventory);
    }
    match args[2].as_str() {
        "reverse" => reverse_for(state, &args[1], &args[3..]),
        "shell" if args[3] == "pm" => output(
            u32::from(!state.installed),
            if state.installed {
                "package:/data/app/blent/base.apk\n"
            } else {
                ""
            },
        ),
        "shell" => output(0, ""),
        _ => panic!("Unexpected fake ADB invocation: {args:?}"),
    }
}
fn reverse_for(state: &mut State, serial: &str, args: &[String]) -> Output {
    if serial == "USB" {
        return reverse_reply(state, args);
    }
    let mut device = State {
        routes: state.other_routes.remove(serial).unwrap_or_default(),
        ..Default::default()
    };
    let reply = reverse_reply(&mut device, args);
    state.other_routes.insert(serial.into(), device.routes);
    reply
}
fn reverse_reply(state: &mut State, args: &[String]) -> Output {
    let port = |text: &str| text.strip_prefix("tcp:").unwrap().parse::<u16>().unwrap();
    match args[0].as_str() {
        "--list" => output(
            0,
            &state
                .routes
                .iter()
                .map(|(remote, local)| format!("UsbFfs tcp:{remote} tcp:{local}\n"))
                .collect::<String>(),
        ),
        "--no-rebind" => {
            let remote = port(&args[1]);
            if state.routes.contains_key(&remote) {
                return output(1, "");
            }
            state.routes.insert(remote, port(&args[2]));
            output(0, "")
        }
        "--remove" => {
            state.routes.remove(&port(&args[1]));
            output(0, "")
        }
        _ => panic!("Unexpected reverse mutation: {args:?}"),
    }
}
fn fixture() -> (Adb<Fake>, connection::Connection) {
    let fake = Fake::default();
    fake.0.lock().unwrap().installed = true;
    let settings = watch::channel(EncoderSettings {
        encoder: "libx264".into(),
        fps: 30,
        bitrate: 20000,
        width: 640,
        height: 480,
        quality: 18,
        width_mm: 310,
        height_mm: 194,
        stream_scale: 1,
        geometry_ready: false,
        decoders: None,
        decoder_epoch: 0,
        selection: None,
    })
    .0;
    let attachment = Attachment::with_token(settings, Some("a".repeat(64)));
    (
        Adb(fake),
        connection::Connection::new(attachment, (9000, 9001), true).unwrap(),
    )
}

#[tokio::test]
async fn t654_inventory_status_and_size_are_independent_requirements() {
    let (adb, _) = fixture();
    let listing = "List of devices attached\nUSB\tdevice\n";
    for size in [65535, 65536, 65537, 65538, 131072] {
        for code in [0, 1, 7, 255] {
            {
                let mut state = adb.0 .0.lock().unwrap();
                state.inventory = format!("{listing}{}", " ".repeat(size - listing.len()));
                state.inventory_code = code;
            }
            let expected = (code == 0 && size <= 65536).then(|| vec!["USB".into()]);
            assert_eq!(
                adb.inventory().await,
                expected,
                "T654: size={size}, exit={code}"
            );
        }
    }
}

#[tokio::test]
async fn t525_usb_inventory_rejects_unknown_failed_and_nonready_observations() {
    let (adb, _) = fixture();
    for invalid in [
        "",
        "partial",
        "List of devices attached\nUSB\tdevice\nUSB\toffline",
        "List of devices attached\nUSB\tbogus",
    ] {
        adb.0 .0.lock().unwrap().inventory = invalid.into();
        assert_eq!(adb.inventory().await, None);
    }
    adb.0.0.lock().unwrap().inventory="List of devices attached\r\nOFF\toffline\r\nNO\tunauthorized\r\nUSB\tdevice\r\n192.0.2.1:5555\tdevice\nphone._adb-tls-connect._tcp\tdevice\n".into();
    assert_eq!(adb.inventory().await, Some(vec!["USB".into()]));
    adb.0 .0.lock().unwrap().fail = Some("devices".into());
    assert_eq!(adb.inventory().await, None);
    adb.0 .0.lock().unwrap().fail = None;
    adb.0 .0.lock().unwrap().inventory = " ".repeat(65537);
    assert_eq!(adb.inventory().await, None);
}

#[tokio::test]
async fn t525_attachment_rotates_credentials_and_retires_owned_routes() {
    let (adb, mut connection) = fixture();
    assert!(connection.serial().is_none());
    assert!(connection.redeliver(&adb).await.is_err());
    assert!(connection.refresh(&adb).await.is_err());
    let presence = connection.attachment.subscribe();
    connection.connect(&adb, "USB").await.unwrap();
    assert_eq!(connection.serial(), Some("USB"));
    assert_eq!(connection.selected(), Some("USB"));
    let first = connection.attachment.token().unwrap().unwrap();
    assert_ne!(first, "a".repeat(64));
    assert!(*presence.borrow());
    assert!(connection.connect(&adb, "OTHER").await.is_err());
    connection.redeliver(&adb).await.unwrap();
    {
        let state = adb.0 .0.lock().unwrap();
        assert_eq!(state.routes.len(), 2);
        assert!(state
            .calls
            .iter()
            .all(|(args, _)| !args.join(" ").contains(&first)));
        let inputs: Vec<_> = state
            .calls
            .iter()
            .filter_map(|(_, input)| input.as_deref())
            .collect();
        assert!(String::from_utf8_lossy(inputs[0]).contains("am start"));
        assert!(String::from_utf8_lossy(inputs[1]).contains("am broadcast"));
        assert!(inputs
            .iter()
            .all(|bytes| String::from_utf8_lossy(bytes).contains(&first)));
    }
    adb.0 .0.lock().unwrap().routes.remove(&8890);
    connection.refresh(&adb).await.unwrap();
    assert_eq!(adb.0 .0.lock().unwrap().routes.len(), 2);
    connection.disconnect(&adb).await.unwrap();
    assert!(!*connection.attachment.subscribe().borrow());
    assert!(adb.0 .0.lock().unwrap().routes.is_empty());
    connection.connect(&adb, "USB").await.unwrap();
    assert_ne!(
        connection.attachment.token().unwrap().as_deref(),
        Some(first.as_str())
    );
    connection.disconnect(&adb).await.unwrap();
}

#[tokio::test]
async fn t525_partial_startup_and_failed_delivery_never_publish_ready() {
    for failure in [
        "tcp:8891 tcp:9001",
        "shell -T",
        "shell pm",
        "reverse --list",
    ] {
        let (adb, mut connection) = fixture();
        adb.0 .0.lock().unwrap().fail = Some(failure.into());
        assert!(connection.connect(&adb, "USB").await.is_err());
        assert_eq!(connection.serial(), None);
        assert!(!*connection.attachment.subscribe().borrow());
        assert!(adb.0 .0.lock().unwrap().routes.is_empty());
        adb.0 .0.lock().unwrap().fail = None;
        connection.disconnect(&adb).await.unwrap();
        connection.connect(&adb, "USB").await.unwrap();
        connection.disconnect(&adb).await.unwrap();
    }
    let (adb, mut connection) = fixture();
    adb.0 .0.lock().unwrap().installed = false;
    assert!(connection.connect(&adb, "USB").await.is_err());
    assert!(adb.0 .0.lock().unwrap().routes.is_empty());
}

#[tokio::test]
async fn t525_cleanup_preserves_foreign_routes_and_retains_failed_ownership() {
    let (adb, mut connection) = fixture();
    adb.0 .0.lock().unwrap().routes.insert(8890, 9000);
    connection.connect(&adb, "USB").await.unwrap();
    adb.0 .0.lock().unwrap().fail = Some("--remove".into());
    assert!(connection.disconnect(&adb).await.is_err());
    assert!(!*connection.attachment.subscribe().borrow());
    assert!(connection.connect(&adb, "OTHER").await.is_err());
    adb.0 .0.lock().unwrap().fail = None;
    connection.disconnect(&adb).await.unwrap();
    assert_eq!(adb.0 .0.lock().unwrap().routes.get(&8890), Some(&9000));
    connection.connect(&adb, "USB").await.unwrap();
    adb.0 .0.lock().unwrap().routes.insert(8891, 9999);
    connection.disconnect(&adb).await.unwrap();
    assert_eq!(adb.0 .0.lock().unwrap().routes.get(&8891), Some(&9999));
    assert!(connection.connect(&adb, "USB").await.is_err());
    assert_eq!(adb.0 .0.lock().unwrap().routes.get(&8891), Some(&9999));
}

#[tokio::test]
async fn t525_bounded_invalid_tokens_serials_and_ports_cause_no_commands() {
    let (adb, _) = fixture();
    for length in 0..128 {
        assert!(adb.deliver("USB", &"x".repeat(length), true).await.is_err());
    }
    for serial in ["", "a\0b", "a\nb", "1.2.3.4:5555", "name._adb._tcp"] {
        assert!(Routes::new(serial, (9000, 9001)).is_err());
    }
    assert!(Routes::new(&"x".repeat(1025), (1, 2)).is_err());
    for ports in [(0, 1), (1, 0), (1, 1), (65535, 65535)] {
        assert!(Routes::new("USB", ports).is_err());
    }
    for ports in [(1, 2), (65534, 65535)] {
        assert!(Routes::new("USB", ports).is_ok());
    }
    assert!(adb.0 .0.lock().unwrap().calls.is_empty());
}

fn free_port_pairs() -> ((u16, u16), Vec<std::net::TcpListener>) {
    // Keep each multi-socket reservation atomic with respect to sibling tests.
    static ALLOCATION: Mutex<()> = Mutex::new(());
    let _allocation = ALLOCATION.lock().unwrap();
    for _ in 0..64 {
        let video = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let video_port = video.local_addr().unwrap().port();
        let Some(next_video) = video_port.checked_add(2) else {
            continue;
        };
        // T655: reserve the sibling before requesting another ephemeral port.
        // Interleaved Windows allocations must not choose video + 2 as input.
        let Ok(extra_video) = std::net::TcpListener::bind(("127.0.0.1", next_video)) else {
            continue;
        };
        let input = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let ports = (video_port, input.local_addr().unwrap().port());
        let Ok(pairs) = blent_config::slot_ports(ports.0, ports.1, 2) else {
            continue;
        };
        let Ok(extra_input) = std::net::TcpListener::bind(("127.0.0.1", pairs[1].1)) else {
            continue;
        };
        return (ports, vec![video, input, extra_video, extra_input]);
    }
    panic!("T525: no two-slot listener fixture available");
}

#[test]
fn t655_concurrent_listener_fixtures_reserve_disjoint_pairs() {
    for _ in 0..16 {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    free_port_pairs()
                })
            })
            .collect();
        let reservations: Vec<_> = handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        for (ports, listeners) in &reservations {
            assert_eq!(listeners.len(), 4);
            assert!(blent_config::slot_ports(ports.0, ports.1, 2).is_ok());
            for listener in listeners {
                assert!(seen.insert(listener.local_addr().unwrap().port()));
            }
        }
    }
}
async fn monitor_fixture() -> (
    Adb<Fake>,
    monitor::Monitor<Fake>,
    watch::Sender<bool>,
    (u16, u16),
) {
    let (ports, _reserved) = free_port_pairs();
    let (adb, _) = fixture();
    adb.0 .0.lock().unwrap().inventory = "List of devices attached\nUSB\tdevice\n".into();
    let config = blent_config::FileConfig {
        video_port: ports.0,
        input_port: ports.1,
        ..Default::default()
    };
    let (stop, receiver) = watch::channel(false);
    let monitor = monitor::Monitor::new(Adb(adb.0.clone()), config, receiver).unwrap();
    (adb, monitor, stop, ports)
}
fn delivered(adb: &Adb<Fake>) -> String {
    let state = adb.0 .0.lock().unwrap();
    let input = state
        .calls
        .iter()
        .rev()
        .find_map(|(_, input)| input.as_ref())
        .unwrap();
    String::from_utf8_lossy(input)
        .split(" --es token ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .into()
}
async fn control(
    port: u16,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use futures_util::SinkExt;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"type":"auth","token":token}).to_string(),
        ))
        .await
        .unwrap();
    socket
}
#[tokio::test]
async fn t525_monitor_reconnect_uses_shared_authenticated_protocol_and_retires_old_clients() {
    use futures_util::StreamExt;
    let (adb, mut monitor, _stop, ports) = monitor_fixture().await;
    monitor.poll().await;
    assert_eq!(monitor.sessions()[0].serial, "USB");
    let token = delivered(&adb);
    let mut socket = control(ports.1, &token).await;
    let greeting = socket.next().await.unwrap().unwrap().into_text().unwrap();
    let greeting: serde_json::Value = serde_json::from_str(&greeting).unwrap();
    assert_eq!(greeting["transport"], "usb");
    assert_eq!(greeting["pen"], false);
    assert_eq!(greeting["touch"], false);
    assert_eq!(greeting["pen_only"], true);
    adb.0 .0.lock().unwrap().inventory = "malformed".into();
    monitor.poll().await;
    assert_eq!(monitor.sessions().len(), 1);
    adb.0 .0.lock().unwrap().inventory = "List of devices attached\nUSB\toffline\n".into();
    monitor.poll().await;
    assert!(monitor.sessions().is_empty());
    assert!(adb.0 .0.lock().unwrap().routes.is_empty());
    let old = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap();
    assert!(!matches!(
        old,
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))
    ));
    adb.0 .0.lock().unwrap().inventory = "List of devices attached\nUSB\tdevice\n".into();
    monitor.poll().await;
    assert_ne!(delivered(&adb), token);
    let mut old = control(ports.1, &token).await;
    let response = tokio::time::timeout(Duration::from_secs(2), old.next())
        .await
        .unwrap();
    assert!(!matches!(
        response,
        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))
    ));
    monitor.shutdown().await.unwrap();
    tokio::net::TcpListener::bind(("127.0.0.1", ports.0))
        .await
        .unwrap();
    tokio::net::TcpListener::bind(("127.0.0.1", ports.1))
        .await
        .unwrap();
}
#[tokio::test(start_paused = true)]
async fn t657_redelivery_waits_for_each_complete_interval() {
    let (adb, mut monitor, _stop, _) = monitor_fixture().await;
    monitor.poll().await;
    for (nanoseconds, expected) in [
        (0, 1),
        (4_999_999_999, 1),
        (1, 2),
        (0, 2),
        (4_999_999_999, 2),
        (1, 3),
    ] {
        tokio::time::advance(Duration::from_nanos(nanoseconds)).await;
        monitor.poll().await;
        let deliveries = adb
            .0
             .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(_, input)| input.is_some())
            .count();
        assert_eq!(
            deliveries, expected,
            "T657: after advancing {nanoseconds} ns"
        );
    }
    monitor.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn t525_monitor_repairs_redelivers_and_reports_failed_cleanup() {
    let (adb, mut monitor, stop, _) = monitor_fixture().await;
    monitor.poll().await;
    let token = delivered(&adb);
    adb.0 .0.lock().unwrap().routes.remove(&8890);
    tokio::time::advance(Duration::from_secs(5)).await;
    monitor.poll().await;
    assert_eq!(adb.0 .0.lock().unwrap().routes.len(), 2);
    assert_eq!(delivered(&adb), token);
    assert!(String::from_utf8_lossy(
        adb.0
             .0
            .lock()
            .unwrap()
            .calls
            .last()
            .unwrap()
            .1
            .as_ref()
            .unwrap()
    )
    .contains("am broadcast"));
    adb.0 .0.lock().unwrap().fail = Some("--remove".into());
    assert!(monitor.shutdown().await.is_err());
    assert!(monitor.sessions().is_empty());
    adb.0 .0.lock().unwrap().fail = None;
    monitor.poll().await;
    assert_eq!(monitor.sessions().len(), 1);
    stop.send_replace(true);
    monitor.poll().await;
    monitor.shutdown().await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn t525_monitor_partial_bind_command_failure_and_invalid_config() {
    let (adb, mut monitor, _stop, ports) = monitor_fixture().await;
    let occupied = tokio::net::TcpListener::bind(("127.0.0.1", ports.1))
        .await
        .unwrap();
    monitor.poll().await;
    assert!(monitor.sessions().is_empty());
    assert!(adb.0 .0.lock().unwrap().routes.is_empty());
    drop(occupied);
    adb.0 .0.lock().unwrap().fail = Some("shell -T".into());
    monitor.poll().await;
    assert!(monitor.sessions().is_empty());
    adb.0 .0.lock().unwrap().fail = None;
    monitor.poll().await;
    assert!(
        monitor.sessions().is_empty(),
        "T525: failed device retried before deadline"
    );
    tokio::time::advance(Duration::from_secs(5)).await;
    monitor.poll().await;
    assert_eq!(monitor.sessions().len(), 1);
    adb.0 .0.lock().unwrap().fail = Some("reverse --list".into());
    monitor.poll().await;
    assert!(monitor.sessions().is_empty());
    adb.0 .0.lock().unwrap().fail = None;
    monitor.shutdown().await.unwrap();
    for config in [
        blent_config::FileConfig {
            require_token: false,
            ..Default::default()
        },
        blent_config::FileConfig {
            max_tablets: 0,
            ..Default::default()
        },
        blent_config::FileConfig {
            video_port: 0,
            ..Default::default()
        },
    ] {
        assert!(
            monitor::Monitor::new(Adb(adb.0.clone()), config, watch::channel(false).1).is_err()
        );
    }
}

#[tokio::test]
async fn t525_preview_rejects_unavailable_display_mode() {
    use futures_util::{SinkExt, StreamExt};
    let (adb, mut monitor, _stop, ports) = monitor_fixture().await;
    monitor.poll().await;
    let mut socket = control(ports.1, &delivered(&adb)).await;
    let _ = socket.next().await;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"type":"mode","pen_only":false}"#.into(),
        ))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(
        reply["status"], "settings_rejected",
        "T525: preview advertised unavailable display mode"
    );
    monitor.shutdown().await.unwrap();
}
#[tokio::test]
async fn t525_failed_device_does_not_starve_next_eligible_usb_device() {
    let (adb, _, stop, ports) = monitor_fixture().await;
    let config = blent_config::FileConfig {
        video_port: ports.0,
        input_port: ports.1,
        ..Default::default()
    };
    let mut monitor = monitor::Monitor::new(Adb(adb.0.clone()), config, stop.subscribe()).unwrap();
    adb.0 .0.lock().unwrap().inventory =
        "List of devices attached\nBAD\tdevice\nUSB\tdevice\n".into();
    adb.0 .0.lock().unwrap().fail = Some("BAD reverse".into());
    monitor.poll().await;
    monitor.poll().await;
    assert_eq!(
        monitor.sessions().first().map(|s| s.serial.as_str()),
        Some("USB"),
        "T525: one failing device monopolized all attempts"
    );
    monitor.shutdown().await.unwrap();
    monitor.shutdown().await.unwrap();
}

#[tokio::test]
async fn t525_two_slots_preserve_assignments_and_cleanup_independently() {
    let (adb, _, stop, ports) = monitor_fixture().await;
    let config = blent_config::FileConfig {
        video_port: ports.0,
        input_port: ports.1,
        max_tablets: 2,
        ..Default::default()
    };
    let mut monitor = monitor::Monitor::new(Adb(adb.0.clone()), config, stop.subscribe()).unwrap();
    adb.0 .0.lock().unwrap().inventory =
        "List of devices attached\nUSB\tdevice\nOTHER\tdevice\n".into();
    monitor.poll().await;
    let sessions = monitor.sessions();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].serial, "USB");
    assert_eq!(sessions[1].serial, "OTHER");
    assert_eq!(
        (sessions[1].video_port, sessions[1].input_port),
        (ports.0 + 2, ports.1 + 2)
    );
    adb.0 .0.lock().unwrap().inventory = "List of devices attached\nOTHER\tdevice\n".into();
    monitor.poll().await;
    assert_eq!(monitor.sessions(), vec![sessions[1].clone()]);
    assert!(adb.0 .0.lock().unwrap().routes.is_empty());
    assert_eq!(adb.0 .0.lock().unwrap().other_routes["OTHER"].len(), 2);
    monitor.shutdown().await.unwrap();
    assert!(adb.0 .0.lock().unwrap().other_routes["OTHER"].is_empty());
}

#[tokio::test]
async fn t650_malformed_serial_preserves_connected_attachment_and_credential() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (adb, mut monitor, _stop, ports) = monitor_fixture().await;
    monitor.poll().await;
    let token = delivered(&adb);
    let sessions = monitor.sessions();
    let mut socket = control(ports.1, &token).await;
    let _ = socket.next().await.unwrap().unwrap();
    for serial in ["\0USB", "USB\0corrupt", "USB\0"] {
        adb.0 .0.lock().unwrap().inventory =
            format!("List of devices attached\n{serial}\tdevice\n");
        monitor.poll().await;
        assert_eq!(
            monitor.sessions(),
            sessions,
            "T650: malformed inventory detached known tablet"
        );
        assert_eq!(delivered(&adb), token);
        socket.send(Message::Ping(vec![42])).await.unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            reply,
            Message::Pong(vec![42]),
            "T650: old authenticated lease was retired"
        );
    }
    monitor.shutdown().await.unwrap();
}
