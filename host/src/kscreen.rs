//! Shared access to the compositor's output inventory.
use serde_json::Value;
use uscreen_config::commands::AsyncCommandExt;

/// Preserve absent names distinctly: input mapping cannot invent a connector.
#[derive(Clone, Debug)]
pub(crate) struct Output {
    pub id: u32,
    pub name: Option<String>,
    pub enabled: bool,
    pub primary: bool,
    pub position: (i64, i64),
    pub pixel_size: (i64, i64),
    pub scale: f64,
    pub icc_profile: String,
}

impl Output {
    fn from_json(value: &Value) -> Self {
        Self {
            id: value.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            enabled: value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            // Plasma 6 serializes priority (1 is primary); retain the legacy
            // boolean schema for older inventories.
            primary: value.get("priority").and_then(Value::as_u64) == Some(1)
                || value
                    .get("primary")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            position: (integer(value, "/pos/x"), integer(value, "/pos/y")),
            pixel_size: (
                integer(value, "/size/width"),
                integer(value, "/size/height"),
            ),
            scale: value.get("scale").and_then(Value::as_f64).unwrap_or(1.0),
            icc_profile: value
                .get("iccProfilePath")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
        }
    }

    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
}

fn integer(value: &Value, path: &str) -> i64 {
    value.pointer(path).and_then(Value::as_i64).unwrap_or(0)
}

#[derive(Debug, PartialEq)]
pub(crate) enum ParseError {
    InvalidJson,
    MissingOutputs,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Output>, ParseError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ParseError::InvalidJson)?;
    let outputs = value
        .get("outputs")
        .and_then(Value::as_array)
        .ok_or(ParseError::MissingOutputs)?;
    Ok(outputs.iter().map(Output::from_json).collect())
}

pub(crate) async fn fetch_with(
    mut command: tokio::process::Command,
) -> std::io::Result<std::process::Output> {
    command.arg("-j").output_bounded().await
}

/// Preserve the existing contract: parseable output remains usable even when
/// the command exits nonzero. Consumers decide mapping/diagnostic policy.
pub(crate) async fn outputs() -> Option<Vec<Output>> {
    outputs_using(std::ffi::OsStr::new("kscreen-doctor")).await
}

pub(crate) async fn outputs_using(program: &std::ffi::OsStr) -> Option<Vec<Output>> {
    parse(
        &fetch_with(tokio::process::Command::new(program))
            .await
            .ok()?
            .stdout,
    )
    .ok()
}

