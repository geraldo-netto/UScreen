//! Preview is a thumbnail of the backend's exported pixels, never a second capture.
use eframe::egui;
use std::sync::Arc;
use uscreen_config::camera::{CameraPreview, Lens};

#[derive(Default)]
pub(super) struct Preview {
    frame: Option<Arc<CameraPreview>>,
    texture: Option<egui::TextureHandle>,
}

impl Preview {
    pub(super) fn show(&mut self, ui: &mut egui::Ui, frame: Option<Arc<CameraPreview>>) {
        self.update(ui.ctx(), frame);
        ui.label("Preview");
        ui.vertical(|ui| {
            if let (Some(texture), Some(frame)) = (&self.texture, &self.frame) {
                ui.image((texture.id(), texture.size_vec2()));
                ui.label(match frame.lens() {
                    Lens::Front => "Front webcam output",
                    Lens::Rear => "Rear webcam output",
                });
            } else {
                ui.allocate_ui(egui::vec2(160.0, 90.0), |ui| {
                    ui.centered_and_justified(|ui| ui.label("No live video"));
                });
            }
        });
        ui.end_row();
    }

    fn update(&mut self, context: &egui::Context, frame: Option<Arc<CameraPreview>>) {
        let Some(frame) = frame.filter(|frame| frame.fresh()) else {
            self.frame = None;
            self.texture = None;
            return;
        };
        if self
            .frame
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, &frame))
        {
            return;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(frame.size(), frame.rgba());
        if let Some(texture) = &mut self.texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.texture =
                Some(context.load_texture("camera-preview", image, egui::TextureOptions::LINEAR));
        }
        self.frame = Some(frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t543_preview_uses_exported_lens_and_clears_texture_after_stop() {
        let context = egui::Context::default();
        let mut preview = Preview::default();
        for lens in [Lens::Front, Lens::Rear] {
            let frame = Arc::new(CameraPreview::new(lens, [2, 2], vec![127; 16]).unwrap());
            for _ in 0..2 {
                let _ = context.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| preview.show(ui, Some(frame.clone())));
                });
                assert_eq!(preview.frame.as_ref().unwrap().lens(), lens);
                assert_eq!(preview.texture.as_ref().unwrap().size(), [2, 2]);
            }
        }
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| preview.show(ui, None));
        });
        assert!(preview.frame.is_none() && preview.texture.is_none());
    }
}
