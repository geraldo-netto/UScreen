//! T689: owned synthetic touch device and explicit mouse-button injection.
use crate::direct_input::{Adapter, Button, Contact, Mode, MouseAction, Phase};
use anyhow::{ensure, Result};
use std::{marker::PhantomData, mem::size_of, ptr, rc::Rc};
use windows_sys::Win32::{
    Foundation::POINT,
    UI::{
        Controls::*,
        Input::{KeyboardAndMouse::*, Pointer::*},
        WindowsAndMessaging::PT_TOUCH,
    },
};

pub struct NativeInput {
    touch: HSYNTHETICPOINTERDEVICE,
    contacts: Vec<Contact>,
    buttons: Vec<Button>,
    // Native injection stays on its creating worker thread.
    _thread: PhantomData<Rc<()>>,
}
impl NativeInput {
    pub fn new(mode: Mode) -> Result<Self> {
        let touch = if mode == Mode::Touch {
            unsafe { CreateSyntheticPointerDevice(PT_TOUCH, 10, POINTER_FEEDBACK_NONE) }
        } else {
            ptr::null_mut()
        };
        ensure!(
            mode != Mode::Touch || !touch.is_null(),
            "Windows touch device unavailable: {}",
            std::io::Error::last_os_error()
        );
        Ok(Self {
            touch,
            contacts: Vec::new(),
            buttons: Vec::new(),
            _thread: PhantomData,
        })
    }
    fn touch_frame(&self, frame: &[Contact]) -> Result<()> {
        ensure!(!self.touch.is_null(), "Touch device unavailable");
        ensure!(
            !frame.is_empty() && frame.len() <= 10,
            "Invalid touch frame size"
        );
        let mut seen = [false; 10];
        for contact in frame {
            ensure!(
                contact.slot < 10 && !seen[usize::from(contact.slot)],
                "Invalid or duplicate touch slot"
            );
            seen[usize::from(contact.slot)] = true;
        }
        let native: Vec<_> = frame.iter().map(touch_info).collect();
        ensure!(
            unsafe {
                InjectSyntheticPointerInput(self.touch, native.as_ptr(), native.len() as u32)
            } != 0,
            "Windows touch injection denied or failed: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
    fn release_touch(&mut self) -> Result<()> {
        if self.touch.is_null() {
            return Ok(());
        }
        let cancelled: Vec<_> = self
            .contacts
            .iter()
            .map(|contact| Contact {
                phase: Phase::Cancel,
                ..*contact
            })
            .collect();
        let result = if cancelled.is_empty() {
            Ok(())
        } else {
            self.touch_frame(&cancelled)
        };
        unsafe { DestroySyntheticPointerDevice(self.touch) };
        self.touch = ptr::null_mut();
        self.contacts.clear();
        result
    }
    fn release_buttons(&mut self) -> Result<()> {
        let mut errors = Vec::new();
        for button in self.buttons.clone() {
            match send_mouse(0, 0, button_flags(button, false)) {
                Ok(()) => self.buttons.retain(|owned| *owned != button),
                Err(error) => errors.push(error.to_string()),
            }
        }
        ensure!(
            errors.is_empty(),
            "Mouse button cleanup failed: {}",
            errors.join("; ")
        );
        Ok(())
    }
}
impl Adapter for NativeInput {
    fn touch(&mut self, frame: &[Contact]) -> Result<()> {
        self.touch_frame(frame)?;
        self.contacts = frame
            .iter()
            .copied()
            .filter(|contact| !matches!(contact.phase, Phase::Up | Phase::Cancel))
            .collect();
        Ok(())
    }
    fn mouse(&mut self, point: (u16, u16), action: MouseAction) -> Result<()> {
        let flags = self.mouse_flags(action)?;
        send_mouse(
            i32::from(point.0),
            i32::from(point.1),
            flags | MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )?;
        match action {
            MouseAction::Down(button) => self.buttons.push(button),
            MouseAction::Up(button) => self.buttons.retain(|owned| *owned != button),
            MouseAction::Move => (),
        }
        Ok(())
    }
    fn retire(&mut self) -> Result<()> {
        let touch = self.release_touch();
        let mouse = self.release_buttons();
        touch.and(mouse)
    }
}
impl NativeInput {
    fn mouse_flags(&self, action: MouseAction) -> Result<u32> {
        match action {
            MouseAction::Move => Ok(0),
            MouseAction::Down(button) => {
                ensure!(
                    !self.buttons.contains(&button),
                    "Mouse button already owned"
                );
                ensure!(
                    unsafe { GetAsyncKeyState(i32::from(virtual_key(button))) } >= 0,
                    "Mouse button held by another input source"
                );
                Ok(button_flags(button, true))
            }
            MouseAction::Up(button) => {
                ensure!(
                    self.buttons.contains(&button),
                    "Cannot release an unowned mouse button"
                );
                Ok(button_flags(button, false))
            }
        }
    }
}
impl Drop for NativeInput {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}
fn virtual_key(button: Button) -> u16 {
    match button {
        Button::Left => VK_LBUTTON,
        Button::Right => VK_RBUTTON,
        Button::Middle => VK_MBUTTON,
    }
}
fn button_flags(button: Button, down: bool) -> u32 {
    match (button, down) {
        (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (Button::Left, false) => MOUSEEVENTF_LEFTUP,
        (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
        (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    }
}
fn send_mouse(x: i32, y: i32, flags: u32) -> Result<()> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: x,
                dy: y,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    accepted(unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) })
}
fn accepted(count: u32) -> Result<()> {
    ensure!(count == 1, "Windows mouse injection denied or incomplete");
    Ok(())
}
fn touch_info(contact: &Contact) -> POINTER_TYPE_INFO {
    let flags = match contact.phase {
        Phase::Down => POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
        Phase::Move => POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
        Phase::Up => POINTER_FLAG_UP,
        Phase::Cancel => POINTER_FLAG_UP | POINTER_FLAG_CANCELED,
    };
    POINTER_TYPE_INFO {
        r#type: PT_TOUCH,
        Anonymous: POINTER_TYPE_INFO_0 {
            touchInfo: POINTER_TOUCH_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_TOUCH,
                    pointerId: u32::from(contact.slot) + 1,
                    pointerFlags: flags,
                    ptPixelLocation: POINT {
                        x: contact.position.0,
                        y: contact.position.1,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        },
    }
}

#[cfg(test)]
mod tests;
