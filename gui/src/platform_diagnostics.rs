//! Present shared diagnostic evidence; probes run in the cached status worker.
use eframe::egui;
use uscreen_config::{
    diagnostics::{backend_lines, Report},
    platform::Capabilities,
};

pub(crate) fn show(ui: &mut egui::Ui, report: Option<&Report>, capabilities: Capabilities) {
    let lines = match report {
        Some(report) => report.lines(),
        None => {
            ui.label("Dependency checks pending");
            backend_lines(capabilities)
        }
    };
    for line in lines {
        ui.label(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uscreen_config::diagnostics::{Dependency, State, Tool};

    fn labels(shape: &egui::Shape, output: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => output.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| labels(shape, output)),
            _ => {}
        }
    }
    #[test]
    fn t533_gui_uses_the_cli_evidence_and_keeps_pending_state_explicit() {
        let caps = Capabilities {
            camera: false,
            daemon: false,
            display: false,
            input: false,
            system_setup: false,
            autostart: false,
            pipe_capacity: false,
            conversion_pool: false,
        };
        let report = Report {
            capabilities: caps,
            tools: [
                Dependency {
                    tool: Tool::Adb,
                    path: Some("tools café/adb.exe".into()),
                    state: State::Available("36.0.0".into()),
                },
                Dependency {
                    tool: Tool::Ffmpeg,
                    path: None,
                    state: State::Missing,
                },
            ],
        };
        let ctx = egui::Context::default();
        for value in [None, Some(&report)] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1500.0),
                )),
                ..Default::default()
            };
            let frame = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, value, caps));
            });
            let mut text = Vec::new();
            for shape in frame.shapes {
                labels(&shape.shape, &mut text);
            }
            let text = text.join("\n");
            assert!(
                text.contains("Display: unavailable (unsupported)"),
                "{text}"
            );
            match value {
                Some(report) => {
                    for line in report.lines() {
                        assert!(text.contains(&line), "{text}");
                    }
                }
                None => assert!(text.contains("Dependency checks pending")),
            }
        }
    }
}
