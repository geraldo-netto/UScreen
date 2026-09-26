//! Retry-local readback: avoid repeating RandR probes inside xinput's mapper.
use super::{x11_list_entry, x11_query, DeviceIdentity};
use blent_config::commands::AsyncCommandExt;
use std::collections::HashMap;

pub(super) struct Target {
    pub name: String,
    pub topology: String,
}

#[derive(Default)]
pub(super) struct Mappings {
    topology: String,
    // Device ID, exact owned name and the properties read after successful mapping.
    devices: HashMap<String, (String, String)>,
}

impl Mappings {
    pub fn prepare(&mut self, topology: &str, devices: &str, ident: &DeviceIdentity) {
        if self.topology != topology {
            self.devices.clear();
            self.topology = topology.into();
        }
        let present: HashMap<_, _> = devices.lines()
            .filter_map(|line| x11_list_entry(line, ident))
            .map(|(id, name, _)| (id, name)).collect();
        self.devices.retain(|id, (name, _)| present.get(id.as_str()) == Some(&name.as_str()));
    }

    pub async fn map(&mut self, program: &str, id: &str, name: &str, output: &str) -> bool {
        if let Some((_, saved)) = self.devices.get(id) {
            if properties(program, id).await.as_ref() == Some(saved) {
                return true;
            }
        }
        self.devices.remove(id);
        let ok = tokio::process::Command::new(program)
            .args(["map-to-output", id, output]).output_bounded().await
            .is_ok_and(|out| out.status.success());
        if ok {
            if let Some(readback) = properties(program, id).await {
                self.devices.insert(id.into(), (name.into(), readback));
            }
            tracing::info!("Mapped '{}' (X11 id {}) to {}", name, id, output);
        }
        ok
    }
}

async fn properties(program: &str, id: &str) -> Option<String> {
    let result = x11_query(program, &["list-props", id], "xinput", "Could not read X11 mapping properties").await?;
    let text = String::from_utf8(result.stdout).ok()?;
    valid_properties(&text).then_some(text)
}

fn property<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix(name)?.strip_prefix(" (")?;
        let (atom, value) = rest.split_once("):")?;
        atom.parse::<u32>().ok()?;
        Some(value.trim())
    })
}

fn valid_properties(text: &str) -> bool {
    if text.len() > 16_384 { return false; }
    let Some(node) = property(text, "Device Node") else { return false; };
    if !node.starts_with("\"/dev/input/") || !node.ends_with('"') { return false; }
    let Some(matrix) = property(text, "Coordinate Transformation Matrix") else { return false; };
    let values: Vec<_> = matrix.split(',').take(10).collect();
    values.len() == 9 && values.iter().all(|v| v.trim().parse::<f32>().is_ok_and(f32::is_finite))
}

#[cfg(test)]
mod tests;
