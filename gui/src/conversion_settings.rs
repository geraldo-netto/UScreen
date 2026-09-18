//! Linux conversion pool capacity; applied by the existing explicit restart.
use eframe::egui;
use uscreen_config::model::MAX_CONVERSION_THREADS;

pub(super) fn show(ui: &mut egui::Ui, capacity: &mut u32) {
    ui.label("Conversion capacity");
    ui.vertical(|ui| {
        let mut automatic = *capacity == 0;
        egui::ComboBox::from_id_salt("conversion-capacity")
            .selected_text(if automatic { "Auto" } else { "Manual" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut automatic, true, "Auto");
                ui.selectable_value(&mut automatic, false, "Manual");
            });
        if automatic {
            *capacity = 0;
        } else {
            *capacity = (*capacity).clamp(1, MAX_CONVERSION_THREADS);
            ui.add(egui::DragValue::new(capacity).range(1..=MAX_CONVERSION_THREADS).suffix(" participants"));
        }
        ui.label(egui::RichText::new("Auto reserves two available CPUs. Manual allows 1–128 participants per tablet, including the calling thread. Small updates use fewer workers. Apply & restart to change.").weak().size(11.0));
    });
    ui.end_row();
}
