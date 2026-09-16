//! Shared access to the compositor's output inventory.
use serde_json::Value;
use uscreen_config::commands::AsyncCommandExt;

/// Outputs currently known to KWin; absent when the tool or JSON is unavailable.
pub(crate) async fn outputs() -> Option<Vec<Value>> {
    let out = tokio::process::Command::new("kscreen-doctor")
        .arg("-j")
        .output_bounded()
        .await
        .ok()?;
    let value: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(value.get("outputs")?.as_array()?.clone())
}

pub(crate) fn enabled_matching_output<'a>(
    out: &'a Value,
    names: &[String],
) -> Option<(u32, &'a str)> {
    let name = out.get("name").and_then(Value::as_str).unwrap_or("");
    if !names.iter().any(|candidate| candidate == name) {
        return None;
    }
    if !out.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let id = out.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    Some((id, name))
}

#[derive(Clone, Copy)]
struct Geometry {
    id: u32,
    enabled: bool,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

impl Geometry {
    fn from_json(out: &Value) -> Self {
        let scale = out.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
        Self {
            id: out.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
            enabled: out.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            x: out.pointer("/pos/x").and_then(Value::as_i64).unwrap_or(0),
            y: out.pointer("/pos/y").and_then(Value::as_i64).unwrap_or(0),
            width: Self::logical_size(out, "/size/width", scale),
            height: Self::logical_size(out, "/size/height", scale),
        }
    }

    fn logical_size(out: &Value, path: &str, scale: f64) -> i64 {
        let raw = out.pointer(path).and_then(Value::as_i64).unwrap_or(0);
        if scale > 0.0 {
            (raw as f64 / scale).round() as i64
        } else {
            raw
        }
    }

    fn already_placed(self, x: i64, y: i64, shift_x: i64, shift_y: i64) -> bool {
        self.enabled && self.x == x && self.y == y && shift_x == 0 && shift_y == 0
    }
}

#[derive(Clone, Copy, Default)]
struct Bounds {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}

impl Bounds {
    fn from_output(out: Geometry) -> Self {
        Self {
            min_x: out.x,
            min_y: out.y,
            max_x: out.x + out.width,
            max_y: out.y + out.height,
        }
    }

    fn include(&mut self, out: Geometry) {
        self.min_x = self.min_x.min(out.x);
        self.min_y = self.min_y.min(out.y);
        self.max_x = self.max_x.max(out.x + out.width);
        self.max_y = self.max_y.max(out.y + out.height);
    }

    fn adjacent(self, position: crate::config::Position, tablet: Geometry) -> (i64, i64) {
        use crate::config::Position::*;
        match position {
            Right => (self.max_x, self.min_y),
            Left => (self.min_x - tablet.width, self.min_y),
            Above => (self.min_x, self.min_y - tablet.height),
            Below => (self.min_x, self.max_y),
        }
    }
}

#[derive(Default)]
struct Scene {
    tablet: Option<Geometry>,
    others: Vec<Geometry>,
    bounds: Option<Bounds>,
}

impl Scene {
    fn from_outputs(outputs: &[Value], evdi_names: &[String]) -> Self {
        let mut scene = Self::default();
        for output in outputs {
            let name = output.get("name").and_then(Value::as_str).unwrap_or("");
            let geometry = Geometry::from_json(output);
            if evdi_names.iter().any(|candidate| candidate == name) {
                scene.tablet = Some(geometry);
            } else if geometry.enabled {
                scene.add_other(geometry);
            }
        }
        scene
    }

    fn add_other(&mut self, output: Geometry) {
        if let Some(bounds) = &mut self.bounds {
            bounds.include(output);
        } else {
            self.bounds = Some(Bounds::from_output(output));
        }
        self.others.push(output);
    }

