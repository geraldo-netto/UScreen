//! Desktop placement policy; only KDE Wayland currently exposes this adapter.
use tokio::process::Command;
use tracing::{info, warn};
use uscreen_config::commands::AsyncCommandExt;

/// Enable the EVDI output at the configured edge of the existing desktop.
///
/// The output is identified by the DRM connector names sysfs reports for
/// EVDI cards, not by whether the name happens to contain "DVI": a real DVI
/// monitor on a dock produces exactly the same name pattern, and acting on
/// it would enable and move the user's physical screen instead of ours.
///
/// The position is derived at runtime from the existing display geometry so
/// it works regardless of the laptop's screen resolution or scaling factor.
pub(super) async fn enable_evdi_display(card: Option<u32>, position: crate::config::Position) {
    let evdi_names: Vec<String> = crate::vdisplay::evdi_connectors()
        .into_iter()
        .filter(|c| card.is_none_or(|want| c.card == want))
        .map(|c| c.name)
        .collect();
    enable_named_evdi_display(
        &evdi_names,
        position,
        crate::desktop::Desktop::current(),
        std::ffi::OsStr::new("kscreen-doctor"),
    )
    .await;
}

async fn enable_named_evdi_display(
    evdi_names: &[String],
    position: crate::config::Position,
    desktop: crate::desktop::Desktop,
    program: &std::ffi::OsStr,
) {
    if desktop != crate::desktop::Desktop::KdeWayland {
        return;
    }
    if evdi_names.is_empty() {
        warn!("No EVDI connector found in sysfs — cannot enable the virtual display");
        return;
    }

    // Retry: KWin may not have registered the new EVDI device yet.
    // Use -j (JSON) rather than the plain "-o" text listing: newer
    // kscreen-doctor versions emit ANSI color codes in "-o" output
    // unconditionally, even when piped to a non-tty, which broke the
    // old line-based "Output:"/"Geometry:" parser (it silently matched
    // nothing, since every line actually starts with an escape code).
    for attempt in 0..15 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let Some(outputs) = crate::kscreen::outputs_using(program).await else {
            continue;
        };
        let Some(plan) = crate::kscreen::placement(&outputs, evdi_names, position) else {
            continue;
        };
        if plan.already_applied {
            return;
        }
        apply_display_placement(&plan, position, program).await;
        return;
    }

    warn!("EVDI output did not appear in kscreen-doctor within 3s");
}

