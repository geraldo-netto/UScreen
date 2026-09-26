//! Desktop-specific mapping adapters, independent of wire authentication.
use super::linux::{DeviceIdentity, KWIN_INPUT_IFACE};
use blent_config::commands::AsyncCommandExt;
use tracing::{info, warn};
mod x11;
use x11::{x11_active_outputs, x11_target_output};

pub(super) async fn primary_non_evdi_output() -> Option<String> {
    let evdi: Vec<String> = crate::vdisplay::evdi_connectors()
        .into_iter()
        .map(|c| c.name)
        .collect();
    primary_physical_output(&crate::kscreen::outputs().await?, &evdi)
}

pub(super) fn primary_physical_output(
    outputs: &[crate::kscreen::Output],
    evdi: &[String],
) -> Option<String> {
    let mut fallback = None;
    for o in outputs {
        let name = o.name.clone()?;
        if evdi.contains(&name) || !o.enabled {
            continue;
        }
        if o.primary {
            return Some(name);
        }
        fallback.get_or_insert(name);
    }
    fallback
}

pub(super) async fn kwin_device_property(sysname: &str, property: &str) -> Option<String> {
    crate::kwin::get_property(
        &format!("/org/kde/KWin/InputDevice/{}", sysname),
        KWIN_INPUT_IFACE,
        property,
    )
    .await
}

/// Pin the virtual input devices to the virtual display.
///
/// An absolute-positioning device is meaningless without knowing which screen
/// it addresses. Left unmapped, libinput spreads it across the whole desktop:
/// touching the middle of the tablet lands the cursor somewhere on the laptop
/// panel, and drawing with the pen goes to the wrong monitor entirely.
///
/// This is done over KWin's D-Bus interface rather than by writing kcminputrc.
/// Writing the config file looks like the obvious route and does produce the
/// documented `[Libinput][vendor][product][name] OutputName=` entry, but KWin
/// does not apply it to these devices — verified by reading the property back
/// and finding it empty, both when written before and after device creation.
/// Setting the property directly takes effect immediately, and KWin persists it
/// itself.
/// `expected` is how many devices this instance actually created; the retry
/// loop stops once that many are mapped rather than assuming all three exist.
pub(super) async fn map_devices_to_output(
    pen_only: bool,
    ident: &DeviceIdentity,
    card: Option<u32>,
    expected: usize,
) {
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    map_devices_using(
        pen_only,
        ident,
        card,
        expected,
        &session_type,
        "xinput",
        "xrandr",
        None,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn map_devices_using(
    pen_only: bool,
    ident: &DeviceIdentity,
    card: Option<u32>,
    expected: usize,
    session_type: &str,
    xinput: &str,
    xrandr: &str,
    connectors: Option<&[crate::vdisplay::EvdiConnector]>,
) {
    if expected == 0 {
        return;
    }
    if session_type == "x11" {
        map_x11_devices(pen_only, ident, card, expected, xinput, xrandr, connectors).await;
        return;
    }
    let Some(output) = target_output(
        pen_only,
        card,
        std::time::Duration::from_secs(10),
        connectors,
    )
    .await
    else {
        if pen_only {
            warn!("No physical output found — pen will address the whole desktop");
        } else {
            warn!("No EVDI output found — touch and pen will address the whole desktop");
        }
        return;
    };

    // KWin registers a device slightly after uinput creates it, so retry
    // rather than racing it. The same loop also covers a mapping that KWin
    // accepted but did not keep, which is what the read-back below catches.
    for attempt in 0..20 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        let Some(devices) = crate::kwin::list_strings(
            "/org/kde/KWin/InputDevice",
            "org.kde.KWin.InputDeviceManager",
            "devicesSysNames",
        )
        .await
        else {
            warn!("KWin did not answer — input devices stay unmapped");
            return;
        };

        let mut mapped = 0;
        for sysname in devices {
            mapped += usize::from(map_kwin_device(&sysname, ident, &output).await);
        }

        if mapped >= expected {
            return;
        }
    }

    warn!("Input devices did not appear in KWin within 5s — mapping skipped");
}

pub(super) async fn map_kwin_device(sysname: &str, ident: &DeviceIdentity, output: &str) -> bool {
    let Some(name) = kwin_device_property(sysname, "name").await else {
        return false;
    };
    if !ident.owns(&name) {
        return false;
    }
    let ok = crate::kwin::set_property(
        &format!("/org/kde/KWin/InputDevice/{}", sysname),
        KWIN_INPUT_IFACE,
        "outputName",
        "s",
        output,
    )
    .await;
    match ok {
        true => {
            // A successful Set is not proof: KWin answers ok and then
            // keeps the old value when the output is not usable yet.
            // Only what reads back counts, so a lost mapping is
            // retried on the next pass instead of logged as done.
            match kwin_device_property(sysname, "outputName").await {
                Some(now) if now == output => {
                    info!("Mapped '{}' ({}) to output {}", name, sysname, output);
                    return true;
                }
                now => warn!(
                    "Mapping '{}' to {} did not take (KWin reports {:?}) — retrying",
                    name,
                    output,
                    now.unwrap_or_default()
                ),
            }
        }
        false => warn!(
            "Could not map '{}': KWin refused the outputName property",
            name
        ),
    }
    false
}

/// Xorg can expose a pen as separate pen/eraser devices. Keep tablet suffixes
/// exact: "Blent Pen 2" must never match the first tablet's "Blent Pen".
pub(super) fn x11_device_kind<'a>(name: &str, ident: &'a DeviceIdentity) -> Option<&'a str> {
    if name == ident.touch {
        return Some(&ident.touch);
    }
    if name == ident.pointer {
        return Some(&ident.pointer);
    }
    if name == ident.pen
        || name
            .strip_prefix(&ident.pen)
            .is_some_and(|suffix| suffix.starts_with(" Pen (") || suffix.starts_with(" Eraser ("))
    {
        return Some(&ident.pen);
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn map_x11_devices(
    pen_only: bool,
    ident: &DeviceIdentity,
    card: Option<u32>,
    expected: usize,
    xinput: &str,
    xrandr: &str,
    fixed_connectors: Option<&[crate::vdisplay::EvdiConnector]>,
) {
    for attempt in 0..40 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let current = crate::vdisplay::evdi_connectors();
        let connectors = fixed_connectors.unwrap_or(&current);
        let output = x11_mapping_output(pen_only, card, xrandr, connectors, attempt).await;
        let output = match output {
            Ok(Some(output)) => output,
            Ok(None) => continue,
            Err(()) => return,
        };
        let Some(devices) = x11_query(
            xinput,
            &["list", "--short"],
            "xinput",
            "xinput could not list input devices",
        )
        .await
        else {
            return;
        };
        if map_x11_list(
            &String::from_utf8_lossy(&devices.stdout),
            ident,
            xinput,
            &output,
            expected,
        )
        .await
        {
            return;
        }
    }
    warn!("X11 output or input devices not ready after 10s; check xrandr providers and xinput");
}

