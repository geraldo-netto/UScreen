//! T604: Cinnamon-specific cursor policy; no input injection or saved settings.
use std::time::Duration;

fn supported(touch: bool, desktop: &str, session: &str) -> bool {
    touch
        && session == "x11"
        && desktop.split(':').any(|part| {
            part.eq_ignore_ascii_case("cinnamon") || part.eq_ignore_ascii_case("x-cinnamon")
        })
}

fn script(name: &str) -> String {
    include_str!("cinnamon_cursor.js")
        .replace("__BLENT_DEVICE__", &serde_json::to_string(name).unwrap())
}

async fn install(name: &str) -> anyhow::Result<()> {
    let connection = zbus::Connection::session().await?;
    let proxy =
        zbus::Proxy::new(&connection, "org.Cinnamon", "/org/Cinnamon", "org.Cinnamon").await?;
    let (success, result): (bool, String) = proxy.call("Eval", &(script(name),)).await?;
    anyhow::ensure!(
        success && result == "true",
        "Cinnamon cursor policy unavailable"
    );
    Ok(())
}

pub(super) async fn prepare(touch: bool, name: &str) {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if !supported(touch, &desktop, &session) {
        return;
    }
    match tokio::time::timeout(Duration::from_secs(3), install(name)).await {
        Ok(Ok(())) => {
            tracing::info!("Cinnamon mouse remains visible while Blent touch is attached")
        }
        _ => tracing::warn!("Cinnamon cursor policy unavailable; touchscreen may hide the mouse"),
    }
}

#[cfg(test)]
mod tests;
