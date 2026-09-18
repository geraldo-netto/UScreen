//! Linux-only raw-video buffer controls; privileged host policy stays explicit.
use crate::Status;
use eframe::egui;
use uscreen_config::model::PIPE_CAPACITIES_MIB;

const TEMPORARY: &str = "sudo sysctl -w fs.pipe-max-size=8388608";
const PERSISTENT: &str = "printf 'fs.pipe-max-size = 8388608\\n' | sudo tee /etc/sysctl.d/90-uscreen-pipe.conf\nsudo sysctl -p /etc/sysctl.d/90-uscreen-pipe.conf";

pub(super) fn show(ui: &mut egui::Ui, requested: &mut u32, status: &Status) {
    ui.label("Capture pipe buffer");
    ui.vertical(|ui| {
        egui::ComboBox::from_id_salt("pipe-capacity")
            .selected_text(format!("{requested} MiB"))
            .show_ui(ui, |ui| {
                for mib in PIPE_CAPACITIES_MIB {
                    ui.selectable_value(requested, mib, format!("{mib} MiB"));
                }
            });
        ui.label(egui::RichText::new("1 MiB is the default. Larger buffers may reduce transfer work but can queue older video. Applies to each tablet without restarting the display.").weak().size(11.0));
        show_effective(ui, status);
        show_limit_help(ui, status.pipe_ceiling);
    });
    ui.end_row();
}

fn show_effective(ui: &mut egui::Ui, status: &Status) {
    if status.pipe_capacities.is_empty() {
        ui.label("Effective capacity: available while capturing");
    }
    for (instance, bytes) in &status.pipe_capacities {
        let value = bytes
            .map(size_label)
            .unwrap_or_else(|| "waiting for capture".into());
        ui.label(format!(
            "Tablet {} effective capacity: {value}",
            instance + 1
        ));
    }
    ui.label(egui::RichText::new("A smaller effective value means the request is pending or limited by Linux. A full pipe may postpone shrinking.").weak().size(11.0));
}

fn size_label(bytes: u32) -> String {
    if bytes.is_multiple_of(1048576) {
        format!("{} MiB", bytes / 1048576)
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

fn command(ui: &mut egui::Ui, text: &str) {
    ui.monospace(text);
    if ui.button("Copy command").clicked() {
        ui.ctx().copy_text(text.into());
    }
}

fn show_limit_help(ui: &mut egui::Ui, ceiling: Option<u32>) {
    ui.collapsing("How to allow larger pipes in Linux", |ui| {
        let value = ceiling.map(size_label).unwrap_or_else(|| "unknown".into());
        ui.label(format!("Current per-pipe ceiling: {value}. These commands affect all unprivileged applications, not just UScreen. Record the old value first; only raise it if below 8 MiB."));
        ui.monospace("cat /proc/sys/fs/pipe-max-size");
        ui.label("Until reboot (run in a terminal):"); command(ui, TEMPORARY);
        ui.label("Persist on systems that load /etc/sysctl.d:"); command(ui, PERSISTENT);
        ui.label("Choose a buffer size and Apply. No display restart is needed. Per-user pipe-memory limits can still refuse an increase; check effective capacity.");
        ui.label("Undo: remove /etc/sysctl.d/90-uscreen-pipe.conf if you created it, then restore the recorded value with sudo sysctl -w fs.pipe-max-size=OLD_VALUE. Existing pipes are not automatically shrunk.");
    });
}