/// Cached resources suffice while only input devices are arriving. Missing or
/// stale EDID ownership triggers an explicit refresh at most once per second.
async fn x11_mapping_output(
    pen_only: bool,
    card: Option<u32>,
    xrandr: &str,
    connectors: &[crate::vdisplay::EvdiConnector],
    attempt: u32,
) -> Result<Option<String>, ()> {
    let cached = x11_read_target(pen_only, card, xrandr, connectors, true).await?;
    if cached.is_some() || !attempt.is_multiple_of(4) {
        return Ok(cached);
    }
    x11_read_target(pen_only, card, xrandr, connectors, false).await
}

async fn x11_read_target(
    pen_only: bool,
    card: Option<u32>,
    xrandr: &str,
    connectors: &[crate::vdisplay::EvdiConnector],
    current: bool,
) -> Result<Option<String>, ()> {
    let args: &[&str] = if current {
        &["--current", "--prop"]
    } else {
        &["--prop"]
    };
    let report = x11_query(
        xrandr,
        args,
        "xrandr",
        "xrandr could not query this X11 session",
    )
    .await
    .ok_or(())?;
    let text = String::from_utf8_lossy(&report.stdout);
    Ok(
        x11_target_output(pen_only, &x11_active_outputs(&text), connectors, card)
            .map(str::to_owned),
    )
}

pub(super) async fn x11_query(
    program: &str,
    args: &[&str],
    tool: &str,
    failure: &str,
) -> Option<std::process::Output> {
    let Ok(output) = tokio::process::Command::new(program)
        .args(args)
        .output_bounded()
        .await
    else {
        warn!("X11 input mapping needs {}", tool);
        return None;
    };
    if !output.status.success() {
        warn!("{}", failure);
        return None;
    }
    Some(output)
}

