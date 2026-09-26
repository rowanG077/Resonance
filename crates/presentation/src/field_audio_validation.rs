//! Exercise the production field mixer without an audio device or discarded requests.
use super::{Assets, Control, Frames, RATE};
use anyhow::{Context, Result, ensure};
use resonance_game::{
    clock::{UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR},
    field::FieldSession,
};
use resonance_playback::Decodable;
use std::sync::{Arc, atomic::Ordering};

pub(crate) struct Playback {
    control: Control,
    frames: Frames,
    updates: u64,
    peak: f32,
    commands: Vec<serde_json::Value>,
}
impl Playback {
    pub(crate) fn new(assets: Assets, field: &mut FieldSession) -> Self {
        let (source, control) = assets.session();
        field.voice_feedback = Some(Arc::new(control.clone()));
        Self {
            control,
            frames: source.decoder(),
            updates: 0,
            peak: 0.,
            commands: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn enter(&mut self, assets: Assets, field: &mut FieldSession) -> Result<()> {
        self.control.leave_field()?;
        self.control.enter_field(assets)?;
        field.voice_feedback = Some(Arc::new(self.control.clone()));
        Ok(())
    }

    pub(crate) fn step(&mut self, field: &mut FieldSession) -> Result<()> {
        if let Some(party) = &field.events.world.party {
            let preferences = &party.settings.preferences;
            self.control.stereo(preferences.stereo)?;
            self.control.levels([
                preferences.volumes.music,
                preferences.volumes.effects,
                if preferences.event_voiceover {
                    preferences.volumes.voice
                } else {
                    0
                },
            ])?;
        }
        self.control.movie(field.events.world.blocked_by_movie())?;
        for command in std::mem::take(&mut field.events.world.audio_commands) {
            self.commands
                .push(serde_json::json!({"tick":field.events.tick(),
                "frame":self.frames.frame,"command":format!("{command:?}")}));
            self.control.send(command)?;
        }
        self.updates += 1;
        let end = self.updates * u64::from(RATE) * UPDATE_RATE_DENOMINATOR / UPDATE_RATE_NUMERATOR;
        while self.frames.frame < end {
            self.frame()?;
        }
        self.control
            .completions
            .lock()
            .unwrap()
            .retain(|(end, token)| {
                if *end <= self.frames.frame {
                    token.store(true, Ordering::Release);
                    false
                } else {
                    true
                }
            });
        self.control.check()
    }

    fn frame(&mut self) -> Result<()> {
        let samples = self
            .frames
            .frame()?
            .context("field audio validation stopped early")?;
        ensure!(
            samples.iter().all(|s| s.is_finite()),
            "nonfinite field audio output"
        );
        for sample in samples {
            self.peak = self.peak.max(sample.abs());
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<()> {
        // Release scene-owned loops, then execute their real envelope/reverb tails.
        for sound in &mut self.frames.sounds {
            sound.controls.release = true;
        }
        let end = self.frames.frame + u64::from(RATE) * 10;
        while !self.frames.sounds.is_empty() || self.frames.voice.is_some() {
            ensure!(
                self.frames.frame < end,
                "field audio cue did not retire within ten seconds"
            );
            self.frame()?;
        }
        self.control.check()
    }

    pub(crate) fn report(&self) -> serde_json::Value {
        serde_json::json!({"commands":self.commands,"rendered_frames":self.frames.frame,
            "sample_rate":RATE,"peak":self.peak,"audio_device_opened":false})
    }

    pub(crate) fn voice_position(&self) -> Option<u64> {
        self.frames.voice.as_ref().map(|voice| voice.frame)
    }

    #[cfg(test)]
    pub(crate) fn control(&self) -> Control {
        self.control.clone()
    }

    /// Device-free host tests consume the same mixer queue as live playback.
    #[cfg(test)]
    pub(crate) fn sample_battle(&mut self, frames: u32) -> Result<(bool, Option<i16>)> {
        for _ in 0..frames {
            self.frame()?;
        }
        self.control.acknowledge_frames(self.frames.frame);
        Ok((self.frames.battle.is_some(), self.frames.music_id))
    }
}
