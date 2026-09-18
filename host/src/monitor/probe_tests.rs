//! T413: unknown package observations cannot replace an attached controller.
use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn t413_package_probe_requires_known_exit_and_response_shape() {
    use std::os::unix::process::ExitStatusExt;
    for (code, stdout, stderr, expected) in [
        (0, "package:/base.apk\npackage:/split.apk\n", "", Some(true)),
        (0, "", "", Some(false)),
        (1, "", "", Some(false)),
        (1, "", "error: offline", None),
        (9, "", "", None),
        (0, "error: unavailable", "", None),
        (0, "package:/base.apk\nerror: partial response", "", None),
        (1, "package:/base.apk", "", None),
    ] {
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        };
        assert_eq!(
            crate::package_presence(&output),
            expected,
            "T413: {code} {stdout} {stderr}"
        );
    }
}

fn package_adb(root: &std::path::Path) -> PathBuf {
    let path = root.join("adb");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$4" = getprop ]; then echo "$2-identity"; exit 0; fi
if [ "$4" = pm ]; then
    if [ "$2" = OLD ] && [ -f "$0.fail" ]; then
        echo 'error: transport unavailable' >&2
        exit 1
    fi
    if [ "$2" = OLD ] && [ -f "$0.absent" ]; then exit 1; fi
    echo package:/data/app/uscreen/base.apk
fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

async fn refresh(state: &mut Monitor) {
    state
        .discovery
        .refresh(vec!["OLD".into(), "NEW".into()], &state.config.adb);
    for _ in 0..2 {
        let (serial, identity) =
            tokio::time::timeout(Duration::from_secs(2), state.discovery.next())
                .await
                .unwrap()
                .unwrap();
        state.probe_ready(serial, identity);
    }
    state.reconcile().await;
}

#[tokio::test]
async fn t413_unknown_package_probe_preserves_assignment_and_geometry() {
    let root = tempfile::tempdir().unwrap();
    let adb = package_adb(root.path());
    let mut state = super::tests::fixture();
    state.config.adb = adb.to_str().unwrap().into();
    state.config.extra.max_tablets = 1;
    state.current = Some("OLD".into());
    state.ready.insert("OLD".into());
    let prepared = session::Spec {
        capture: Default::default(),
        ports: (0, 0),
        token: None,
        devices: (false, false, false),
    }
    .prepare(watch::channel(false).0);
    state.config.tablet = prepared.tablet.clone();
    state.config.tablet.begin(Some("OLD-identity".into()));
    refresh(&mut state).await;
    prepared.settings.send_modify(|s| {
        s.width = 1280;
        s.height = 800;
        s.geometry_ready = true;
    });
    let accepted = state.config.tablet.lease();
    std::fs::write(adb.with_extension("fail"), "").unwrap();
    refresh(&mut state).await;
    let current = state.current.clone();
    let remained = accepted.apply(|| {});
    let geometry_ready = prepared.settings.borrow().geometry_ready;
    state.stop().await;
    assert_eq!(
        current.as_deref(),
        Some("OLD"),
        "T413: unknown is not package absence"
    );
    assert!(
        remained,
        "T413: transient query retired accepted controller"
    );
    assert!(
        geometry_ready,
        "T413: transient query invalidated negotiated geometry"
    );
}

#[tokio::test]
async fn t413_unknown_initial_probe_is_not_eligible_and_absence_is_confirmed() {
    let root = tempfile::tempdir().unwrap();
    let adb = package_adb(root.path());
    let mut discovery = discovery::Discovery::new();
    std::fs::write(adb.with_extension("fail"), "").unwrap();
    discovery.refresh(vec!["OLD".into()], adb.to_str().unwrap());
    discovery.next().await.unwrap();
    assert!(discovery.eligible().is_empty());
    std::fs::remove_file(adb.with_extension("fail")).unwrap();
    discovery.refresh(vec!["OLD".into()], adb.to_str().unwrap());
    discovery.next().await.unwrap();
    assert_eq!(discovery.eligible(), ["OLD"]);
    std::fs::write(adb.with_extension("absent"), "").unwrap();
    discovery.refresh(vec!["OLD".into()], adb.to_str().unwrap());
    discovery.next().await.unwrap();
    assert!(discovery.eligible().is_empty());
    discovery.stop().await;
}
