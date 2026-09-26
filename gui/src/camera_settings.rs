//! Portable settings/UI; only create_backend selects a native implementation.
use blent_config::camera::{CameraBackend, CameraProfile, CameraState, Lens};
use eframe::egui;

pub(super) struct Panel {
    pub(super) backend: Option<Box<dyn CameraBackend>>,
    error: Option<String>,
    preview: crate::camera_preview::Preview,
    create: fn() -> Result<Box<dyn CameraBackend>, String>,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            backend: None,
            error: None,
            preview: Default::default(),
            create: create_backend,
        }
    }
}

impl Panel {
    pub(super) fn show(&mut self, ui: &mut egui::Ui, options: &mut CameraProfile) {
        ui.label("Camera sharing");
        ui.vertical(|ui| {
            let state = self.state();
            ui.label(state_label(&state));
            if let Some(error) = &self.error { ui.colored_label(egui::Color32::RED, error); }
            self.buttons(ui, options, &state);
            ui.label(egui::RichText::new("Start/Restart camera applies these settings. Apply only saves preferences. Camera changes never restart display sharing.").weak().size(11.0));
            ui.label(egui::RichText::new("Keep this host window open while sharing. Select Blent Front or Blent Rear in your video call; only the selected lens is live.").weak().size(11.0));
        });
        ui.end_row();
        let frame = self.backend.as_ref().and_then(|backend| backend.preview());
        self.preview.show(ui, frame);
        profile(ui, options);
    }

    fn state(&self) -> CameraState {
        self.backend
            .as_ref()
            .map_or(CameraState::Stopped, |backend| backend.state())
    }

    fn buttons(&mut self, ui: &mut egui::Ui, options: &CameraProfile, state: &CameraState) {
        let active = matches!(
            state,
            CameraState::Starting
                | CameraState::Waiting
                | CameraState::Streaming
                | CameraState::Stopping
        );
        ui.horizontal(|ui| {
            let label = if active {
                "Restart camera"
            } else {
                "Start camera"
            };
            if ui
                .add_enabled(
                    blent_config::platform::capabilities().camera,
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.error = self.start(options.clone()).err();
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            if ui
                .add_enabled(active, egui::Button::new("Stop camera"))
                .clicked()
            {
                if let Some(backend) = &self.backend {
                    backend.stop();
                }
            }
        });
        if active {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        if !blent_config::platform::capabilities().camera {
            ui.label("Camera backend is not implemented on this operating system.");
        }
    }

    fn start(&mut self, options: CameraProfile) -> Result<(), String> {
        options.validate().map_err(|e| e.to_string())?;
        if self.backend.is_none() {
            self.backend = Some((self.create)()?);
        }
        self.backend
            .as_ref()
            .unwrap()
            .start(options)
            .map_err(|e| format!("{e:#}"))
    }
}

fn create_backend() -> Result<Box<dyn CameraBackend>, String> {
    #[cfg(target_os = "linux")]
    return blent::camera_control::Controller::new(blent::camera::run_controlled)
        .map(|controller| Box::new(controller) as Box<dyn CameraBackend>)
        .map_err(|e| e.to_string());
    #[cfg(not(target_os = "linux"))]
    Err("Camera backend is not implemented on this operating system".into())
}

fn state_label(state: &CameraState) -> String {
    match state {
        CameraState::Stopped => "Stopped".into(),
        CameraState::Starting => "Starting camera…".into(),
        CameraState::Waiting => {
            "Waiting for video. Open Blent on the tablet and allow camera access.".into()
        }
        CameraState::Streaming => "Camera video is reaching the host".into(),
        CameraState::Stopping => "Stopping camera…".into(),
        CameraState::Failed(error) => format!("Camera unavailable: {error}"),
    }
}

fn profile(ui: &mut egui::Ui, options: &mut CameraProfile) {
    ui.label("Lens");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut options.lens, Lens::Front, "Front");
        ui.selectable_value(&mut options.lens, Lens::Rear, "Rear");
    });
    ui.end_row();
    resolution(ui, options);
    rotation(ui, &mut options.rotation);
    ui.label("Frame rate");
    ui.add(egui::Slider::new(&mut options.fps, 5..=30).suffix(" FPS"));
    ui.end_row();
    ui.label("Camera bitrate ceiling");
    ui.add(egui::Slider::new(&mut options.bitrate, 256..=20000).suffix(" kbit/s"));
    ui.end_row();
    ui.label("Adaptive camera bitrate");
    ui.checkbox(
        &mut options.adaptive_bitrate,
        "Reduce bitrate when the route falls behind",
    );
    ui.end_row();
    ui.label("Camera bitrate floor");
    ui.add_enabled(options.adaptive_bitrate,
        egui::Slider::new(&mut options.min_bitrate, 256..=20000).suffix(" kbit/s"))
        .on_hover_text("Effective floor is capped by the requested ceiling. Lower rates may reduce picture quality.");
    ui.end_row();
    ui.label("Freshness budget");
    ui.add(egui::Slider::new(&mut options.freshness_ms, 50..=2000).suffix(" ms"))
        .on_hover_text("Lower budgets discard backlog sooner, but may freeze more often on a slow route. This is not total camera latency.");
    ui.end_row();
    ui.label("Mirror");
    ui.checkbox(&mut options.mirror, "Flip video horizontally");
    ui.end_row();
    ui.label("Tablet background");
    ui.vertical(|ui| {
        ui.checkbox(&mut options.background, "Continue while hidden or locked");
        ui.label(egui::RichText::new("Start with Blent visible on the tablet. Background sharing keeps its camera service and CPU awake; higher resolution/FPS uses more power.").weak().size(11.0));
    });
    ui.end_row();
    device(ui, options);
}