pub(crate) fn enabled_matching_output<'a>(
    out: &'a Output,
    names: &[String],
) -> Option<(u32, &'a str)> {
    let name = out.label();
    if !names.iter().any(|candidate| candidate == name) {
        return None;
    }
    if !out.enabled {
        return None;
    }
    Some((out.id, name))
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
    fn from_output(out: &Output) -> Self {
        Self {
            id: out.id,
            enabled: out.enabled,
            x: out.position.0,
            y: out.position.1,
            width: Self::logical_size(out.pixel_size.0, out.scale),
            height: Self::logical_size(out.pixel_size.1, out.scale),
        }
    }

    fn logical_size(raw: i64, scale: f64) -> i64 {
        if scale > 0.0 {
            // Match KScreen's occupied logical bounds at fractional scales.
            (raw as f64 / scale).ceil() as i64
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
    fn from_outputs(outputs: &[Output], evdi_names: &[String]) -> Self {
        let mut scene = Self::default();
        for output in outputs {
            let name = output.label();
            let geometry = Geometry::from_output(output);
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
    outputs: &[Output],
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

    #[test]
    fn t372_shared_inventory_preserves_placement_and_connector_identity() {
        let outputs = parse(include_bytes!("../../testdata/kscreen-inventory.json")).unwrap();
        let names = ["DVI-I-1".into()];
        let plan = placement(&outputs, &names, Position::Right).unwrap();
        assert!(plan.already_applied);
        assert_eq!((plan.x, plan.y), (3968, 0));
        assert_eq!(
            enabled_matching_output(&outputs[2], &names),
            Some((3, "DVI-I-1"))
        );
        assert_eq!(outputs[2].icc_profile, "/fixture/sRGB.icc");
        assert!(enabled_matching_output(&outputs[3], &["HDMI-1".into()]).is_none());
    }

    #[test]
    fn t372_parser_preserves_missing_and_wrong_type_defaults() {
        for source in [b"{}".as_slice(), b"{\"outputs\":false}"] {
            assert_eq!(parse(source).err(), Some(ParseError::MissingOutputs));
        }
        assert_eq!(parse(b"not json").err(), Some(ParseError::InvalidJson));
        let outputs = parse(br#"{"outputs":[{},null,{"name":7,"enabled":"true","id":-1,"scale":0,"size":{"width":120,"height":80}}]}"#).unwrap();
        for output in &outputs {
            assert_eq!(output.name, None);
            assert!(!output.enabled);
            assert_eq!(output.id, 0);
        }
        let geometry = Geometry::from_output(&outputs[2]);
        assert_eq!((geometry.width, geometry.height), (120, 80));
        assert_eq!(outputs[0].scale, 1.0);
    }

    #[tokio::test]
    async fn t372_inventory_command_is_injectable_and_preserves_json_on_failed_exit() {
        let mut command = tokio::process::Command::new("sh");
        command.args([
            "-c",
            "test \"$1\" = -j || exit 31; printf '%s' '{\"outputs\":[]}'; exit 7",
            "inventory-fixture",
        ]);
        let response = fetch_with(command).await.unwrap();
        assert_eq!(response.status.code(), Some(7));
        assert!(parse(&response.stdout).unwrap().is_empty());
    }

    fn inventory(outputs: &[Value]) -> Vec<Output> {
        parse(&serde_json::to_vec(&json!({"outputs": outputs})).unwrap()).unwrap()
    }

    fn screen(id: u32, name: &str, enabled: bool, x: i64, y: i64) -> Value {
        json!({"id": id, "name": name, "enabled": enabled, "pos": {"x": x, "y": y},
            "size": {"width": 1000, "height": 500}, "scale": 1.0})
    }

    #[test]
    fn t298_fractional_scale_neighbors_do_not_overlap() {
        // KScreen's logicalSizeForOutputInt uses ceil for the occupied bounds.
        // 1920x1080 / 1.75 occupies 1098x618; 1280x800 occupies 732x458.
        let mut desktop = screen(1, "eDP-1", true, 0, 0);
        desktop["size"] = json!({"width": 1920, "height": 1080});
        desktop["scale"] = json!(1.75);
        let mut tablet = screen(2, "DVI-I-1", false, 0, 0);
        tablet["size"] = json!({"width": 1280, "height": 800});
        tablet["scale"] = json!(1.75);
        for (direction, x, y, shift_x, shift_y) in [
            (Position::Right, 1098, 0, 0, 0),
            (Position::Below, 0, 618, 0, 0),
            (Position::Left, 0, 0, 732, 0),
            (Position::Above, 0, 0, 0, 458),
        ] {
            let mut outputs = [desktop.clone(), tablet.clone()];
            let names = ["DVI-I-1".into()];
            let plan = placement(&inventory(&outputs), &names, direction).unwrap();
            assert_eq!(
                (plan.x, plan.y, plan.shift_x, plan.shift_y),
                (x, y, shift_x, shift_y),
                "T298: fractional-scale placement overlaps in {direction:?}"
            );
            outputs[0]["pos"] = json!({"x": shift_x, "y": shift_y});
            outputs[1]["pos"] = json!({"x": x, "y": y});
            outputs[1]["enabled"] = json!(true);
            assert!(
                placement(&inventory(&outputs), &names, direction)
                    .unwrap()
                    .already_applied
            );
        }
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
            let plan = placement(&inventory(&outputs), &["DVI-I-1".into()], position).unwrap();
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
        let above = placement(&inventory(&outputs), &["DVI-I-1".into()], Position::Above).unwrap();
        assert!(above.already_applied);
        let right = placement(&inventory(&outputs), &["DVI-I-1".into()], Position::Right).unwrap();
        assert_eq!(
            right.arguments(),
            [
                "output.2.enable",
                "output.2.position.1000,0",
                "output.1.position.0,0"
            ]
        );
        assert!(placement(&inventory(&outputs), &["absent".into()], Position::Right).is_none());
        let alone = placement(
            &inventory(&outputs[1..]),
            &["DVI-I-1".into()],
            Position::Right,
        )
        .unwrap();
        assert!(alone.already_applied);
    }
}
