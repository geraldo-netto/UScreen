//! Shared access to the compositor's output inventory.
use serde_json::Value;
use uscreen_config::commands::AsyncCommandExt;

/// Outputs currently known to KWin; absent when the tool or JSON is unavailable.
pub(crate) async fn outputs() -> Option<Vec<Value>> {
    let out = tokio::process::Command::new("kscreen-doctor")
        .arg("-j")
        .output_bounded()
        .await
        .ok()?;
    let value: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(value.get("outputs")?.as_array()?.clone())
}

pub(crate) fn enabled_matching_output<'a>(
    out: &'a Value,
    names: &[String],
) -> Option<(u32, &'a str)> {
    let name = out.get("name").and_then(Value::as_str).unwrap_or("");
    if !names.iter().any(|candidate| candidate == name) {
        return None;
    }
    if !out.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let id = out.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    Some((id, name))
}
