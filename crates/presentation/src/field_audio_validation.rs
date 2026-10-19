//! Exercise the production field mixer without an audio device or discarded requests.
use super::{Assets, Control, Frames, RATE};
use anyhow::{Context, Result, ensure};
use resonance_game::{
    clock::{UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR},
    field::FieldSession,
};
use resonance_playback::Decodable;
use std::sync::Arc;

pub(crate) struct Playback {
    pub(super) control: Control,
    pub(super) frames: Frames,
    updates: u64,
    peak: f32,
    pub(crate) commands: Vec<(u32, u64, resonance_events::AudioCommand)>,
}
impl Playback {
    pub(crate) fn new(assets: impl Into<Arc<Assets>>, field: &mut FieldSession) -> Self {
        let (source, control) = assets.into().session();
        field.voice_feedback = true;
        Self {
            control,
            frames: source.decoder(),
            updates: 0,
            peak: 0.,
            commands: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn enter(&mut self, assets: Arc<Assets>, field: &mut FieldSession) -> Result<()> {
        self.control.leave_field()?;
        self.control.enter_field(assets)?;
        field.voice_feedback = true;
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
                .push((field.events.tick(), self.frames.frame, command.clone()));
            self.control.send(command)?;
        }
        self.advance()
    }

    pub(super) fn advance(&mut self) -> Result<()> {
        self.updates += 1;
        let end = self.updates * u64::from(RATE) * UPDATE_RATE_DENOMINATOR / UPDATE_RATE_NUMERATOR;
        while self.frames.frame < end {
            self.frame()?;
        }
        self.control.acknowledge_frames(self.frames.frame);
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
        // Consume queued commands and reach the next score block so newly
        // admitted cues execute before their owners retire.
        let block = self.frames.stream_block.len() as u64;
        let admitted = self.frames.frame.next_multiple_of(block) + 1;
        while self.frames.frame < admitted {
            self.frame()?;
        }
        if self.frames.battle.is_some() {
            self.frames
                .battle_command(0, super::battle::Command::End(false))?;
        }
        self.frames.music = None;
        self.frames.sounds.clear();

        let voice_frames = self.frames.voice.as_ref().map_or(0, |voice| {
            let length = (voice.clip.sample_count() as u64 / voice.clip.channels as u64
                * u64::from(RATE))
            .div_ceil(u64::from(voice.clip.rate));
            length.saturating_sub(voice.frame) + 1
        });
        // Dropping a score drains its submitted block and native release.
        let score_frames = block + u64::from(resonance_audio::RELEASE_FRAMES);
        let retirement = self.frames.frame + voice_frames.max(score_frames);
        while self.frames.frame < retirement {
            self.frame()?;
        }
        ensure!(
            self.frames.voice.is_none(),
            "field voice exceeded its declared duration"
        );
        while !self.frames.studio.is_silent() {
            self.frame()?;
        }
        self.control.acknowledge_frames(self.frames.frame);
        self.control.check()
    }

    pub(crate) fn report(&self) -> serde_json::Value {
        let commands: Vec<_> = self.commands.iter().map(|(tick, frame, command)|
            serde_json::json!({"tick":tick,"frame":frame,"command":format!("{command:?}")})).collect();
        serde_json::json!({"commands":commands,"rendered_frames":self.frames.frame,
            "sample_rate":RATE,"peak":self.peak,"audio_device_opened":false})
    }

    pub(crate) fn voice_position(&self) -> Option<u64> {
        self.frames.voice.as_ref().map(|voice| voice.frame)
    }

    #[cfg(test)]
    pub(crate) fn control(&self) -> Control {
        self.control.clone()
    }

    #[cfg(test)]
    pub(crate) fn next_output_sample(&mut self) -> Option<f32> {
        self.frames.next()
    }

    #[cfg(test)]
    pub(crate) fn battle_state(&self) -> Result<(bool, Option<i16>)> {
        self.control.acknowledge_frames(self.frames.frame);
        self.control.check()?;
        Ok((
            self.frames.battle.is_some(),
            self.frames.music.as_ref().map(|music| music.id),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{diagnostics::Diagnostics, field_audio::MusicReverbs};
    use resonance_events::AudioCommand;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn playback(assets: Assets) -> Playback {
        let (source, control) = Arc::new(assets).session();
        Playback {
            control,
            frames: source.decoder(),
            updates: 0,
            peak: 0.,
            commands: vec![],
        }
    }

    #[test]
    fn finish_consumes_queued_long_voice_and_retires_looping_scores() -> Result<()> {
        let mut assets = Assets::silent(Diagnostics::new(true));
        assets.voice_gains.fill(1.);
        let length = RATE as usize * 12;
        assets.voices.insert(
            7,
            Arc::new(super::super::test_clip(vec![12000; length], RATE, 1)),
        );
        let (resources, mut score, tables) =
            super::super::test_score_data(resonance_audio::data::ScoreOrigin::Sequence);
        score.loop_events = score.first_events.clone();
        let score = Arc::new(resonance_audio::package::Loaded::new(
            resources,
            score,
            tables,
            assets.reverbs,
        )?);
        assets.music.insert(0, score);
        assets.sounds.insert(1, super::super::test_score());
        assets.music_reverbs = Some(MusicReverbs {
            presets: assets.reverbs,
            selectors: vec![0],
        });
        let mut playback = playback(assets);
        // Admit at a non-block boundary: queued scores must reach their worker.
        playback.frame()?;
        let complete = Arc::new(AtomicBool::new(false));
        playback
            .control
            .send(AudioCommand::Music(resonance_events::MusicCommand::Play(0)))?;
        playback.control.send(AudioCommand::Sound {
            id: 1,
            pan: 64,
            volume: 127,
            slot: Some(0),
        })?;
        playback.control.send(AudioCommand::Voice {
            resource: 7,
            completion: Some(complete.clone()),
        })?;
        playback.finish()?;
        assert!(complete.load(Ordering::Acquire));
        assert!(playback.frames.voice.is_none());
        assert!(playback.frames.music.is_none() && playback.frames.sounds.is_empty());
        assert!(playback.frames.frame >= length as u64);
        assert!(playback.frames.frame < length as u64 + u64::from(RATE));
        assert!(playback.peak > 0.);
        Ok(())
    }

    #[test]
    fn finish_renders_delayed_reverb_after_sources_are_gone() -> Result<()> {
        let mut assets = Assets::silent(Diagnostics::new(true));
        assets.reverbs = [[0.7, 1., 0.05, 0.6, 0.1]; 2];
        let mut playback = playback(assets);
        playback
            .frames
            .synth
            .release([[0; 2], [100000, -100000], [0; 2]], 0);
        assert!(playback.frames.sounds.is_empty() && playback.frames.voice.is_none());
        playback.finish()?;
        assert!(
            playback.peak > 0.,
            "queued auxiliary return was never rendered"
        );
        assert!(playback.frames.studio.is_silent());
        assert_eq!(playback.frames.frame()?.unwrap(), [0.; 2]);
        Ok(())
    }

    #[test]
    fn finish_checks_commands_even_without_an_active_source() -> Result<()> {
        let mut playback = playback(Assets::silent(Diagnostics::new(true)));
        playback.control.send(AudioCommand::voice(99))?;
        assert!(playback.finish().is_err());
        Ok(())
    }
}
