//! Native numbered world scenes have their own clock and retire their caller.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CameraKey,
    overworld::{Cinematic, CinematicAudio},
};
use resonance_events::{AudioCommand, SceneDestination};
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct Playback {
    pub id: u16,
    pub definition: Arc<Cinematic>,
    pub following: SceneDestination,
    ticks: u32,
}
impl Playback {
    pub fn new(id: u16, definition: Arc<Cinematic>, following: SceneDestination) -> Result<Self> {
        ensure!((513..=526).contains(&id), "invalid numbered world scene");
        ensure!(
            definition.camera.len() >= 2,
            "missing world cinematic camera"
        );
        definition.presentation.validate(&definition.actors)?;
        Ok(Self {
            id,
            definition,
            following,
            ticks: 0,
        })
    }
    pub fn ticks(&self) -> u32 {
        self.ticks.min(self.duration())
    }
    pub fn seconds(&self) -> f32 {
        self.ticks() as f32 / 60.
    }
    pub fn camera(&self) -> CameraKey {
        // The native player samples twice at 0.25 authored frames per 60-Hz tick.
        let frame = self.ticks as f32 * 0.5;
        let keys = &self.definition.camera;
        let end = keys
            .partition_point(|key| key.time < frame)
            .min(keys.len() - 1);
        let a = &keys[end.saturating_sub(1)];
        let b = &keys[end];
        let fraction = if a.time == b.time {
            0.
        } else {
            ((frame - a.time) / (b.time - a.time)).clamp(0., 1.)
        };
        CameraKey {
            time: frame,
            position: std::array::from_fn(|i| {
                a.position[i] + fraction * (b.position[i] - a.position[i])
            }),
            target: std::array::from_fn(|i| a.target[i] + fraction * (b.target[i] - a.target[i])),
        }
    }
    fn duration(&self) -> u32 {
        (self.definition.camera.last().unwrap().time * 2.).ceil() as u32
    }
    pub fn fade(&self) -> (f32, bool) {
        let (alpha, white) = self
            .definition
            .presentation
            .fade
            .sample(self.ticks, self.duration());
        (f32::from(alpha) / 255., white)
    }
    pub fn dialogue(&self) -> Option<(&resonance_content::overworld::CinematicDialogue, f32)> {
        let line = self
            .definition
            .dialogue
            .iter()
            .rev()
            .find(|line| self.ticks >= line.tick)?;
        let elapsed = self.ticks - line.tick;
        if elapsed + 1 >= u32::from(line.duration) {
            return None;
        }
        let remaining = u32::from(line.duration) - elapsed;
        let alpha = if remaining < 16 {
            remaining * 16
        } else {
            ((elapsed + 1) * 16).min(255)
        };
        Some((line, alpha as f32 / 255.))
    }
    pub fn initial_audio(&self) -> Vec<AudioCommand> {
        self.definition
            .presentation
            .initial_audio
            .iter()
            .map(audio_command)
            .collect()
    }
    pub fn step(&mut self, audio: &mut Vec<AudioCommand>) -> bool {
        self.ticks += 1;
        let complete =
            self.ticks >= self.duration() + self.definition.presentation.fade.end_ticks();
        if self.ticks > self.duration() {
            return complete;
        }
        for before_dialogue in [true, false] {
            if !before_dialogue {
                audio.extend(
                    self.definition
                        .dialogue
                        .iter()
                        .filter(|line| line.tick == self.ticks)
                        .map(|line| AudioCommand::Voice {
                            resource: line.voice,
                            completion: None,
                        }),
                );
            }
            audio.extend(
                self.definition
                    .presentation
                    .audio
                    .iter()
                    .filter(|event| {
                        event.tick == self.ticks && event.before_dialogue == before_dialogue
                    })
                    .map(|event| audio_command(&event.action)),
            );
        }
        if self.ticks == self.duration() {
            audio.extend(
                self.definition
                    .presentation
                    .end_audio
                    .iter()
                    .map(audio_command),
            );
        }
        complete
    }
}

fn audio_command(action: &CinematicAudio) -> AudioCommand {
    match *action {
        CinematicAudio::Sound {
            id,
            pan,
            volume,
            slot,
            repeat: true,
        } => AudioCommand::RepeatSound {
            id,
            pan,
            volume,
            slot: slot.expect("validated cinematic loop slot"),
        },
        CinematicAudio::Sound {
            id,
            pan,
            volume,
            slot,
            repeat: false,
        } => AudioCommand::Sound {
            id,
            pan,
            volume,
            slot,
        },
        CinematicAudio::Volume { slot, volume } => AudioCommand::SoundVolume {
            slot: u16::from(slot),
            volume,
        },
        CinematicAudio::Stop { slot } => AudioCommand::StopSound(u16::from(slot)),
    }
}

impl super::Session {
    pub fn play_cinematic(
        assets: Arc<super::Assets>,
        request: &resonance_events::WorldTransition,
        definition: Arc<Cinematic>,
        persistent: resonance_events::PersistentState,
        play_time: crate::clock::PlayTime,
    ) -> Result<Self> {
        ensure!(
            definition.world == assets.world,
            "wrong world cinematic terrain"
        );
        let playback = Playback::new(
            request.location,
            definition,
            request
                .following
                .clone()
                .context("world cinematic continuation missing")?,
        )?;
        let state = super::travel::State {
            world: assets.world,
            position: super::Position::from_map([38400., 28800., 0.])?,
            heading: 0.,
            camera_yaw: 0.,
            alternate_perspective: false,
            map_display: Default::default(),
            mount: super::travel::Mount::Foot,
            altitude: 0.,
        };
        let mut session = Self::construct(assets, state, persistent, play_time, false)?;
        session.events.world.input_enabled = false;
        session.events.world.audio_commands = playback.initial_audio();
        session.cinematic = Some(playback);
        Ok(session)
    }
    pub(super) fn step_cinematic(&mut self) -> Result<()> {
        let playback = self.cinematic.as_mut().unwrap();
        self.events.world.tick += 1;
        if !playback.step(&mut self.events.world.audio_commands) {
            return Ok(());
        }
        if playback.id == 516 {
            self.events.set_global(20, 1)?;
            self.events
                .world
                .request_world(517, 0, Some(playback.following.clone()))
                .map_err(anyhow::Error::msg)?;
        } else {
            if playback.id == 518 {
                let mut state = self
                    .events
                    .world
                    .party
                    .as_ref()
                    .and_then(|p| p.travel.overworld.clone())
                    .unwrap_or_else(|| self.travel.state().clone());
                state.mount = super::travel::Mount::Rheairds;
                state.altitude = state.position.map()[2] + self.assets.movement.flight_clearance;
                self.events
                    .world
                    .party
                    .as_mut()
                    .context("cinematic party missing")?
                    .travel
                    .overworld = Some(state);
            }
            self.events
                .world
                .request_destination(playback.following.clone())
                .map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}
