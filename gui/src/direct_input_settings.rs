//! Shared non-stylus preferences; the caller supplies native monitor inventory.
use blent_config::{
    direct_input::{Config, Mode},
    input_mapping::Monitor,
    FileConfig,
};
use eframe::egui;

pub fn show(ui: &mut egui::Ui, config: &mut FileConfig, monitors: &[Monitor]) {
    let mut enabled = config.direct_input.is_some();
    ui.checkbox(&mut enabled, "Touch/mouse input preview");
    if !enabled {
        config.direct_input = None;
        return;
    }
    let direct = config.direct_input.get_or_insert_with(|| Config {
        monitor: String::new(),
        mode: Mode::DirectMouse,
    });
    egui::ComboBox::from_label("Target monitor")
        .selected_text(&direct.monitor)
        .show_ui(ui, |ui| {
            for monitor in monitors {
                ui.selectable_value(&mut direct.monitor, monitor.id.clone(), &monitor.name);
            }
        });
    ui.horizontal(|ui| {
        ui.selectable_value(&mut direct.mode, Mode::Touch, "Touch");
        ui.selectable_value(&mut direct.mode, Mode::DirectMouse, "Mouse");
    });
    ui.checkbox(&mut config.input_touch, "Allow touch");
    ui.checkbox(&mut config.input_mouse, "Allow finger mouse");
    ui.label("Save and restart to apply. Physical tablet acceptance pending; stylus unavailable.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use blent_config::input_mapping::{Rect, Rotation};

    fn frame(
        ctx: &egui::Context,
        config: &mut FileConfig,
        monitors: &[Monitor],
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, config, monitors));
            },
        )
        .shapes
    }
    fn position(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| position(shape, label)),
            _ => None,
        }
    }
    fn click(ctx: &egui::Context, config: &mut FileConfig, monitors: &[Monitor], label: &str) {
        let shapes = frame(ctx, config, monitors, Vec::new());
        let pos = shapes
            .iter()
            .find_map(|shape| position(&shape.shape, label))
            .unwrap_or_else(|| panic!("T673: missing {label}"));
        for pressed in [true, false] {
            frame(
                ctx,
                config,
                monitors,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn t673_input_controls_select_monitor_and_preserve_independent_permissions() {
        let monitors = [Monitor {
            id: "screen".into(),
            name: "Test screen".into(),
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            rotation: Rotation::Identity,
            scale_percent: 100,
            primary: true,
        }];
        let ctx = egui::Context::default();
        let mut config = FileConfig {
            input_pen: false,
            input_pointer: false,
            ..Default::default()
        };
        click(&ctx, &mut config, &monitors, "Touch/mouse input preview");
        assert_eq!(
            config.direct_input.as_ref().unwrap().mode,
            Mode::DirectMouse
        );
        assert!(config.validate_input_mode().is_err());
        config.direct_input.as_mut().unwrap().monitor = "missing".into();
        click(&ctx, &mut config, &monitors, "missing");
        click(&ctx, &mut config, &monitors, "Test screen");
        assert_eq!(config.direct_input.as_ref().unwrap().monitor, "screen");
        click(&ctx, &mut config, &monitors, "Touch");
        assert_eq!(config.direct_input.as_ref().unwrap().mode, Mode::Touch);
        click(&ctx, &mut config, &monitors, "Allow touch");
        assert!(config.validate_input_mode().is_err());
        click(&ctx, &mut config, &monitors, "Mouse");
        assert!(config.validate_input_mode().is_ok());
        click(&ctx, &mut config, &monitors, "Allow finger mouse");
        assert!(config.validate_input_mode().is_err());
        click(&ctx, &mut config, &monitors, "Touch/mouse input preview");
        assert!(config.direct_input.is_none());
        assert!(
            !config.input_touch
                && !config.input_mouse
                && !config.input_pen
                && !config.input_pointer
        );
    }
    #[test]
    fn t673_input_preferences_render_and_roundtrip_without_pen_dependency() {
        let monitor = Monitor {
            id: "screen".into(),
            name: "Test screen".into(),
            bounds: Rect {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            },
            rotation: Rotation::Identity,
            scale_percent: 100,
            primary: true,
        };
        let ctx = egui::Context::default();
        for mode in [None, Some(Mode::Touch), Some(Mode::DirectMouse)] {
            let mut config = FileConfig {
                direct_input: mode.map(|mode| Config {
                    monitor: monitor.id.clone(),
                    mode,
                }),
                input_pen: false,
                input_pointer: false,
                ..Default::default()
            };
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, &mut config, std::slice::from_ref(&monitor))
                });
            });
            let root = tempfile::tempdir().unwrap();
            let store = blent_config::storage::ConfigStore::new(root.path().join("input.toml"));
            store.save_edits(&config, &FileConfig::default()).unwrap();
            assert_eq!(store.load(), config);
        }
    }
}
