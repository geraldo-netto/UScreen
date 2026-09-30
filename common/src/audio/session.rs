//! One direction per owner. Stop closes admission before asynchronous retirement.
use super::{
    AudioCapabilities, AudioGrant, AudioProfile, Direction, FrameReader, PcmQueue, RenderResult,
};
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioState {
    #[default]
    Stopped,
    Starting,
    Streaming,
    Stopping,
    Failed,
}

struct Active {
    grant: AudioGrant,
    reader: Option<FrameReader>,
    queue: PcmQueue,
    deadline: u64,
    failed: bool,
    last_time: u64,
}

pub struct AudioSession {
    entropy: crate::credentials::Entropy,
    direction: Direction,
    generation: u64,
    state: AudioState,
    active: Option<Active>,
}

impl AudioSession {
    pub fn new(direction: Direction) -> Self {
        Self::with_entropy(direction, crate::credentials::system_entropy)
    }

    /// Supply a CSPRNG adapter independently of platform IO and session policy.
    pub fn with_entropy(direction: Direction, entropy: crate::credentials::Entropy) -> Self {
        Self {
            entropy,
            direction,
            generation: 0,
            state: AudioState::Stopped,
            active: None,
        }
    }

    pub fn state(&self) -> AudioState {
        self.state
    }

    /// Only an explicit user Start calls this. Settings/load/reconnect never do.
    /// Caller supplies capabilities intersected across the two native endpoints.
    pub fn start(
        &mut self,
        profile: AudioProfile,
        capabilities: AudioCapabilities,
        authenticated: bool,
        now_ms: u64,
    ) -> Result<AudioGrant> {
        self.start_with_clock(profile, capabilities, authenticated, now_ms, false)
    }

