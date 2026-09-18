//! The stock low-latency profiles emit one primary frame per IVF temporal unit.
//! Parse only OBU boundaries and the random-access prefix, not decoder syntax.
use anyhow::{ensure, Context, Result};

#[derive(Default)]
pub(super) struct Av1State {
    sequence: Vec<u8>,
    reduced: bool,
}

struct Obu<'a> {
    kind: u8,
    bytes: &'a [u8],
    payload: &'a [u8],
}

impl Av1State {
    pub(super) fn prepare(&mut self, data: &mut Vec<u8>) -> Result<Option<bool>> {
        let mut rest = data.as_slice();
        let mut frame = None;
        let mut sequence_here = false;
        let mut delimiter_end = 0;
        let mut count = 0;
        while !rest.is_empty() {
            count += 1;
            ensure!(count <= 4096, "Too many AV1 OBUs");
            let obu = read_obu(&mut rest)?;
            match obu.kind {
                1 => {
                    ensure!(
                        frame.is_none() && !sequence_here,
                        "Misordered AV1 sequence header"
                    );
                    self.set_sequence(&obu)?;
                    sequence_here = true;
                }
                2 => {
                    ensure!(
                        count == 1 && obu.payload.is_empty(),
                        "Invalid AV1 temporal delimiter"
                    );
                    delimiter_end = obu.bytes.len();
                }
                3 | 6 => {
                    ensure!(
                        frame.is_none(),
                        "Multiple AV1 frames in one temporal unit are unsupported"
                    );
                    frame = Some(self.keyframe(obu.payload)?);
                }
                _ => {}
            }
        }
        if frame == Some(true) && !sequence_here {
            self.repeat_sequence(data, delimiter_end)?;
        }
        Ok(frame)
    }

    fn set_sequence(&mut self, obu: &Obu<'_>) -> Result<()> {
        ensure!(obu.bytes.len() <= 65536, "AV1 sequence header too large");
        let prefix = *obu.payload.first().context("Empty AV1 sequence header")?;
        ensure!(prefix >> 5 == 0, "Only AV1 Main profile is supported");
        let reduced = prefix & 8 != 0;
        ensure!(
            !reduced || prefix & 16 != 0,
            "Invalid AV1 still-picture header"
        );
        self.sequence = obu.bytes.to_vec();
        self.reduced = reduced;
        Ok(())
    }

    fn keyframe(&self, payload: &[u8]) -> Result<bool> {
        ensure!(
            !self.sequence.is_empty(),
            "AV1 frame before sequence header"
        );
        let prefix = *payload.first().context("Empty AV1 frame header")?;
        // show_existing_frame, frame_type (2 bits), show_frame (MSB first).
        Ok(self.reduced || prefix & 0xf0 == 0x10)
    }

    fn repeat_sequence(&self, data: &mut Vec<u8>, offset: usize) -> Result<()> {
        ensure!(
            data.len() + self.sequence.len() <= crate::video_queue::MAX_FRAME_BYTES,
            "AV1 join packet too large"
        );
        // Delimiter must remain first; repeating the exact sequence OBU lets a
        // newly created decoder join a later keyframe without old reference data.
        data.splice(offset..offset, self.sequence.iter().copied());
        Ok(())
    }
}

fn read_obu<'a>(input: &mut &'a [u8]) -> Result<Obu<'a>> {
    let original = *input;
    let header = take_byte(input)?;
    ensure!(header & 0x83 == 2, "Invalid AV1 low-overhead OBU header");
    if header & 4 != 0 {
        ensure!(
            take_byte(input)? == 0,
            "Layered AV1 streams are unsupported"
        );
    }
    let length = read_leb(input)?;
    ensure!(length <= input.len(), "Truncated AV1 OBU payload");
    let (payload, tail) = input.split_at(length);
    *input = tail;
    Ok(Obu {
        kind: (header >> 3) & 15,
        bytes: &original[..original.len() - tail.len()],
        payload,
    })
}

fn take_byte(input: &mut &[u8]) -> Result<u8> {
    let (&byte, tail) = input.split_first().context("Truncated AV1 OBU header")?;
    *input = tail;
    Ok(byte)
}

fn read_leb(input: &mut &[u8]) -> Result<usize> {
    let mut value = 0u64;
    for shift in (0..56).step_by(7) {
        let byte = take_byte(input)?;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            ensure!(
                value <= crate::video_queue::MAX_FRAME_BYTES as u64,
                "AV1 OBU too large"
            );
            return Ok(value as usize);
        }
    }
    anyhow::bail!("Invalid AV1 OBU length")
}

#[cfg(test)]
mod tests;
