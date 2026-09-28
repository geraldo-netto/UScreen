//! T672: validated physical-pixel input mapping, independent of native monitors.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
impl Rect {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.left < self.right && self.top < self.bottom,
            "Unusable monitor geometry"
        );
        Ok(())
    }
    pub fn project(self, x: f64, y: f64) -> Result<(i32, i32)> {
        self.validate()?;
        ensure!(
            (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
            "Invalid normalized input coordinates"
        );
        let axis = |start: i32, end: i32, unit: f64| {
            (f64::from(start) + (i64::from(end) - i64::from(start) - 1) as f64 * unit).round()
                as i32
        };
        Ok((
            axis(self.left, self.right, x),
            axis(self.top, self.bottom, y),
        ))
    }
    /// Windows absolute mouse adapters use this shared virtual-desktop projection.
    pub fn normalize(self, x: i32, y: i32) -> Result<(u16, u16)> {
        self.validate()?;
        ensure!(
            (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y),
            "Input lies outside desktop bounds"
        );
        let axis = |start: i32, end: i32, pixel: i32| {
            let span = (i64::from(end) - i64::from(start) - 1).max(1);
            ((i64::from(pixel) - i64::from(start)) * 65535 / span) as u16
        };
        Ok((
            axis(self.left, self.right, x),
            axis(self.top, self.bottom, y),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Identity,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}
impl Rotation {
    pub fn project(self, x: f64, y: f64) -> (f64, f64) {
        match self {
            Self::Identity => (x, y),
            Self::Clockwise90 => (1.0 - y, x),
            Self::Clockwise180 => (1.0 - x, 1.0 - y),
            Self::Clockwise270 => (y, 1.0 - x),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    /// Desktop geometry already reflects output rotation; never scale it by DPI.
    pub bounds: Rect,
    pub rotation: Rotation,
    pub scale_percent: u32,
    pub primary: bool,
}
impl Monitor {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.id.is_empty() && self.id.len() <= 1024 && !self.id.chars().any(char::is_control),
            "Invalid monitor identity"
        );
        ensure!(
            (1..=1000).contains(&self.scale_percent),
            "Invalid monitor scale"
        );
        self.bounds.validate()
    }
    /// Normalized coordinates relative to the displayed image, as sent by Android.
    pub fn project(&self, x: f64, y: f64) -> Result<(i32, i32)> {
        self.validate()?;
        self.bounds.project(x, y)
    }
    /// For sources explicitly expressed in unrotated panel coordinates only.
    pub fn project_panel(&self, x: f64, y: f64) -> Result<(i32, i32)> {
        ensure!(
            (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
            "Invalid panel coordinates"
        );
        let (x, y) = self.rotation.project(x, y);
        self.project(x, y)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    monitors: Vec<Monitor>,
    desktop: Rect,
}
impl Snapshot {
    pub fn new(mut monitors: Vec<Monitor>) -> Result<Self> {
        ensure!(
            !monitors.is_empty() && monitors.len() <= 128,
            "Monitor inventory must contain 1–128 outputs"
        );
        monitors.sort_by(|a, b| a.id.cmp(&b.id));
        for monitor in &monitors {
            monitor.validate()?;
        }
        ensure!(
            !monitors.windows(2).any(|pair| pair[0].id == pair[1].id),
            "Duplicate monitor identity"
        );
        let desktop = monitors
            .iter()
            .skip(1)
            .fold(monitors[0].bounds, |bounds, monitor| Rect {
                left: bounds.left.min(monitor.bounds.left),
                top: bounds.top.min(monitor.bounds.top),
                right: bounds.right.max(monitor.bounds.right),
                bottom: bounds.bottom.max(monitor.bounds.bottom),
            });
        Ok(Self { monitors, desktop })
    }
    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }
    pub fn desktop(&self) -> Rect {
        self.desktop
    }
    pub fn select(&self, id: &str) -> Result<Mapping> {
        let monitor = self
            .monitors
            .iter()
            .find(|monitor| monitor.id == id)
            .context("Selected monitor is unavailable")?;
        Ok(Mapping {
            monitor: monitor.clone(),
            snapshot: self.clone(),
        })
    }
}

pub trait MonitorInventory {
    fn snapshot(&self) -> Result<Snapshot>;
}

#[derive(Clone, Debug)]
pub struct Mapping {
    monitor: Monitor,
    snapshot: Snapshot,
}
impl Mapping {
    /// Callers must release contacts/buttons before adopting a changed snapshot.
    pub fn current(&self, snapshot: &Snapshot) -> bool {
        &self.snapshot == snapshot
    }
    pub fn project(&self, snapshot: &Snapshot, x: f64, y: f64) -> Result<(i32, i32)> {
        ensure!(
            self.current(snapshot),
            "Monitor inventory changed; retire input before remapping"
        );
        self.monitor.project(x, y)
    }
    pub fn absolute(&self, snapshot: &Snapshot, x: f64, y: f64) -> Result<(u16, u16)> {
        let (x, y) = self.project(snapshot, x, y)?;
        self.snapshot.desktop.normalize(x, y)
    }
}

#[cfg(test)]
mod tests;
