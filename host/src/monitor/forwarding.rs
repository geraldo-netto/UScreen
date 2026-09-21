//! Repair missing tunnels without retiring capture, changing credentials or
//! launching Android. The per-device mutation owner also cancels these jobs
//! before a route/slot can be reassigned.
use super::*;

impl Monitor {
    pub(super) fn check_forwarding(&mut self) {
        let routes: Vec<_> = self
            .ready
            .iter()
            .filter_map(|serial| {
                self.forwarding_ports(serial)
                    .map(|ports| (serial.clone(), ports))
            })
            .collect();
        for (serial, ports) in routes {
            if is_fake_serial(&serial) || self.mutations.contains(&serial) {
                continue;
            }
            let adb = self.config.adb.clone();
            let device = serial.clone();
            self.mutations.schedule(serial, async move {
                if let Err(error) = repair(&adb, &device, ports).await {
                    warn!("Could not verify ADB forwarding for {device}: {error}; retry scheduled");
                }
                Mutation::Forwarding
            });
        }
    }

    fn forwarding_ports(&self, serial: &str) -> Option<(u16, u16)> {
        if self.retiring_routes.contains_key(serial) {
            return None;
        }
        if self.current.as_deref() == Some(serial) {
            return Some(self.config.ports);
        }
        self.extras
            .get(serial)
            .map(|session| (session.video_port, session.input_port))
    }
}

async fn repair(adb: &str, serial: &str, ports: (u16, u16)) -> Result<()> {
    let listing = tokio::process::Command::new(adb)
        .args(["-s", serial, "reverse", "--list"])
        .output_bounded()
        .await?;
    anyhow::ensure!(listing.status.success(), "ADB reverse listing failed");
    let text = std::str::from_utf8(&listing.stdout)?;
    let expected = [(APP_VIDEO_PORT, ports.0), (APP_INPUT_PORT, ports.1)];
    for (remote, local) in config::adb_reverse::missing(text, &expected)? {
        let result = tokio::process::Command::new(adb)
            .args([
                "-s",
                serial,
                "reverse",
                "--no-rebind",
                &format!("tcp:{remote}"),
                &format!("tcp:{local}"),
            ])
            .output_bounded()
            .await?;
        anyhow::ensure!(
            result.status.success(),
            "Could not restore missing ADB reverse tcp:{remote}"
        );
        info!("Restored missing ADB reverse tcp:{remote} to tcp:{local} for {serial}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_adb(root: &std::path::Path, listing: &[u8]) -> PathBuf {
        let adb = root.join("adb");
        std::fs::write(adb.with_extension("listing"), listing).unwrap();
        std::fs::write(
            &adb,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.args"
if [ "$4" = --list ]; then
    if [ -f "$0.list-fail" ]; then exit 1; fi
    cat "$0.listing"
    exit 0
fi
if [ -f "$0.rebind-fail" ]; then exit 1; fi
[ "$4" = --no-rebind ] || exit 2
"#,
        )
        .unwrap();
        std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).unwrap();
        adb
    }

    #[tokio::test]
    async fn t557_repairs_only_missing_routes_and_uses_no_rebind() {
        let root = tempfile::tempdir().unwrap();
        let adb = fake_adb(root.path(), b"UsbFfs tcp:8890 tcp:9010\n");
        repair(adb.to_str().unwrap(), "USB", (9010, 9011))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("args")).unwrap(),
            "-s USB reverse --list\n-s USB reverse --no-rebind tcp:8891 tcp:9011\n"
        );
        std::fs::write(
            adb.with_extension("listing"),
            "UsbFfs tcp:8890 tcp:9010\nUsbFfs tcp:8891 tcp:9011\n",
        )
        .unwrap();
        std::fs::remove_file(adb.with_extension("args")).unwrap();
        repair(adb.to_str().unwrap(), "USB", (9010, 9011))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(adb.with_extension("args")).unwrap(),
            "-s USB reverse --list\n"
        );
    }

    #[tokio::test]
    async fn t557_conflicts_malformed_lists_and_backend_failures_never_rebind() {
        let root = tempfile::tempdir().unwrap();
        for listing in [
            b"UsbFfs tcp:8891 tcp:2222\n".as_slice(),
            b"truncated",
            b"\xff",
        ] {
            let adb = fake_adb(root.path(), listing);
            std::fs::remove_file(adb.with_extension("args")).ok();
            assert!(repair(adb.to_str().unwrap(), "USB", (9010, 9011))
                .await
                .is_err());
            assert_eq!(
                std::fs::read_to_string(adb.with_extension("args")).unwrap(),
                "-s USB reverse --list\n"
            );
        }
        let adb = fake_adb(root.path(), b"");
        for suffix in ["list-fail", "rebind-fail"] {
            let marker = adb.with_extension(suffix);
            std::fs::write(&marker, "fail").unwrap();
            assert!(repair(adb.to_str().unwrap(), "USB", (9010, 9011))
                .await
                .is_err());
            std::fs::remove_file(marker).unwrap();
        }
        assert!(repair("/missing-t557-adb", "USB", (9010, 9011))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn t557_periodic_repair_preserves_assignment_and_joins_existing_mutations() {
        let mut state = super::super::tests::fixture();
        state.current = Some("USB".into());
        state.ready.extend(["USB".into(), "unassigned".into()]);
        assert_eq!(state.forwarding_ports("unassigned"), None);
        state
            .mutations
            .schedule("USB".into(), std::future::pending());
        state.check_forwarding();
        assert!(state.mutations.contains("USB"));
        let job = state.mutations.retire("USB").unwrap();
        assert!(matches!(job.await, Err(error) if error.is_cancelled()));
        state.check_forwarding(); // Missing adapter fails, keeping the ready assignment.
        let (serial, result) = state.mutations.next().await;
        state.mutation_ready(serial, result);
        assert!(state.ready.contains("USB"));
        state.retiring_routes.insert("USB".into(), 0);
        state.check_forwarding();
        assert!(!state.mutations.contains("USB"));
        state.stop().await;
    }

    #[tokio::test]
    async fn t557_extra_repair_uses_its_existing_slot_ports() {
        let mut state = super::super::tests::fixture();
        let session = session::Spec {
            capture: capture::CaptureConfig {
                instance: 1,
                helper_path: "/missing-t557-helper".into(),
                ..Default::default()
            },
            ports: (0, 0),
            token: None,
            devices: (false, false, false),
        }
        .prepare(state.config.extra.mode_tx.clone())
        .start(state.config.extra.shutdown_rx.clone())
        .await
        .unwrap();
        let ports = (session.video_port, session.input_port);
        state.extras.insert("EXTRA".into(), session);
        state.ready.insert("EXTRA".into());
        assert_eq!(state.forwarding_ports("EXTRA"), Some(ports));
        state.check_forwarding();
        let (serial, result) = state.mutations.next().await;
        assert_eq!(serial, "EXTRA");
        state.mutation_ready(serial, result);
        assert_eq!(state.forwarding_ports("EXTRA"), Some(ports));
        state.stop().await;
    }
}
