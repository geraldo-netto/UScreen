//! Audio preferences and explicit controls; native construction stays in factory().
use blent_config::audio::{
    AudioController, AudioOptions, AudioState, AudioStatus, Direction, Processing,
};
use eframe::egui;

#[derive(Default)]
pub(super) struct Panel {
    pub(super) backend: Option<Box<dyn AudioController>>,
    error: Option<String>,
}
impl Panel {
    pub(super) fn show(&mut self, ui: &mut egui::Ui, options: &mut AudioOptions) {
        let microphone = options.profile.direction == Direction::Microphone;
        let (label, selector) = if microphone {
            ("Microphone", "input")
        } else {
            ("Speakers", "output")
        };
        ui.heading(format!("{label} sharing"));
        ui.label(format!("Select Blent {label} in your computer’s {selector} selector. Existing defaults stay unchanged."));
        let status = self
            .backend
            .as_ref()
            .map_or_else(AudioStatus::default, |b| b.status());
        ui.label(&status.detail);
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        self.buttons(ui, options, status.state);
        super::settings_grid(
            ui,
            if microphone {
                "audio-microphone"
            } else {
                "audio-speakers"
            },
            |ui| preferences(ui, options),
        );
        ui.label("Start applies these settings. Apply saves preferences; it never starts audio or restarts display sharing.");
        ui.label(if microphone { "Open Blent on the tablet for microphone permission and background consent. Raw mode may include acoustic echo." }
            else { "Open Blent on the tablet for speaker controls and background consent. Other apps may pause playback through audio focus." });
    }
    fn buttons(&mut self, ui: &mut egui::Ui, options: &AudioOptions, state: AudioState) {
        let active = matches!(
            state,
            AudioState::Starting | AudioState::Streaming | AudioState::Stopping
        );
        let label = if options.profile.direction == Direction::Microphone {
            "microphone"
        } else {
            "speakers"
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !active && cfg!(target_os = "linux"),
                    egui::Button::new(format!("Start {label}")),
                )
                .clicked()
            {
                self.error = self.start(options.clone()).err();
            }
            if ui
                .add_enabled(active, egui::Button::new(format!("Stop {label}")))
                .clicked()
            {
                if let Some(backend) = &self.backend {
                    backend.stop();
                }
            }
        });
        if active {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !cfg!(target_os = "linux") {
            ui.label("Audio backend unavailable on this operating system.");
        }
    }
    fn start(&mut self, options: AudioOptions) -> Result<(), String> {
        options.profile.validate().map_err(|e| e.to_string())?;
        if self.backend.is_none() {
            self.backend = Some(factory()?);
        }
        self.backend
            .as_ref()
            .unwrap()
            .start(options)
            .map_err(|e| format!("{e:#}"))
    }
}
fn factory() -> Result<Box<dyn AudioController>, String> {
    #[cfg(target_os = "linux")]
    return blent::audio_control::Controller::new(blent::audio::run)
        .map(|c| Box::new(c) as Box<dyn AudioController>)
        .map_err(|e| e.to_string());
    #[cfg(not(target_os = "linux"))]
    Err("Audio backend unavailable".into())
}
fn preferences(ui: &mut egui::Ui, options: &mut AudioOptions) {
    ui.label("Processing");
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut options.profile.processing,
            Processing::Speech,
            "Speech",
        );
        ui.selectable_value(&mut options.profile.processing, Processing::Raw, "Raw");
    });
    ui.end_row();
    ui.label("Application buffering");
    ui.add(egui::Slider::new(&mut options.profile.buffer_ms, 20..=200).step_by(10.0).suffix(" ms"))
        .on_hover_text("Lower means less lag; higher tolerates more jitter. Native device buffers add their own delay.");
    ui.end_row();
    ui.label("Tablet background");
    ui.checkbox(
        &mut options.profile.background,
        "Continue while hidden (requires tablet consent)",
    );
    ui.end_row();
    ui.label("Tablet serial");
    let mut serial = options.serial.clone().unwrap_or_default();
    if ui.text_edit_singleline(&mut serial).changed() {
        options.serial = (!serial.trim().is_empty()).then(|| serial.trim().to_owned());
    }
    ui.end_row();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use blent_config::audio::Direction;
    use std::{cell::RefCell, rc::Rc};
    struct Backend(Rc<RefCell<Vec<AudioOptions>>>);
    impl AudioController for Backend {
        fn start(&self, options: AudioOptions) -> blent_config::camera::BackendResult {
            self.0.borrow_mut().push(options);
            Ok(())
        }
        fn stop(&self) {
            self.0.borrow_mut().clear();
        }
        fn status(&self) -> AudioStatus {
            AudioStatus::default()
        }
    }
    #[test]
    fn t718_audio_controls_preserve_passive_preferences_and_validate_start() {
        let starts = Rc::new(RefCell::new(Vec::new()));
        let mut panel = Panel {
            backend: Some(Box::new(Backend(starts.clone()))),
            error: Some("fixture failure".into()),
        };
        let mut options = AudioOptions::new(Direction::Microphone);
        let context = egui::Context::default();
        let _ = context.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.show(ui, &mut options));
        });
        assert!(starts.borrow().is_empty());
        panel.start(options.clone()).unwrap();
        assert_eq!(starts.borrow()[0], options);
        options.profile.buffer_ms = 1;
        assert!(panel.start(options).is_err());
        drop(factory().unwrap());
    }
}