async fn apply_display_placement(
    plan: &crate::kscreen::Placement,
    position: crate::config::Position,
    program: &std::ffi::OsStr,
) {
    info!(
        "Enabling EVDI output.{} at ({}, {}) — {:?} of the other screens",
        plan.id, plan.x, plan.y, position
    );
    if plan.shifts_desktop() {
        info!(
            "  Shifting the other screens by ({}, {}) to keep the layout at the origin",
            plan.shift_x, plan.shift_y
        );
    }
    // Apply the whole layout in one compositor transaction.
    match Command::new(program)
        .args(plan.arguments())
        .output_bounded()
        .await
    {
        Ok(output) if output.status.success() => info!("kscreen-doctor enable+position: ok"),
        Ok(output) => warn!(
            "kscreen-doctor failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => warn!("kscreen-doctor error: {}", error),
    }
}

/// Turn the virtual output back off so its windows return to the real
/// screens. Without this the desktop keeps a monitor nobody can see after
/// the tablet is unplugged, and windows left there are effectively lost.
pub(super) async fn disable_evdi_display(card: Option<u32>) {
    let evdi_names: Vec<String> = crate::vdisplay::evdi_connectors()
        .into_iter()
        .filter(|c| card.is_none_or(|want| c.card == want))
        .map(|c| c.name)
        .collect();
    disable_named_evdi_display(
        &evdi_names,
        crate::desktop::Desktop::current(),
        std::ffi::OsStr::new("kscreen-doctor"),
    )
    .await;
}

async fn disable_named_evdi_display(
    evdi_names: &[String],
    desktop: crate::desktop::Desktop,
    program: &std::ffi::OsStr,
) {
    if desktop != crate::desktop::Desktop::KdeWayland || evdi_names.is_empty() {
        return;
    }
    let Some(outputs) = crate::kscreen::outputs_using(program).await else {
        return;
    };
    for out in &outputs {
        let Some((id, name)) = crate::kscreen::enabled_matching_output(out, evdi_names) else {
            continue;
        };
        info!("Disabling EVDI output.{} ({})", id, name);
        let _ = tokio::process::Command::new(program)
            .arg(format!("output.{}.disable", id))
            .output_bounded()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t224_placement_fixture(
        inventory: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("kscreen-fixture");
        let trace = dir.path().join("trace");
        std::fs::write(dir.path().join("inventory"), inventory).unwrap();
        std::fs::write(
            &program,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "${0%/*}/trace"
if [ "$1" = -j ]; then /bin/cat "${0%/*}/inventory"; fi
"#,
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        (dir, program, trace)
    }
    #[tokio::test]
    async fn t224_non_kde_sessions_skip_placement_commands_and_retries() {
        for (desktop, session) in [
            ("X-Cinnamon", "x11"),
            ("GNOME", "wayland"),
            ("sway", "wayland"),
            ("KDE", "x11"),
            ("", ""),
        ] {
            let (_dir, program, trace) = t224_placement_fixture(r#"{"outputs":[]}"#);
            let kind = crate::desktop::Desktop::detect(desktop, session);
            let names = ["DVI-I-1".into()];
            let started = std::time::Instant::now();
            enable_named_evdi_display(
                &names,
                crate::config::Position::Right,
                kind,
                program.as_os_str(),
            )
            .await;
            disable_named_evdi_display(&names, kind, program.as_os_str()).await;
            assert!(
                started.elapsed() < std::time::Duration::from_millis(200),
                "T224: {desktop}/{session} incurred retry delay"
            );
            assert!(!trace.exists(), "T224: {desktop}/{session} invoked KScreen");
        }
    }
    #[tokio::test]
    async fn t224_kde_wayland_still_enables_positions_and_disables_output() {
        let (_dir, program, trace) = t224_placement_fixture(
            r#"{"outputs":[
            {"id":1,"name":"eDP-1","enabled":true,"pos":{"x":0,"y":0},"size":{"width":1000,"height":500}},
            {"id":2,"name":"DVI-I-1","enabled":true,"pos":{"x":0,"y":0},"size":{"width":1280,"height":800}}]}"#,
        );
        let kind = crate::desktop::Desktop::detect("Plasma:KDE", "wayland");
        let names = ["DVI-I-1".into()];
        enable_named_evdi_display(
            &names,
            crate::config::Position::Right,
            kind,
            program.as_os_str(),
        )
        .await;
        disable_named_evdi_display(&names, kind, program.as_os_str()).await;
        assert_eq!(
            std::fs::read_to_string(trace).unwrap(),
            "-j\noutput.2.enable output.2.position.1000,0\n-j\noutput.2.disable\n"
        );
    }

    const T497_TEST: &str =
        "capture::placement::tests::t497_placement_handles_absence_retries_and_failed_commands";

    fn isolated_placement() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", T497_TEST, "--nocapture"])
            .env("USCREEN_T497_PLACEMENT", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn missing_outputs() {
        let kind = crate::desktop::Desktop::KdeWayland;
        let position = crate::config::Position::Left;
        let names = ["fixture-evdi".into()];
        let (_dir, program, trace) = t224_placement_fixture("invalid");
        enable_named_evdi_display(&[], position, kind, program.as_os_str()).await;
        assert!(!trace.exists());
        for inventory in ["invalid", r#"{"outputs":[]}"#] {
            std::fs::write(_dir.path().join("inventory"), inventory).unwrap();
            enable_named_evdi_display(&names, position, kind, program.as_os_str()).await;
        }
        assert_eq!(std::fs::read_to_string(trace).unwrap(), "-j\n".repeat(30));
        let logs = crate::test_logging::text();
        assert!(logs.contains("No EVDI connector found"));
        assert!(logs.contains("did not appear in kscreen-doctor"));
    }

    async fn failed_placement_commands() {
        let inventory = r#"{"outputs":[
            {"id":1,"name":"physical","enabled":true,"pos":{"x":0,"y":0},"size":{"width":1000,"height":500}},
            {"id":2,"name":"fixture-evdi","enabled":true,"pos":{"x":0,"y":0},"size":{"width":1280,"height":800}}]}"#;
        let (_dir, program, trace) = t224_placement_fixture(inventory);
        let names = ["fixture-evdi".into()];
        let position = crate::config::Position::Left;
        let outputs = crate::kscreen::parse(inventory.as_bytes()).unwrap();
        let plan = crate::kscreen::placement(&outputs, &names, position).unwrap();
        apply_display_placement(&plan, position, program.as_os_str()).await;
        std::fs::write(
            &program,
            "#!/bin/sh\nprintf 'fixture refused' >&2\nexit 17\n",
        )
        .unwrap();
        apply_display_placement(&plan, position, program.as_os_str()).await;
        std::fs::remove_file(&program).unwrap();
        apply_display_placement(&plan, position, program.as_os_str()).await;
        let logs = crate::test_logging::text();
        assert!(logs.contains("Shifting the other screens by (1280, 0)"));
        assert!(logs.contains("kscreen-doctor enable+position: ok"));
        assert!(logs.contains("kscreen-doctor failed: fixture refused"));
        assert!(logs.contains("kscreen-doctor error:"));
        assert_eq!(
            std::fs::read_to_string(trace).unwrap(),
            "output.2.enable output.2.position.0,0 output.1.position.1280,0\n"
        );
    }

    async fn already_placed() {
        let (_dir, program, trace) = t224_placement_fixture(
            r#"{"outputs":[
            {"id":1,"name":"physical","enabled":true,"pos":{"x":0,"y":0},"size":{"width":1000,"height":500}},
            {"id":2,"name":"fixture-evdi","enabled":true,"pos":{"x":1000,"y":0},"size":{"width":1280,"height":800}}]}"#,
        );
        enable_named_evdi_display(
            &["fixture-evdi".into()],
            crate::config::Position::Right,
            crate::desktop::Desktop::KdeWayland,
            program.as_os_str(),
        )
        .await;
        assert_eq!(std::fs::read_to_string(trace).unwrap(), "-j\n");
    }

    #[tokio::test]
    async fn t497_placement_handles_absence_retries_and_failed_commands() {
        if std::env::var_os("USCREEN_T497_PLACEMENT").is_none() {
            isolated_placement();
            return;
        }
        crate::test_logging::enable();
        missing_outputs().await;
        failed_placement_commands().await;
        already_placed().await;
    }
}
