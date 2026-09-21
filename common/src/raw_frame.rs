//! T418: portable raw-frame descriptor. Native handles stay in transport adapters.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

pub const MESSAGE_BYTES: usize = 96;
pub const CONTROL_BYTES: usize = 4096;
pub const SLOT_CONTROL_BYTES: usize = 32;
pub const MAX_SLOTS: u32 = 8;
pub const FREE: u32 = 0;
pub const WRITING: u32 = 1;
pub const READY: u32 = 2;
pub const READING: u32 = 3;
pub const DESCRIPTOR: u32 = 1;
pub const FRAME: u32 = 2;
pub const RELEASE: u32 = 3;
pub const HELLO: u32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawTransport {
    #[default]
    Auto,
    Fifo,
    SharedMemory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub uv_offset: u32,
    pub slot_bytes: u32,
    pub slots: u32,
    pub total_bytes: u64,
}

impl Layout {
    pub fn new(width: u32, height: u32, slots: u32) -> Result<Self> {
        ensure!(
            (2..=4096).contains(&width) && width.is_multiple_of(2),
            "invalid raw width"
        );
        ensure!(
            (2..=4096).contains(&height) && height.is_multiple_of(2),
            "invalid raw height"
        );
        ensure!((2..=MAX_SLOTS).contains(&slots), "invalid raw slot count");
        let uv_offset = width * height;
        // Zero padding permits libavcodec's documented optimized overreads.
        let slot_bytes = (uv_offset * 3 / 2 + 64).div_ceil(4096) * 4096;
        Ok(Self {
            width,
            height,
            stride: width,
            uv_offset,
            slot_bytes,
            slots,
            total_bytes: CONTROL_BYTES as u64 + u64::from(slot_bytes) * u64::from(slots),
        })
    }

    pub fn frame_bytes(self) -> usize {
        self.uv_offset as usize * 3 / 2
    }

    pub fn slot_offset(self, slot: u32) -> Result<usize> {
        ensure!(slot < self.slots, "raw slot out of bounds");
        Ok(CONTROL_BYTES + slot as usize * self.slot_bytes as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: u32,
    pub generation: u64,
    pub nonce: u64,
    pub layout: Layout,
    pub sequence: u64,
    pub slot: u32,
    pub captured_us: u64,
}

impl Message {
    pub fn encode(self) -> [u8; MESSAGE_BYTES] {
        let mut bytes = [0; MESSAGE_BYTES];
        for (offset, value) in [
            (0, 0x52435355u32),
            (4, 1),
            (8, self.kind),
            (12, 0x3231564e),
            (32, self.layout.width),
            (36, self.layout.height),
            (40, self.layout.stride),
            (44, self.layout.stride),
            (48, self.layout.uv_offset),
            (52, self.layout.slot_bytes),
            (56, self.layout.slots),
            (60, CONTROL_BYTES as u32),
            (80, self.slot),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (16, self.generation),
            (24, self.nonce),
            (64, self.layout.total_bytes),
            (72, self.sequence),
            (88, self.captured_us),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() == MESSAGE_BYTES, "invalid raw message length");
        let u32_at = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let u64_at = |offset| u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
        ensure!(
            u32_at(0) == 0x52435355 && u32_at(4) == 1,
            "unknown raw protocol"
        );
        ensure!(u32_at(12) == 0x3231564e, "unsupported raw pixel format");
        let message = Self {
            kind: u32_at(8),
            generation: u64_at(16),
            nonce: u64_at(24),
            layout: Layout::new(u32_at(32), u32_at(36), u32_at(56))?,
            sequence: u64_at(72),
            slot: u32_at(80),
            captured_us: u64_at(88),
        };
        message.validate(bytes)?;
        Ok(message)
    }

    fn validate(self, bytes: &[u8]) -> Result<()> {
        ensure!(
            (DESCRIPTOR..=HELLO).contains(&self.kind),
            "unknown raw message kind"
        );
        ensure!(
            self.generation != 0 && self.nonce != 0,
            "missing raw session identity"
        );
        self.layout.slot_offset(self.slot)?;
        ensure!(
            self.encode().as_slice() == bytes,
            "noncanonical raw layout or reserved fields"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Message {
        Message {
            kind: DESCRIPTOR,
            generation: 1,
            nonce: 2,
            layout: Layout::new(1280, 800, 4).unwrap(),
            sequence: 0,
            slot: 0,
            captured_us: 0,
        }
    }

    #[test]
    fn t418_descriptor_layout_roundtrips_with_bounded_arithmetic() {
        for width in [2, 66, 1280, 4096] {
            for height in [2, 800, 4096] {
                let layout = Layout::new(width, height, 8).unwrap();
                let message = Message {
                    layout,
                    ..message()
                };
                assert_eq!(Message::decode(&message.encode()).unwrap(), message);
                assert_eq!(
                    layout.frame_bytes(),
                    width as usize * height as usize * 3 / 2
                );
                assert!(
                    layout.slot_offset(7).unwrap() + layout.slot_bytes as usize
                        <= layout.total_bytes as usize
                );
                assert!(layout.slot_offset(8).is_err());
            }
        }
        for value in [0, 1, 3, 4097, u32::MAX] {
            assert!(Layout::new(value, 800, 4).is_err());
            assert!(Layout::new(1280, value, 4).is_err());
        }
        for slots in [0, 1, 9, u32::MAX] {
            assert!(Layout::new(1280, 800, slots).is_err());
        }
    }

    #[test]
    fn t418_raw_transport_settings_are_explicit_and_round_trip() {
        let defaults = crate::FileConfig::default();
        assert_eq!(defaults.raw_transport, RawTransport::Auto);
        assert_eq!(defaults.raw_slots, 4);
        let parsed: crate::FileConfig =
            toml::from_str("raw_transport = 'shared_memory'\nraw_slots = 2\n").unwrap();
        assert_eq!(parsed.raw_transport, RawTransport::SharedMemory);
        assert_eq!(parsed.raw_slots, 2);
        let again: crate::FileConfig = toml::from_str(&toml::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(parsed, again);
        assert!(toml::from_str::<crate::FileConfig>("raw_transport = 'invalid'").is_err());
    }

    #[test]
    fn t418_malformed_descriptors_and_bounded_mutation_fuzz_never_escape_layout() {
        let valid = message().encode();
        for length in 0..128 {
            if length != MESSAGE_BYTES {
                assert!(Message::decode(&vec![0; length]).is_err());
            }
        }
        for index in 0..MESSAGE_BYTES {
            for value in [0, 1, 127, 255] {
                let mut bytes = valid;
                bytes[index] = value;
                if let Ok(decoded) = Message::decode(&bytes) {
                    assert_eq!(decoded.encode(), bytes);
                    assert!(decoded.layout.total_bytes < 256 * 1024 * 1024);
                    decoded.layout.slot_offset(decoded.slot).unwrap();
                }
            }
        }
        assert_eq!(RawTransport::default(), RawTransport::Auto);
    }
}