    fn placement(self, position: crate::config::Position) -> Option<Placement> {
        let tablet = self.tablet?;
        let (want_x, want_y) = self
            .bounds
            .map(|bounds| bounds.adjacent(position, tablet))
            .unwrap_or((0, 0));
        let bounds = self.bounds.unwrap_or_default();
        // KWin requires nonnegative coordinates. Normalize the entire layout
        // to the origin so changing placement remains idempotent.
        let shift_x = -want_x.min(bounds.min_x);
        let shift_y = -want_y.min(bounds.min_y);
        let (x, y) = (want_x + shift_x, want_y + shift_y);
        Some(Placement {
            id: tablet.id,
            x,
            y,
            shift_x,
            shift_y,
            already_applied: tablet.already_placed(x, y, shift_x, shift_y),
            moves: self
                .others
                .iter()
                .map(|out| (out.id, out.x + shift_x, out.y + shift_y))
                .collect(),
        })
    }
}

pub(crate) struct Placement {
    pub id: u32,
    pub x: i64,
    pub y: i64,
    pub shift_x: i64,
    pub shift_y: i64,
    pub already_applied: bool,
    moves: Vec<(u32, i64, i64)>,
}

impl Placement {
    pub fn shifts_desktop(&self) -> bool {
        self.shift_x != 0 || self.shift_y != 0
    }

    pub fn arguments(&self) -> Vec<String> {
        let mut args = vec![
            format!("output.{}.enable", self.id),
            format!("output.{}.position.{},{}", self.id, self.x, self.y),
        ];
        if self.shifts_desktop() {
            for (id, x, y) in &self.moves {
                args.push(format!("output.{id}.position.{x},{y}"));
            }
        }
        args
    }
}

pub(crate) fn placement(
    outputs: &[Value],
    evdi_names: &[String],
    position: crate::config::Position,
) -> Option<Placement> {
    Scene::from_outputs(outputs, evdi_names).placement(position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Position;
    use serde_json::json;

    fn screen(id: u32, name: &str, enabled: bool, x: i64, y: i64) -> Value {
        json!({"id": id, "name": name, "enabled": enabled, "pos": {"x": x, "y": y},
            "size": {"width": 1000, "height": 500}, "scale": 1.0})
    }

    #[test]
    fn t171_scaled_layout_preserves_all_four_positions() {
        let mut desktop = screen(1, "eDP-1", true, 0, 0);
        desktop["size"] = json!({"width": 2560, "height": 1440});
        desktop["scale"] = json!(2.0);
        let outputs = [desktop, screen(2, "DVI-I-1", false, 0, 0)];
        for (position, x, y, shift_x, shift_y) in [
            (Position::Right, 1280, 0, 0, 0),
            (Position::Left, 0, 0, 1000, 0),
            (Position::Above, 0, 0, 0, 500),
            (Position::Below, 0, 720, 0, 0),
        ] {
            let plan = placement(&outputs, &["DVI-I-1".into()], position).unwrap();
            assert_eq!(
                (plan.x, plan.y, plan.shift_x, plan.shift_y),
                (x, y, shift_x, shift_y)
            );
            assert!(!plan.already_applied);
            assert_eq!(
                plan.arguments()[..2],
                [
                    "output.2.enable".to_string(),
                    format!("output.2.position.{x},{y}")
                ]
            );
        }
    }

    #[test]
    fn t171_normalization_is_idempotent_and_preserves_other_outputs() {
        let outputs = [
            screen(1, "DVI-I-99", true, 0, 500),
            screen(2, "DVI-I-1", true, 0, 0),
        ];
        let above = placement(&outputs, &["DVI-I-1".into()], Position::Above).unwrap();
        assert!(above.already_applied);
        let right = placement(&outputs, &["DVI-I-1".into()], Position::Right).unwrap();
        assert_eq!(
            right.arguments(),
            [
                "output.2.enable",
                "output.2.position.1000,0",
                "output.1.position.0,0"
            ]
        );
        assert!(placement(&outputs, &["absent".into()], Position::Right).is_none());
        let alone = placement(&outputs[1..], &["DVI-I-1".into()], Position::Right).unwrap();
        assert!(alone.already_applied);
    }
}
