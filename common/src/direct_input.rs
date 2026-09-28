//! T689: non-stylus event contract and owned input lifecycle, without OS APIs.
use crate::input_mapping::{Mapping, Snapshot};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Touch,
    DirectMouse,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub monitor: String,
    pub mode: Mode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Right,
    Middle,
}
impl Button {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Middle => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Touch {
        x: f64,
        y: f64,
        slot: u8,
        phase: Phase,
    },
    Mouse {
        x: f64,
        y: f64,
        button: Option<Button>,
        phase: Phase,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contact {
    pub slot: u8,
    pub position: (i32, i32),
    pub phase: Phase,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Move,
    Down(Button),
    Up(Button),
}

/// Native implementations must retain ownership on failure and release it on drop.
pub trait Adapter {
    fn touch(&mut self, frame: &[Contact]) -> Result<()>;
    fn mouse(&mut self, point: (u16, u16), action: MouseAction) -> Result<()>;
    fn retire(&mut self) -> Result<()>;
}

pub struct Session<A: Adapter> {
    adapter: A,
    mode: Mode,
    mapping: Mapping,
    contacts: [Option<(i32, i32)>; 10],
    buttons: [bool; 3],
    active: bool,
}
impl<A: Adapter> Session<A> {
    pub fn new(adapter: A, config: &Config, snapshot: &Snapshot) -> Result<Self> {
        Ok(Self {
            adapter,
            mode: config.mode,
            mapping: snapshot.select(&config.monitor)?,
            contacts: [None; 10],
            buttons: [false; 3],
            active: true,
        })
    }
    pub fn apply(&mut self, snapshot: &Snapshot, event: Event) -> Result<()> {
        ensure!(self.active, "Input session retired");
        if !self.mapping.current(snapshot) {
            self.retire()?;
            anyhow::bail!("Monitor changed; input session retired");
        }
        match event {
            Event::Touch { x, y, slot, phase } => self.touch(snapshot, x, y, slot, phase),
            Event::Mouse {
                x,
                y,
                button,
                phase,
            } => self.mouse(snapshot, x, y, button, phase),
        }
    }
    fn touch(&mut self, snapshot: &Snapshot, x: f64, y: f64, slot: u8, phase: Phase) -> Result<()> {
        ensure!(self.mode == Mode::Touch, "Touch input is disabled");
        let point = self.mapping.project(snapshot, x, y)?;
        ensure!(
            usize::from(slot) < self.contacts.len(),
            "Invalid contact slot"
        );
        let old = self.contacts[usize::from(slot)];
        ensure!(
            old.is_none() == (phase == Phase::Down),
            "Invalid contact transition"
        );
        let point = if matches!(phase, Phase::Up | Phase::Cancel) {
            old.unwrap()
        } else {
            point
        };
        let mut frame: Vec<_> = self
            .contacts
            .iter()
            .enumerate()
            .filter_map(|(index, position)| {
                position.map(|position| Contact {
                    slot: index as u8,
                    position,
                    phase: Phase::Move,
                })
            })
            .collect();
        frame.retain(|contact| contact.slot != slot);
        frame.push(Contact {
            slot,
            position: point,
            phase,
        });
        if let Err(error) = self.adapter.touch(&frame) {
            return self.failed(error);
        }
        self.contacts[usize::from(slot)] =
            (!matches!(phase, Phase::Up | Phase::Cancel)).then_some(point);
        Ok(())
    }
    fn mouse(
        &mut self,
        snapshot: &Snapshot,
        x: f64,
        y: f64,
        button: Option<Button>,
        phase: Phase,
    ) -> Result<()> {
        ensure!(self.mode == Mode::DirectMouse, "Mouse input is disabled");
        let point = self.mapping.absolute(snapshot, x, y)?;
        let action = self.mouse_action(button, phase)?;
        if let Err(error) = self.adapter.mouse(point, action) {
            return self.failed(error);
        }
        if let Some(button) = button {
            self.buttons[button.index()] = phase == Phase::Down;
        }
        Ok(())
    }
    fn mouse_action(&self, button: Option<Button>, phase: Phase) -> Result<MouseAction> {
        match (button, phase) {
            (None, Phase::Move) => Ok(MouseAction::Move),
            (Some(button), Phase::Down) if !self.buttons[button.index()] => {
                Ok(MouseAction::Down(button))
            }
            (Some(button), Phase::Up | Phase::Cancel) if self.buttons[button.index()] => {
                Ok(MouseAction::Up(button))
            }
            _ => anyhow::bail!("Invalid mouse button transition"),
        }
    }
    fn failed(&mut self, error: anyhow::Error) -> Result<()> {
        let cleanup = self.retire();
        Err(error.context(format!("input retired; cleanup: {cleanup:?}")))
    }
    /// Stop, replacement and disconnect all use the same idempotent operation.
    pub fn retire(&mut self) -> Result<()> {
        self.active = false;
        self.adapter.retire()?;
        self.contacts = [None; 10];
        self.buttons = [false; 3];
        Ok(())
    }
}
impl<A: Adapter> Drop for Session<A> {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

#[cfg(test)]
mod tests;
