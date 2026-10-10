//! Native numbered world scenes have their own clock and retire their caller.
use anyhow::{Context, Result, ensure};
use resonance_content::{CameraKey, overworld::Cinematic};
use resonance_events::{AudioCommand, SceneDestination};
use std::sync::Arc;

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
    fn fade_rate(&self) -> u32 {
        if matches!(self.id, 513..=515 | 518 | 520) {
            4
        } else {
            8
        }
    }
    pub fn fade(&self) -> (f32, bool) {
        let white = matches!(self.id, 516 | 517 | 520);
        if self.ticks > self.duration() {
            return (
                (self.ticks - self.duration())
                    .saturating_mul(self.fade_rate())
                    .min(255) as f32
                    / 255.,
                white,
            );
        }
        if self.id == 521 && (299..=302).contains(&self.ticks) {
            return (
                if matches!(self.ticks, 300 | 301) {
                    1.
                } else {
                    127. / 255.
                },
                true,
            );
        }
        if matches!(self.id, 513 | 520) && self.ticks < 64 {
            return (
                (255 - (self.ticks * 4).min(255)) as f32 / 255.,
                self.id == 520,
            );
        }
        (0., white)
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
        let id = match self.id {
            513..=515 => 177,
            516..=518 => 24,
            520 => 183,
            521 => 182,
            _ => return vec![],
        };
        vec![AudioCommand::RepeatSound {
            id,
            pan: 64,
            volume: 127,
            slot: 15,
        }]
    }
    pub fn step(&mut self, audio: &mut Vec<AudioCommand>) -> bool {
        self.ticks += 1;
        if self.ticks > self.duration() {
            return self.ticks >= self.duration() + 255u32.div_ceil(self.fade_rate());
        }
        for line in self
            .definition
            .dialogue
            .iter()
            .filter(|line| line.tick == self.ticks)
        {
            if line.tick == 540 {
                audio.push(AudioCommand::Sound {
                    id: 438,
                    pan: 64,
                    volume: 127,
                    slot: None,
                });
                audio.push(AudioCommand::SoundVolume {
                    slot: 15,
                    volume: 32,
                });
            }
            audio.push(AudioCommand::Voice(line.voice));
        }
        let sounds: &[(u32, i16)] = match self.id {
            516 => &[(1, 26), (1, 443), (40, 443), (74, 443), (130, 443)],
            517 => &[
                (140, 443),
                (170, 443),
                (190, 443),
                (220, 443),
                (520, 438),
                (1540, 438),
                (1620, 437),
                (1650, 437),
            ],
            518 => &[(1, 443), (170, 443), (190, 443), (220, 443)],
            519 => &[(50, 282), (50, 282)],
            520 => &[(220, 217), (272, 217), (340, 193)],
            521 => &[(300, 133)],
            522 | 523 => &[(46, 193), (62, 193), (88, 193), (46, 182), (88, 182)],
            525 => &[(1, 445)],
            526 => &[(1, 160)],
            _ => &[],
        };
        for &(_, id) in sounds.iter().filter(|(tick, _)| *tick == self.ticks) {
            audio.push(AudioCommand::Sound {
                id,
                pan: 64,
                volume: 127,
                slot: None,
            });
        }
        if self.ticks == self.duration() {
            audio.push(AudioCommand::StopSound(15));
        }
        self.ticks >= self.duration() + 255u32.div_ceil(self.fade_rate())
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
