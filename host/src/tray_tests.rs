//! T497: exercise tray presentation and channel actions without a desktop bus.
use super::*;
use std::path::Path;
use std::time::Duration;

const LIVE_TEST: &str = "tray::tests::t497_tray_actions_and_watch_updates_use_private_services";

fn isolated_tray() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let executable = directory.path().join("test-tray");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let tools = directory.path().join("bin");
    std::fs::create_dir(&tools).unwrap();
    let dbus = uscreen_config::linux::programs::find_in(
        "dbus-run-session",
        &std::env::var_os("PATH").unwrap_or_default(),
    )
    .expect("test prerequisite: dbus-run-session");
    let daemon = uscreen_config::linux::programs::find_in(
        "dbus-daemon",
        &std::env::var_os("PATH").unwrap_or_default(),
    )
    .expect("test prerequisite: dbus-daemon");
    let output = std::process::Command::new(dbus)
        .arg(format!("--dbus-daemon={}", daemon.display()))
        .arg("--")
        .arg(executable)
        .args(["--exact", LIVE_TEST, "--nocapture"])
        .env("USCREEN_T497_TRAY", "1")
        .env("HOME", directory.path())
        .env("PATH", tools)
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fake_launcher(path: &Path, marker: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(
        path,
        format!("#!/bin/sh\nprintf '%s\\n' \"{marker}:$*\" >> \"$HOME/launches\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

async fn launched(root: &Path, expected: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let lines = std::fs::read_to_string(root.join("launches")).unwrap_or_default();
            if lines.lines().any(|line| line == expected) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn launch_actions(root: &Path) {
    let sibling = root.join("uscreen-gui");
    let fallback = root.join("bin/uscreen-gui");
    fake_launcher(&sibling, "sibling");
    fake_launcher(&fallback, "fallback");
    fake_launcher(&root.join("bin/xdg-open"), "release");
    let (mut item, _, _) = tray();
    item.activate(i32::MIN, i32::MAX);
    launched(root, "sibling:").await;
    std::fs::remove_file(sibling).unwrap();
    item.update = Some("v-test".into());
    for action in item.menu() {
        if let MenuItem::Standard(action) = action {
            if action.label == "Settings…" || action.label.starts_with("Update available:") {
                (action.activate)(&mut item);
            }
        }
    }
    launched(root, "fallback:").await;
    launched(root, &format!("release:{}", crate::update::RELEASES_PAGE)).await;
    std::fs::remove_file(fallback).unwrap();
    std::fs::remove_file(root.join("bin/xdg-open")).unwrap();
    open_settings();
    open_release_page();
}

struct Watcher(tokio::sync::mpsc::UnboundedSender<String>);

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: &str) {
        self.0.send(service.to_owned()).unwrap();
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
}

async fn tray_property(proxy: &zbus::Proxy<'_>, name: &str, expected: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let value: String = proxy.get_property(name).await.unwrap();
            if value == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn tray_description(proxy: &zbus::Proxy<'_>, expected: &str) {
    type Tooltip = (String, Vec<(i32, i32, Vec<u8>)>, String, String);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let value: Tooltip = proxy.get_property("ToolTip").await.unwrap();
            if value.3.contains(expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn watched_state() {
    let (mode, _) = watch::channel(false);
    let (tablets, tablet_rx) = watch::channel(false);
    let (shutdown, _) = watch::channel(false);
    let (updates, update_rx) = watch::channel(None);
    // No watcher on this private bus is an ordinary, nonfatal condition.
    tokio::time::timeout(
        Duration::from_secs(3),
        run(
            mode.clone(),
            tablet_rx.clone(),
            shutdown.clone(),
            update_rx.clone(),
            true,
        ),
    )
    .await
    .unwrap();
    let (registered, mut registrations) = tokio::sync::mpsc::unbounded_channel();
    let connection = zbus::connection::Builder::session()
        .unwrap()
        .name("org.kde.StatusNotifierWatcher")
        .unwrap()
        .serve_at("/StatusNotifierWatcher", Watcher(registered))
        .unwrap()
        .build()
        .await
        .unwrap();
    let task = tokio::spawn(run(mode.clone(), tablet_rx, shutdown, update_rx, true));
    let destination = tokio::time::timeout(Duration::from_secs(3), registrations.recv())
        .await
        .unwrap()
        .unwrap();
    let proxy = zbus::proxy::Builder::new(&connection)
        .destination(destination)
        .unwrap()
        .path("/StatusNotifierItem")
        .unwrap()
        .interface("org.kde.StatusNotifierItem")
        .unwrap()
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .unwrap();
    mode.send(true).unwrap();
    tray_property(&proxy, "IconName", ICON_TABLET).await;
    tablets.send(true).unwrap();
    tray_description(&proxy, "Graphics tablet").await;
    updates.send(Some("v-test".into())).unwrap();
    tray_description(&proxy, "Update available: v-test").await;
    mode.send(false).unwrap();
    tray_property(&proxy, "IconName", ICON_SCREEN).await;
    tablets.send(false).unwrap();
    tray_description(&proxy, "No tablet connected").await;
    drop(updates);
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn t497_tray_actions_and_watch_updates_use_private_services() {
    if std::env::var_os("USCREEN_T497_TRAY").is_none() {
        isolated_tray();
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    launch_actions(&root).await;
    watched_state().await;
}

fn tray() -> (UScreenTray, watch::Receiver<bool>, watch::Receiver<bool>) {
    let (mode_tx, mode) = watch::channel(false);
    let (shutdown_tx, shutdown) = watch::channel(false);
    (
        UScreenTray {
            pen_only: false,
            pen_device: true,
            tablet_present: false,
            update: None,
            mode_tx,
            shutdown_tx,
        },
        mode,
        shutdown,
    )
}

#[test]
fn t497_icons_and_tooltips_follow_connection_mode_and_update() {
    let (mut tray, _, _) = tray();
    assert_eq!(tray.id(), "uscreen");
    assert_eq!(tray.title(), "UScreen");
    assert_eq!(tray.status(), Status::Active);
    assert_eq!(tray.state_line(), "No tablet connected");
    assert_eq!(tray.icon_name(), ICON_SCREEN);
    assert_eq!(tray.icon_pixmap()[0].data.len(), 64 * 64 * 4);
    assert_eq!(tray.tool_tip().description, tray.state_line());
    tray.tablet_present = true;
    assert_eq!(tray.state_line(), "Second screen");
    tray.pen_only = true;
    tray.update = Some("v2".into());
    assert_eq!(tray.icon_name(), ICON_TABLET);
    assert!(tray.tool_tip().description.contains("Graphics tablet"));
    assert!(tray
        .tool_tip()
        .description
        .ends_with("Update available: v2"));
    assert_ne!(tray.icon_pixmap()[0].data, pixmap(PIXMAP_SCREEN).data);
    assert_eq!(pixmap(&[1, 2, 3, 4, 9]).data, [4, 1, 2, 3]);
}

fn toggle(tray: &mut UScreenTray) {
    let item = tray
        .menu()
        .into_iter()
        .find_map(|item| match item {
            MenuItem::Checkmark(item) => Some(item),
            _ => None,
        })
        .unwrap();
    assert_eq!(item.checked, tray.pen_only);
    (item.activate)(tray);
}

#[test]
fn t497_menu_uses_authoritative_mode_and_prevents_penless_transition() {
    let (mut tray, mode, shutdown) = tray();
    let plain_count = tray.menu().len();
    tray.update = Some("v2".into());
    assert_eq!(tray.menu().len(), plain_count + 1);
    tray.pen_device = false;
    toggle(&mut tray);
    assert!(!*mode.borrow());
    tray.pen_device = true;
    toggle(&mut tray);
    assert!(*mode.borrow());
    assert!(
        !tray.pen_only,
        "T497: menu invented unconfirmed daemon state"
    );
    tray.pen_only = true;
    toggle(&mut tray);
    assert!(!*mode.borrow());
    let quit = tray
        .menu()
        .into_iter()
        .find_map(|item| match item {
            MenuItem::Standard(item) if item.label == "Quit" => Some(item),
            _ => None,
        })
        .unwrap();
    (quit.activate)(&mut tray);
    assert!(*shutdown.borrow());
}