    pub fn start_with_clock(
        &mut self,
        profile: AudioProfile,
        capabilities: AudioCapabilities,
        authenticated: bool,
        now_ms: u64,
        clocked: bool,
    ) -> Result<AudioGrant> {
        ensure!(self.active.is_none(), "audio retirement still pending");
        ensure!(authenticated, "authenticated tablet required");
        ensure!(
            profile.direction == self.direction,
            "wrong audio session direction"
        );
        capabilities.validate(profile)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("audio generation exhausted"))?;
        let deadline = now_ms
            .checked_add(5_000)
            .ok_or_else(|| anyhow::anyhow!("invalid audio clock"))?;
        let mut grant = AudioGrant::new(profile, generation, self.entropy)?;
        grant.clocked = clocked;
        self.active = Some(Active {
            grant: grant.clone(),
            reader: None,
            queue: PcmQueue::new(profile)?,
            deadline,
            failed: false,
            last_time: now_ms,
        });
        self.generation = generation;
        self.state = AudioState::Starting;
        Ok(grant)
    }

    fn active(&mut self, generation: u64) -> Result<&mut Active> {
        ensure!(generation == self.generation, "stale audio callback");
        self.active
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("audio session stopped"))
    }

    /// Invalid/stale negotiation never acquires the session or resets its deadline.
    pub fn connected(&mut self, generation: u64, hello: &[u8], now_ms: u64) -> Result<()> {
        ensure!(self.state == AudioState::Starting, "audio is not starting");
        let active = self.active(generation)?;
        ensure!(
            now_ms >= active.last_time && now_ms < active.deadline,
            "audio handshake clock/deadline invalid"
        );
        let reader = active.grant.authenticate(hello)?;
        let deadline = now_ms
            .checked_add(250)
            .ok_or_else(|| anyhow::anyhow!("invalid audio clock"))?;
        active.last_time = now_ms;
        active.reader = Some(reader);
        active.deadline = deadline;
        self.state = AudioState::Streaming;
        Ok(())
    }

    /// Explicit user Stop for the current direction. Native callbacks must use
    /// cancel(generation, failed) so stale failures cannot stop a replacement.
    /// Returned generation identifies native resources to retire. Repeated Stop
    /// remains idempotent; no replacement can start until retirement completes.
    pub fn stop(&mut self, failed: bool) -> Option<u64> {
        let active = self.active.as_mut()?;
        active.queue.clear();
        active.reader = None;
        active.failed |= failed;
        self.state = AudioState::Stopping;
        Some(active.grant.generation)
    }

    /// Generation-bound permission/focus loss, disconnect or adapter failure.
    pub fn cancel(&mut self, generation: u64, failed: bool) -> Option<u64> {
        self.active(generation).ok()?;
        self.stop(failed)
    }

    pub fn retired(&mut self, generation: u64) -> bool {
        if self.state != AudioState::Stopping || generation != self.generation {
            return false;
        }
        let failed = self.active.take().is_some_and(|active| active.failed);
        self.state = if failed {
            AudioState::Failed
        } else {
            AudioState::Stopped
        };
        true
    }

    /// Adapter polls using its local monotonic clock even when no bytes arrive.
    pub fn tick(&mut self, now_ms: u64) -> Option<u64> {
        if !matches!(self.state, AudioState::Starting | AudioState::Streaming) {
            return None;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| now_ms >= active.deadline || now_ms < active.last_time)
        {
            return self.stop(true);
        }
        if let Some(active) = self.active.as_mut() {
            active.last_time = now_ms;
        }
        None
    }

    /// Complete, bounded frame from the authenticated connection. Invalid input
    /// closes this generation immediately; stale callbacks cannot stop a new one.
    pub fn receive(&mut self, generation: u64, bytes: &[u8], now_ms: u64) -> Result<()> {
        ensure!(
            self.state == AudioState::Streaming,
            "audio is not streaming"
        );
        let active = self.active(generation)?;
        let result = Self::enqueue(active, bytes, now_ms);
        if result.is_err() {
            self.stop(true);
        }
        result
    }

    fn enqueue(active: &mut Active, bytes: &[u8], now_ms: u64) -> Result<()> {
        ensure!(
            now_ms >= active.last_time && now_ms < active.deadline,
            "audio receive clock/deadline invalid"
        );
        let reader = active
            .reader
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("audio is not authenticated"))?;
        let block = reader.decode(bytes)?;
        active.queue.push(block, now_ms)?;
        active.last_time = now_ms;
        active.deadline = now_ms
            .checked_add(250)
            .ok_or_else(|| anyhow::anyhow!("invalid audio clock"))?;
        Ok(())
    }

    /// Silence is written even for stale callbacks, stopped sessions or bad sizes.
    pub fn render(
        &mut self,
        generation: u64,
        now_ms: u64,
        output: &mut [i16],
    ) -> Result<RenderResult> {
        output.fill(0);
        ensure!(
            self.state == AudioState::Streaming,
            "audio is not streaming"
        );
        self.active(generation)?;
        self.tick(now_ms);
        ensure!(
            self.state == AudioState::Streaming,
            "audio receive deadline expired"
        );
        self.active(generation)?.queue.render(now_ms, output)
    }

    pub fn adjust_drift(
        &mut self,
        generation: u64,
        source_frames: u64,
        destination_frames: u64,
    ) -> Result<i32> {
        ensure!(
            self.state == AudioState::Streaming,
            "audio is not streaming"
        );
        self.active(generation)?
            .queue
            .adjust_drift(source_frames, destination_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t736_entropy_failure_preserves_generation_for_retry() {
        let direction = Direction::Microphone;
        let profile = AudioProfile::new(direction);
        let capabilities = AudioCapabilities {
            microphone: true,
            speech: true,
            ..Default::default()
        };
        let mut session =
            AudioSession::with_entropy(direction, |_| anyhow::bail!("entropy offline"));
        assert!(session.start(profile, capabilities, true, 0).is_err());
        assert_eq!(session.generation, 0);
        assert_eq!(session.state(), AudioState::Stopped);
        assert!(session.active.is_none());
        session.entropy = crate::credentials::tests::entropy;
        assert_eq!(
            session
                .start(profile, capabilities, true, 0)
                .unwrap()
                .generation(),
            1
        );
    }

    #[test]
    fn t717_generation_exhaustion_and_absent_owner_fail_closed() {
        let mut session = AudioSession::new(Direction::Microphone);
        assert!(session.active(0).is_err());
        session.generation = u64::MAX;
        let capabilities = AudioCapabilities {
            microphone: true,
            speech: true,
            ..Default::default()
        };
        assert!(session
            .start(
                AudioProfile::new(Direction::Microphone),
                capabilities,
                true,
                0
            )
            .is_err());
        assert_eq!(session.state(), AudioState::Stopped);
    }
}
