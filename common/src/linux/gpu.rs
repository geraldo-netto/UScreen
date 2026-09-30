//! Linux VAAPI discovery. Stable by-path identities never become user-supplied paths.
use crate::{
    encoding::Backend,
    gpu::{Adapter, Catalog},
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Device {
    pub adapter: Adapter,
    pub node: PathBuf,
}

pub fn discover() -> Vec<Device> {
    discover_in(Path::new("/dev/dri"), Path::new("/sys/class/drm"), |node| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(node)
            .is_ok()
    })
}

pub fn catalog(devices: &[Device]) -> Catalog {
    Catalog {
        supported: true,
        adapters: devices
            .iter()
            .map(|device| device.adapter.clone())
            .collect(),
    }
}

fn discover_in(dri: &Path, sys: &Path, accessible: impl Fn(&Path) -> bool) -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(dri.join("by-path")) else {
        return Vec::new();
    };
    let database = pci_database(&[
        Path::new("/usr/share/misc/pci.ids"),
        Path::new("/usr/share/hwdata/pci.ids"),
    ]);
    let mut devices: Vec<_> = entries
        .take(256)
        .flatten()
        .filter_map(|entry| device(&entry.path(), dri, sys, &database, &accessible))
        .collect();
    devices.sort_by(|a, b| a.adapter.id.cmp(&b.adapter.id));
    devices.dedup_by(|a, b| a.adapter.id == b.adapter.id);
    devices
}

fn device(
    alias: &Path,
    dri: &Path,
    sys: &Path,
    database: &str,
    accessible: &impl Fn(&Path) -> bool,
) -> Option<Device> {
    let identity = alias.file_name()?.to_str()?;
    if !identity.ends_with("-render") || identity.len() > 200 {
        return None;
    }
    let node = std::fs::canonicalize(alias).ok()?;
    if node.parent()? != std::fs::canonicalize(dri).ok()? {
        return None;
    }
    let name = node.file_name()?.to_str()?;
    name.strip_prefix("renderD")?.parse::<u32>().ok()?;
    let event = std::fs::read_to_string(sys.join(name).join("device/uevent")).unwrap_or_default();
    let label = device_label(identity, &event, database);
    Some(Device {
        adapter: Adapter {
            id: format!("vaapi:{identity}"),
            label,
            backend: Backend::Vaapi,
            accessible: accessible(&node),
        },
        node,
    })
}

fn device_label(identity: &str, event: &str, database: &str) -> String {
    let pci = event
        .lines()
        .find_map(|line| line.strip_prefix("PCI_ID="))
        .unwrap_or("unknown");
    let vendor = match pci.split(':').next().unwrap_or("") {
        "1002" => "AMD",
        "8086" => "Intel",
        "10DE" => "NVIDIA",
        _ => "GPU",
    };
    let driver = event
        .lines()
        .find_map(|line| line.strip_prefix("DRIVER="))
        .unwrap_or("unknown driver");
    let product = pci_product(database, pci).unwrap_or(pci);
    format!("{vendor} {product} · {driver} · {identity}")
}

fn pci_database(paths: &[&Path]) -> String {
    use std::io::Read;
    let mut text = String::new();
    if let Some(file) = paths.iter().find_map(|path| std::fs::File::open(path).ok()) {
        let _ = file.take(4 * 1024 * 1024).read_to_string(&mut text);
    }
    text
}

fn pci_product<'a>(database: &'a str, pci: &str) -> Option<&'a str> {
    let (vendor, device) = pci.split_once(':')?;
    let vendor = format!("{}  ", vendor.to_ascii_lowercase());
    let device = format!("\t{}  ", device.to_ascii_lowercase());
    let mut lines = database
        .lines()
        .skip_while(|line| !line.starts_with(&vendor));
    lines.next()?;
    lines
        .take_while(|line| line.starts_with(['\t', '#']) || line.is_empty())
        .find_map(|line| line.strip_prefix(&device))
}

/// Observations are current owned FFmpeg arguments plus opened render descriptors.
/// No presentation/throughput claim is derived from a process being alive.
pub fn active(daemon: u32, fifos: &[PathBuf], devices: &[Device]) -> Vec<String> {
    if daemon == 0 {
        return Vec::new();
    }
    super::processes::same_user_processes_named("ffmpeg")
        .unwrap_or_default()
        .iter()
        .filter_map(|process| observe(Path::new("/proc"), process, daemon, fifos, devices))
        .collect()
}

fn observe(
    proc: &Path,
    process: &super::processes::Process,
    daemon: u32,
    fifos: &[PathBuf],
    devices: &[Device],
) -> Option<String> {
    let root = proc.join(process.pid.to_string());
    let stat = std::fs::read_to_string(root.join("stat")).ok()?;
    let parent: u32 = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    if parent != daemon
        || !fifos
            .iter()
            .any(|fifo| process.has_path_argument("-i", fifo))
    {
        return None;
    }
    let encoder = process
        .arguments
        .windows(2)
        .find(|pair| pair[0] == "-c:v")?[1]
        .to_str()?;
    let backend = crate::encoding::find(encoder)?.backend;
    let detail = effective_device(&root, backend, devices);
    Some(format!(
        "Active encoder: {encoder} · {detail} (presentation not measured here)"
    ))
}

fn effective_device(process: &Path, backend: Backend, devices: &[Device]) -> String {
    if matches!(backend, Backend::X264 | Backend::Vpx | Backend::Aom) {
        return "CPU / software".into();
    }
    let opened: Vec<_> = std::fs::read_dir(process.join("fd"))
        .into_iter()
        .flatten()
        .take(256)
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .collect();
    if backend == Backend::Vaapi {
        if let Some(device) = devices.iter().find(|device| opened.contains(&device.node)) {
            return format!("opened GPU: {}", device.adapter.label);
        }
    }
    "effective GPU unreported / awaiting device evidence".into()
}

#[cfg(test)]
mod tests;