pub(super) fn x11_has_geometry(field: &str) -> bool {
    field.split_once('x').is_some_and(|(w, h)| {
        w.parse::<u32>().is_ok()
            && h.split(['+', '-'])
                .next()
                .is_some_and(|h| h.parse::<u32>().is_ok())
            && (h.contains('+') || h.contains('-'))
    })
}

pub(super) fn x11_list_entry<'a, 'b>(
    line: &'a str,
    ident: &'b DeviceIdentity,
) -> Option<(&'a str, &'a str, &'b str)> {
    let start = line.find("Blent ")?;
    let (name, rest) = line[start..].split_once("id=")?;
    let kind = x11_device_kind(name.trim(), ident)?;
    let id = rest
        .split_whitespace()
        .next()
        .filter(|s| s.parse::<u32>().is_ok())?;
    Some((id, name.trim(), kind))
}

pub(super) async fn map_x11_list(
    text: &str,
    ident: &DeviceIdentity,
    xinput: &str,
    output: &str,
    expected: usize,
) -> bool {
    let mut mapped = std::collections::HashSet::new();
    let mut failed = false;
    for line in text.lines() {
        let Some((id, name, kind)) = x11_list_entry(line, ident) else {
            continue;
        };
        let ok = tokio::process::Command::new(xinput)
            .args(["map-to-output", id, output])
            .output_bounded()
            .await
            .is_ok_and(|out| out.status.success());
        if ok {
            mapped.insert(kind);
            info!("Mapped '{}' (X11 id {}) to {}", name, id, output);
        } else {
            failed = true;
        }
    }
    !failed && mapped.len() >= expected
}

/// The output the devices should address in this mode, returned only once
/// KWin lists it as enabled.
///
/// Mapping is not something that can be done ahead of time: set `outputName`
/// while the virtual display is still being brought back and KWin keeps the
/// previous mapping, so after leaving graphics-tablet mode the pen and touch
/// would go on driving the laptop screen (issue #6). Leaving display mode has
/// the opposite problem — the physical screen is always there, but the EVDI
/// output is going away at that moment. So this polls the output list up to
/// `timeout` and only then falls back to the best name it knows, so a mapping
/// is at least attempted on a desktop where kscreen-doctor cannot answer.
pub(super) async fn target_output(
    pen_only: bool,
    card: Option<u32>,
    timeout: std::time::Duration,
    known_connectors: Option<&[crate::vdisplay::EvdiConnector]>,
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if pen_only {
            if let Some(name) = primary_non_evdi_output().await {
                return Some(name);
            }
            // No kscreen-doctor means no KDE session: nothing to wait for.
            crate::kscreen::outputs().await?;
        } else {
            let discovered = known_connectors
                .is_none()
                .then(crate::vdisplay::evdi_connectors);
            let connectors = known_connectors
                .or(discovered.as_deref())
                .unwrap_or_default();
            // Keep this tablet's assigned card, including during discovery gaps.
            let fallback = fallback_output(connectors, card);
            let Some(outputs) = crate::kscreen::outputs().await else {
                return fallback;
            };
            let enabled = enabled_named_output(&outputs, fallback.as_deref());
            if let Some(o) = enabled {
                return o.name.clone();
            }
            if tokio::time::Instant::now() >= deadline {
                if fallback.is_some() {
                    warn!(
                        "EVDI output not enabled within {:?} — mapping onto {:?} anyway",
                        timeout, fallback
                    );
                }
                return fallback;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod coverage_tests;

pub(super) fn enabled_named_output<'a>(
    outputs: &'a [crate::kscreen::Output],
    name: Option<&str>,
) -> Option<&'a crate::kscreen::Output> {
    outputs
        .iter()
        .find(|output| output.name.as_deref() == name && output.enabled)
}

pub(super) fn fallback_output(
    connectors: &[crate::vdisplay::EvdiConnector],
    card: Option<u32>,
) -> Option<String> {
    // A known card must remain ours even while its connector is absent. With
    // no card yet, wait unless there is exactly one possible connector.
    match card {
        Some(card) => connectors.iter().find(|connector| connector.card == card),
        None if connectors.len() == 1 => connectors.first(),
        None => None,
    }
    .map(|connector| connector.name.clone())
}

#[cfg(test)]
pub(super) mod x11_tests;