fn rotation(ui: &mut egui::Ui, rotation: &mut u16) {
    ui.label("Rotate clockwise");
    egui::ComboBox::from_id_salt("camera-rotation")
        .selected_text(format!("{rotation}°"))
        .show_ui(ui, |ui| {
            for degrees in [0, 90, 180, 270] {
                ui.selectable_value(rotation, degrees, format!("{degrees}°"));
            }
        });
    ui.end_row();
}

fn resolution(ui: &mut egui::Ui, options: &mut CameraProfile) {
    ui.label("Camera resolution");
    egui::ComboBox::from_id_salt("camera-resolution")
        .selected_text(format!("{} × {}", options.width, options.height))
        .show_ui(ui, |ui| {
            for (width, height) in [(640, 480), (1280, 720), (1920, 1080)] {
                if ui
                    .selectable_label(
                        (options.width, options.height) == (width, height),
                        format!("{width} × {height}"),
                    )
                    .clicked()
                {
                    options.width = width;
                    options.height = height;
                }
            }
        });
    ui.end_row();
}

fn device(ui: &mut egui::Ui, options: &mut CameraProfile) {
    ui.label("Tablet serial");
    let mut serial = options.serial.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut serial)
                .hint_text("Automatic (one connected tablet)")
                .desired_width(190.0),
        )
        .changed()
    {
        options.serial = if serial.trim().is_empty() {
            None
        } else {
            Some(serial.trim().into())
        };
    }
    ui.end_row();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct Fixture {
        state: Rc<RefCell<CameraState>>,
        profiles: Rc<RefCell<Vec<CameraProfile>>>,
    }
    impl CameraBackend for Fixture {
        fn start(&self, options: CameraProfile) -> blent_config::camera::BackendResult {
            self.profiles.borrow_mut().push(options);
            *self.state.borrow_mut() = CameraState::Streaming;
            Ok(())
        }
        fn stop(&self) {
            *self.state.borrow_mut() = CameraState::Stopped;
        }
        fn state(&self) -> CameraState {
            self.state.borrow().clone()
        }
        fn preview(&self) -> Option<std::sync::Arc<blent_config::camera::CameraPreview>> {
            None
        }
    }

    #[test]
    fn t543_portable_backend_contract_and_status_are_separate_from_preferences() {
        let state = Rc::new(RefCell::new(CameraState::Stopped));
        let profiles = Rc::new(RefCell::new(Vec::new()));
        let mut panel = Panel {
            backend: Some(Box::new(Fixture {
                state: state.clone(),
                profiles: profiles.clone(),
            })),
            error: None,
            ..Default::default()
        };
        assert_eq!(panel.state(), CameraState::Stopped);
        assert!(profiles.borrow().is_empty());
        assert!(panel
            .start(CameraProfile {
                fps: 0,
                ..Default::default()
            })
            .is_err());
        panel
            .start(CameraProfile {
                lens: Lens::Rear,
                background: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(profiles.borrow()[0].lens, Lens::Rear);
        assert_eq!(panel.state(), CameraState::Streaming);
        panel.backend.as_ref().unwrap().stop();
        assert_eq!(panel.state(), CameraState::Stopped);
        for state in [
            CameraState::Stopped,
            CameraState::Starting,
            CameraState::Waiting,
            CameraState::Streaming,
            CameraState::Stopping,
            CameraState::Failed("missing device".into()),
        ] {
            assert!(!state_label(&state).is_empty());
        }
        let mut failing = Panel {
            create: || Err("native runtime unavailable".into()),
            ..Default::default()
        };
        assert_eq!(
            failing.start(CameraProfile::default()).unwrap_err(),
            "native runtime unavailable"
        );
        drop(create_backend().unwrap()); // Creating the adapter starts no native camera work.
    }
}
