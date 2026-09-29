//! Bounded v1 PCM framing on a session-authenticated connection, not encryption.
use super::{AudioProfile, Direction, BLOCK_FRAMES, MAX_SAMPLES, SAMPLE_RATE};
use crate::credentials::{random_token, token_matches};
use anyhow::{ensure, Result};

pub const HELLO_BYTES: usize = 92;
pub const FRAME_HEADER_BYTES: usize = 28;
const MAGIC: &[u8; 8] = b"BLAUD001";

/// Secret-bearing grant. Never log, serialize into configuration or reuse it.
#[derive(Clone)]
pub struct AudioGrant {
    pub(super) profile: AudioProfile,
    pub(super) generation: u64,
    token: String,
}

impl AudioGrant {
    pub(super) fn new(profile: AudioProfile, generation: u64) -> Result<Self> {
        Ok(Self {
            profile,
            generation,
            token: random_token()?,
        })
    }

    pub fn profile(&self) -> AudioProfile {
        self.profile
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Deliver only through the already-authenticated control connection.
    pub fn hello(&self) -> [u8; HELLO_BYTES] {
        let mut bytes = [0; HELLO_BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..72].copy_from_slice(self.token.as_bytes());
        bytes[72..80].copy_from_slice(&self.generation.to_be_bytes());
        bytes[80] = self.profile.direction as u8;
        bytes[81] = self.profile.direction.channels() as u8;
        bytes[82..86].copy_from_slice(&SAMPLE_RATE.to_be_bytes());
        bytes[86..88].copy_from_slice(&(BLOCK_FRAMES as u16).to_be_bytes());
        bytes[88] = self.profile.processing as u8;
        bytes[89] = u8::from(self.profile.background);
        bytes[90..92].copy_from_slice(&self.profile.buffer_ms.to_be_bytes());
        bytes
    }

    /// Fixed-length negotiation before reading/allocating any PCM payload.
    pub fn authenticate(&self, bytes: &[u8]) -> Result<FrameReader> {
        ensure!(bytes.len() == HELLO_BYTES, "invalid audio handshake length");
        let token = std::str::from_utf8(&bytes[8..72])?;
        ensure!(
            token_matches(&self.token, token),
            "audio authentication failed"
        );
        let expected = self.hello();
        ensure!(
            bytes[..8] == expected[..8] && bytes[72..] == expected[72..],
            "audio negotiation mismatch"
        );
        Ok(FrameReader {
            generation: self.generation,
            direction: self.profile.direction,
            previous: None,
        })
    }

    /// Sequence starts at zero and cannot wrap. Timestamp is sender monotonic µs.
    pub fn encode(&self, sequence: u64, timestamp_us: u64, samples: &[i16]) -> Result<Vec<u8>> {
        ensure!(sequence < u64::MAX, "audio sequence exhausted");
        ensure!(
            samples.len() == BLOCK_FRAMES * self.profile.direction.channels(),
            "invalid PCM block size"
        );
        let mut bytes = vec![0; FRAME_HEADER_BYTES + samples.len() * 2];
        bytes[..8].copy_from_slice(&self.generation.to_be_bytes());
        bytes[8..16].copy_from_slice(&sequence.to_be_bytes());
        bytes[16..24].copy_from_slice(&timestamp_us.to_be_bytes());
        bytes[24..26].copy_from_slice(&(samples.len() as u16 * 2).to_be_bytes());
        bytes[26] = self.profile.direction as u8;
        for (sample, output) in samples.iter().zip(bytes[28..].chunks_exact_mut(2)) {
            output.copy_from_slice(&sample.to_le_bytes());
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct PcmBlock {
    pub(super) samples: [i16; MAX_SAMPLES],
    pub(super) channels: usize,
    pub discontinuity: bool,
}

impl PcmBlock {
    pub fn samples(&self) -> &[i16] {
        &self.samples[..BLOCK_FRAMES * self.channels]
    }
}

pub struct FrameReader {
    generation: u64,
    direction: Direction,
    previous: Option<(u64, u64)>,
}

impl FrameReader {
    /// Check the entire fixed header before allocating or waiting for payload.
    /// Adapter must use an exact, bounded read and retire the connection on error.
    pub fn payload_bytes(&self, header: &[u8]) -> Result<usize> {
        ensure!(
            header.len() == FRAME_HEADER_BYTES,
            "invalid audio frame header"
        );
        ensure!(
            header[..8] == self.generation.to_be_bytes(),
            "stale audio generation"
        );
        ensure!(
            header[26] == self.direction as u8 && header[27] == 0,
            "invalid audio frame flags/direction"
        );
        let size = u16::from_be_bytes(header[24..26].try_into()?) as usize;
        ensure!(
            size == BLOCK_FRAMES * self.direction.channels() * 2,
            "invalid audio payload size"
        );
        self.sequence(header)?;
        Ok(size)
    }

    fn sequence(&self, header: &[u8]) -> Result<(u64, u64, bool)> {
        let sequence = u64::from_be_bytes(header[8..16].try_into()?);
        let timestamp = u64::from_be_bytes(header[16..24].try_into()?);
        ensure!(sequence < u64::MAX, "audio sequence exhausted");
        let discontinuity = if let Some((last, time)) = self.previous {
            ensure!(
                sequence > last && timestamp > time,
                "replayed or reordered audio"
            );
            sequence != last + 1
        } else {
            ensure!(sequence == 0, "audio must start at sequence zero");
            false
        };
        Ok((sequence, timestamp, discontinuity))
    }

    /// No state advances until header and complete payload both validate.
    pub fn decode(&mut self, bytes: &[u8]) -> Result<PcmBlock> {
        ensure!(bytes.len() >= FRAME_HEADER_BYTES, "truncated audio frame");
        let size = self.payload_bytes(&bytes[..FRAME_HEADER_BYTES])?;
        ensure!(
            bytes.len() == FRAME_HEADER_BYTES + size,
            "truncated or oversized audio payload"
        );
        let (sequence, timestamp, discontinuity) = self.sequence(&bytes[..FRAME_HEADER_BYTES])?;
        let mut block = PcmBlock {
            samples: [0; MAX_SAMPLES],
            channels: self.direction.channels(),
            discontinuity,
        };
        for (output, input) in block.samples.iter_mut().zip(bytes[28..].chunks_exact(2)) {
            *output = i16::from_le_bytes([input[0], input[1]]);
        }
        self.previous = Some((sequence, timestamp));
        Ok(block)
    }
}
