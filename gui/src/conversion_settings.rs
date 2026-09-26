//! Linux conversion pool capacity; applied by the existing explicit restart.
use blent_config::model::MAX_CONVERSION_THREADS;
use eframe::egui;

pub(super) fn show(ui: &mut egui::Ui, capacity: &mut u32) {
    capacity_control(ui, "Conversion capacity", "conversion-capacity", capacity, " participants", "Auto reserves two available CPUs. Manual allows 1–128 participants per tablet, including the calling thread. Small updates use fewer workers. Apply & restart to change.");
}

pub(super) fn encoder_workers(ui: &mut egui::Ui, capacity: &mut u32) {
    capacity_control(ui, "Software encoder workers", "encoder-workers", capacity, " workers", "Auto compares 1, 2 and 4 workers with tablet feedback, keeping one worker as fallback. Manual fixes 1–128 workers for software H.264. More workers can increase CPU, memory and latency. Apply & restart to change.");
}

fn capacity_control(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    capacity: &mut u32,
    unit: &str,
    help: &str,
) {
    ui.label(label);
    ui.vertical(|ui| {
        let mut automatic = *capacity == 0;
        egui::ComboBox::from_id_salt(id)
            .selected_text(if automatic { "Auto" } else { "Manual" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut automatic, true, "Auto");
                ui.selectable_value(&mut automatic, false, "Manual");
            });
        if automatic {
            *capacity = 0;
        } else {
            *capacity = (*capacity).clamp(1, MAX_CONVERSION_THREADS);
            ui.add(
                egui::DragValue::new(capacity)
                    .range(1..=MAX_CONVERSION_THREADS)
                    .suffix(unit),
            );
        }
        ui.label(egui::RichText::new(help).weak().size(11.0));
    });
    ui.end_row();
}
