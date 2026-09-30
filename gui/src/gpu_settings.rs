//! Portable settings UI; discovery and active-device observation belong to adapters.
use crate::{egui, FileConfig, Status};

pub(crate) fn show(ui: &mut egui::Ui, config: &mut FileConfig, status: &Status) {
    ui.label("Encoding GPU");
    ui.vertical(|ui| {
        choices(ui, &mut config.encoding_gpu, &status.gpus);
        let policy = status.gpus.policy(&config.encoding_gpu);
        if policy.encoder(&config.encoder) != config.encoder {
            ui.label("This GPU cannot serve the requested encoder; Apply uses H.264 software fallback.");
        }
        if !config.encoding_gpu.is_empty() && policy == blent_config::gpu::Policy::Software {
            ui.label("CPU policy or unavailable GPU: hardware encoding is disabled after Apply.");
        }
        ui.small("Apply & restart changes encoding only. Desktop composition and capture stay on their existing GPU.");
        ui.small("Explicit GPU selection: Linux VAAPI. NVENC and other native selectors are unsupported; Automatic preserves existing encoder behavior. Device access does not prove codec support.");
        active(ui, status);
    });
    ui.end_row();
}

fn choices(ui: &mut egui::Ui, request: &mut String, catalog: &blent_config::gpu::Catalog) {
    ui.add_enabled_ui(catalog.supported, |ui| {
        egui::ComboBox::from_id_salt("encoding-gpu")
            .selected_text(catalog.requested(request))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    request,
                    String::new(),
                    "Automatic / existing device setting",
                );
                ui.selectable_value(request, "software".into(), "CPU (software encoders)");
                for adapter in &catalog.adapters {
                    ui.add_enabled_ui(adapter.accessible, |ui| {
                        ui.selectable_value(request, adapter.id.clone(), &adapter.label);
                    })
                    .response
                    .on_hover_text(if adapter.accessible {
                        "Device access verified; encoder support is checked when used."
                    } else {
                        "Device access denied or unavailable."
                    });
                }
            });
    });
    if !catalog.supported {
        ui.label("Native GPU selection unavailable on this backend.");
    }
}

fn active(ui: &mut egui::Ui, status: &Status) {
    if status.active_gpus.is_empty() {
        ui.small("Effective GPU: not observed (stopped, starting, or backend unreported).");
    }
    for description in &status.active_gpus {
        ui.small(description);
    }
}
