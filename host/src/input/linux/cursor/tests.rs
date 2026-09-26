use super::*;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

#[test]
fn t604_policy_is_scoped_to_enabled_cinnamon_x11_touch() {
    for desktop in ["Cinnamon", "X-Cinnamon", "GNOME:Cinnamon", "cinnamon"] {
        assert!(supported(true, desktop, "x11"));
        assert!(!supported(false, desktop, "x11"));
        assert!(!supported(true, desktop, "wayland"));
    }
    for desktop in ["", "KDE", "GNOME", "not-Cinnamon", "CinnamonExtra"] {
        assert!(!supported(true, desktop, "x11"));
    }
    for name in [
        "Blent Touch",
        "Blent Touch 128",
        "\";throw Error(); //",
        "\n\0\\",
    ] {
        let rendered = script(name);
        assert!(!rendered.contains("__BLENT_DEVICE__"));
        assert!(rendered.ends_with(&format!(")({})\n", serde_json::to_string(name).unwrap())));
    }
}

struct Cinnamon(Arc<AtomicU8>);
#[zbus::interface(name = "org.Cinnamon")]
impl Cinnamon {
    async fn eval(&self, script: &str) -> (bool, String) {
        assert!(script.contains("Blent Touch"));
        match self.0.load(Ordering::SeqCst) {
            0 => (true, "true".into()),
            1 => (false, "unavailable".into()),
            _ => {
                tokio::time::sleep(Duration::from_secs(4)).await;
                (true, "true".into())
            }
        }
    }
}

#[tokio::test]
async fn t604_private_bus_acceptance_failure_and_bounded_preparation() {
    if std::env::var_os("BLENT_T604_PRIVATE_BUS").is_none() {
        let output = std::process::Command::new("dbus-run-session")
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "input::linux::cursor::tests::t604_private_bus_acceptance_failure_and_bounded_preparation", "--nocapture"])
            .env("BLENT_T604_PRIVATE_BUS", "1")
            .env("XDG_CURRENT_DESKTOP", "X-Cinnamon")
            .env("XDG_SESSION_TYPE", "x11")
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    crate::test_logging::enable();
    prepare(false, "Blent Touch").await;
    assert!(install("Blent Touch").await.is_err());
    let mode = Arc::new(AtomicU8::new(0));
    let _server = zbus::connection::Builder::session()
        .unwrap()
        .name("org.Cinnamon")
        .unwrap()
        .serve_at("/org/Cinnamon", Cinnamon(mode.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    assert!(install("Blent Touch").await.is_ok());
    prepare(true, "Blent Touch").await;
    mode.store(1, Ordering::SeqCst);
    assert!(install("Blent Touch").await.is_err());
    prepare(true, "Blent Touch").await;
    mode.store(2, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_millis(3500), prepare(true, "Blent Touch"))
        .await
        .expect("T604 desktop calls must be bounded");
}
