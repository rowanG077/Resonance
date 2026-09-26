//! Actor voice arbitration (71D90/71E78), dispatched by the common actor update
//! (71674). The audio host reports completion; simulation never guesses a duration.
use crate::{ActorId, Cue, SoundBinding};
use anyhow::{Result, ensure};

/// A playback instance, never reused during this battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceId(pub(crate) u64);

#[derive(Debug, Clone, Default)]
pub(crate) struct Voice {
    pub pending: Option<(SoundBinding, u8)>,
    pub secondary: Option<(SoundBinding, u8)>,
    pub priority: u8,
    pub mode: u8,
    pub latched: bool,
    pub blocked: bool,
    /// 10A8 sets this before requesting; only enabled playback consumes it.
    pub centered: bool,
    pub playing: Option<VoiceId>,
}

impl Voice {
    pub fn request(&mut self, sound: SoundBinding, priority: u8) -> bool {
        if self.blocked || self.priority > priority {
            return false;
        }
        self.pending = Some((sound, priority));
        self.secondary = None;
        self.mode = priority;
        true
    }

    /// 71C74 stores a secondary request without interrupting current playback.
    pub fn enqueue(&mut self, sound: SoundBinding, mode: u8) {
        if !self.blocked {
            self.secondary = Some((sound, mode));
        }
    }

    pub fn step(
        &mut self,
        actor: ActorId,
        position: [f32; 3],
        enabled: bool,
        next: &mut u64,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if self.playing.is_none() {
            self.priority = 0;
        }
        if self.pending.is_none()
            && self.playing.is_none()
            && let Some((sound, mode)) = self.secondary.take()
        {
            self.latched = false;
            self.pending = Some((sound, 0));
            self.mode = mode;
        }
        let Some((sound, priority)) = self.pending else {
            return Ok(());
        };
        if self.latched {
            if self.playing.is_some() && priority < self.priority {
                return Ok(());
            }
            if let Some(playback) = self.playing.take() {
                cues.push(Cue::VoiceStopped { playback });
            }
            self.latched = false;
        }
        if enabled {
            let playback = VoiceId(*next);
            *next = next
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("voice handle exhausted"))?;
            self.playing = Some(playback);
            self.priority = self.mode;
            cues.push(Cue::Voice {
                actor,
                playback,
                sound,
                position,
                centered: std::mem::take(&mut self.centered),
            });
        }
        self.latched = true;
        self.pending = None;
        Ok(())
    }
}

impl crate::Battle {
    /// Prepared result requests use the same source voice arbitration as actors.
    pub fn request_result_voice(&mut self, actor: ActorId, line: crate::VoiceLine) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Results,
            "result voice outside results"
        );
        self.actor(actor)?;
        self.voices[actor.index()].request(line.sound, 3);
        Ok(())
    }

    pub fn actor_voice_finished(&self, actor: ActorId) -> Result<bool> {
        self.actor(actor)?;
        let voice = &self.voices[actor.index()];
        Ok(voice.pending.is_none() && voice.secondary.is_none() && voice.playing.is_none())
    }

    pub(crate) fn complete_voices(&mut self, completed: &[VoiceId]) -> Result<()> {
        // A replaced playback may finish after its replacement starts. Known
        // retired handles are harmless; an unknown handle is invalid input.
        ensure!(
            completed
                .iter()
                .all(|id| id.0 > 0 && id.0 < self.next_voice),
            "unknown voice playback handle"
        );
        for voice in &mut self.voices {
            if voice.playing.is_some_and(|id| completed.contains(&id)) {
                voice.playing = None;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod casting_tests;
#[cfg(test)]
mod tests;
