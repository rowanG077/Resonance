//! Prepared presentation tracks use the world scene's 60 Hz clock.
use crate::{ScenePart, TextureAnimation, field_audio::FieldAudio};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicPresentation {
    pub provenance: String,
    pub fade: CinematicFade,
    pub initial_audio: Vec<CinematicAudio>,
    /// Stable source order is retained for simultaneous events, including duplicates.
    pub audio: Vec<CinematicAudioEvent>,
    pub end_audio: Vec<CinematicAudio>,
    pub actors: BTreeMap<u8, CinematicActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicFade {
    pub white: bool,
    pub end_rate: u8,
    pub intro_rate: Option<u8>,
    pub flashes: Vec<CinematicFlash>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicFlash {
    pub start: u32,
    pub white: bool,
    pub alpha: Vec<u8>,
}

impl CinematicFade {
    pub fn sample(&self, tick: u32, duration: u32) -> (u8, bool) {
        if tick > duration {
            return (
                tick.saturating_sub(duration)
                    .saturating_mul(u32::from(self.end_rate))
                    .min(255) as u8,
                self.white,
            );
        }
        for flash in &self.flashes {
            if let Some(elapsed) = tick.checked_sub(flash.start)
                && let Some(&alpha) = flash.alpha.get(elapsed as usize)
            {
                return (alpha, flash.white);
            }
        }
        let alpha = self.intro_rate.map_or(0, |rate| {
            255 - tick.saturating_mul(u32::from(rate)).min(255) as u8
        });
        (alpha, self.white)
    }

    pub fn end_ticks(&self) -> u32 {
        255u32.div_ceil(u32::from(self.end_rate))
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.end_rate != 0 && self.intro_rate != Some(0),
            "zero cinematic fade rate"
        );
        let mut end = 0;
        for flash in &self.flashes {
            ensure!(
                flash.start >= end
                    && flash.start <= 36000
                    && !flash.alpha.is_empty()
                    && flash.alpha.len() <= 36000,
                "invalid or overlapping cinematic flash"
            );
            end = flash.start + flash.alpha.len() as u32;
            ensure!(end <= 36000, "cinematic flash exceeds track bounds");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicAudioEvent {
    pub tick: u32,
    pub before_dialogue: bool,
    pub action: CinematicAudio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CinematicAudio {
    Sound {
        id: i16,
        pan: u8,
        volume: u8,
        slot: Option<u8>,
        repeat: bool,
    },
    Volume {
        slot: u8,
        volume: u8,
    },
    Stop {
        slot: u8,
    },
}

impl CinematicAudio {
    pub fn sound(&self) -> Option<i16> {
        match *self {
            Self::Sound { id, .. } => Some(id),
            _ => None,
        }
    }

    fn validate(&self) -> Result<()> {
        match *self {
            Self::Sound {
                id,
                pan,
                volume,
                slot,
                repeat,
            } => {
                ensure!(
                    id >= 0
                        && pan <= 127
                        && volume <= 127
                        && slot.is_none_or(|s| s <= 15)
                        && (!repeat || slot.is_some()),
                    "invalid cinematic sound"
                );
            }
            Self::Volume { slot, volume } => ensure!(
                slot <= 15 && volume <= 127,
                "invalid cinematic sound volume"
            ),
            Self::Stop { slot } => ensure!(slot <= 15, "invalid cinematic sound stop"),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CinematicActor {
    pub background: bool,
    /// None preserves the model's own material setting.
    pub depth_write: Option<bool>,
    pub depth_test: Option<bool>,
    pub blend: Option<CinematicBlend>,
    pub opacity: Option<CinematicOpacity>,
    pub uv: Vec<CinematicUv>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CinematicClock {
    Scene,
    /// Original scrolling controllers sample VI's continuous retrace counter.
    Presentation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicUv {
    pub clock: CinematicClock,
    pub animation: TextureAnimation,
}

impl CinematicUv {
    pub fn offset(&self, scene_tick: u32, presentation_tick: u32) -> [f32; 2] {
        let tick = match self.clock {
            CinematicClock::Scene => scene_tick,
            CinematicClock::Presentation => presentation_tick,
        };
        self.animation.offset(u64::from(tick))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CinematicBlend {
    Alpha,
    Additive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CinematicOpacity {
    pub begin: u32,
    pub end: u32,
}

impl CinematicOpacity {
    pub fn alpha(&self, tick: u32) -> u8 {
        if tick >= self.end {
            0
        } else if tick > self.begin {
            // Native actor alpha is an integer byte; truncate before normalization.
            ((self.end - 1 - tick) * 255 / (self.end - self.begin)) as u8
        } else {
            255
        }
    }
}

impl CinematicPresentation {
    pub fn actions(&self) -> impl Iterator<Item = &CinematicAudio> {
        self.initial_audio
            .iter()
            .chain(self.audio.iter().map(|event| &event.action))
            .chain(&self.end_audio)
    }

    pub fn validate(&self, actors: &BTreeMap<u8, Vec<ScenePart>>) -> Result<()> {
        ensure!(
            !self.provenance.trim().is_empty() && self.provenance.len() <= 2048,
            "cinematic presentation requires bounded provenance"
        );
        self.fade.validate()?;
        ensure!(
            actors.keys().eq(self.actors.keys()),
            "cinematic actor tracks are incomplete"
        );
        ensure!(
            self.audio.len() <= 4096
                && self.initial_audio.len() <= 32
                && self.end_audio.len() <= 32,
            "too many cinematic audio events"
        );
        ensure!(
            self.audio
                .windows(2)
                .all(|events| (events[0].tick, !events[0].before_dialogue)
                    <= (events[1].tick, !events[1].before_dialogue)),
            "unordered cinematic audio events"
        );
        ensure!(
            self.audio
                .iter()
                .all(|event| (1..=36000).contains(&event.tick)),
            "invalid cinematic audio tick"
        );
        for action in self.actions() {
            action.validate()?;
        }
        for (&id, track) in &self.actors {
            if let Some(opacity) = track.opacity {
                ensure!(
                    opacity.begin < opacity.end && opacity.end <= 36000,
                    "invalid cinematic opacity window"
                );
            }
            let mut textures = BTreeSet::new();
            for uv in &track.uv {
                let uv = &uv.animation;
                uv.validate()?;
                ensure!(
                    uv.delay_ticks <= 36000
                        && textures.insert(uv.texture)
                        && actors[&id]
                            .iter()
                            .any(|part| uv.texture < part.textures.len()),
                    "invalid or duplicate cinematic UV binding"
                );
            }
        }
        Ok(())
    }

    pub fn validate_audio(&self, audio: &FieldAudio) -> Result<()> {
        for id in self.actions().filter_map(CinematicAudio::sound) {
            ensure!(
                audio.sounds.contains_key(&id),
                "cinematic sound {id} is not prepared"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_clock_selection_preserves_continuous_phase_across_scene_entries() {
        let mut uv = CinematicUv {
            clock: CinematicClock::Presentation,
            animation: TextureAnimation {
                interpolation: crate::TextureInterpolation::Step,
                texture: 0,
                delay_ticks: 0,
                loop_start: 0,
                offsets: vec![[0., 0.], [0.25, 0.], [0.5, 0.], [0.75, 0.]],
            },
        };
        assert_eq!(uv.offset(0, 7), [0.75, 0.]);
        assert_eq!(uv.offset(120, 7), [0.75, 0.]);
        assert_eq!(uv.offset(0, 8), [0., 0.]);
        uv.clock = CinematicClock::Scene;
        assert_eq!(uv.offset(1, 8), [0.25, 0.]);
        uv.animation.delay_ticks = 1;
        assert_eq!(uv.offset(1, 8), [0., 0.]);
        assert_eq!(uv.offset(2, 8), [0.25, 0.]);
    }

    #[test]
    fn actor_opacity_preserves_integer_rounding_and_endpoint_visibility() {
        let window = CinematicOpacity { begin: 66, end: 76 };
        assert_eq!(
            [65, 66, 67, 68, 74, 75, 76, 77].map(|tick| window.alpha(tick)),
            [255, 255, 204, 178, 25, 0, 0, 0]
        );
    }

    #[test]
    fn fade_tracks_keep_flash_and_final_fade_boundaries() -> Result<()> {
        let mut fade = CinematicFade {
            white: false,
            end_rate: 8,
            intro_rate: None,
            flashes: vec![CinematicFlash {
                start: 299,
                white: true,
                alpha: vec![127, 255, 255, 127],
            }],
        };
        fade.validate()?;
        assert_eq!(
            [298, 299, 300, 301, 302, 303].map(|tick| fade.sample(tick, 420)),
            [
                (0, false),
                (127, true),
                (255, true),
                (255, true),
                (127, true),
                (0, false)
            ]
        );
        assert_eq!(fade.sample(420, 420), (0, false));
        assert_eq!(fade.sample(421, 420), (8, false));
        assert_eq!(fade.sample(451, 420), (248, false));
        assert_eq!(fade.sample(452, 420), (255, false));
        assert_eq!(fade.end_ticks(), 32);
        fade.white = true;
        fade.intro_rate = Some(4);
        fade.end_rate = 4;
        assert_eq!(
            [0, 1, 63, 64].map(|tick| fade.sample(tick, 1660)),
            [(255, true), (251, true), (3, true), (0, true)]
        );
        assert_eq!(fade.end_ticks(), 64);
        fade.flashes.push(CinematicFlash {
            start: 302,
            white: false,
            alpha: vec![1],
        });
        assert!(fade.validate().is_err());
        fade.flashes.clear();
        fade.end_rate = 0;
        assert!(fade.validate().is_err());
        Ok(())
    }

    #[test]
    fn audio_validation_retains_duplicates_but_rejects_ambiguous_order_and_slots() -> Result<()> {
        let event = CinematicAudioEvent {
            tick: 50,
            before_dialogue: false,
            action: CinematicAudio::Sound {
                id: 282,
                pan: 64,
                volume: 127,
                slot: None,
                repeat: false,
            },
        };
        let mut presentation = CinematicPresentation {
            provenance: "synthetic audio fixture".into(),
            fade: CinematicFade {
                white: false,
                end_rate: 8,
                intro_rate: None,
                flashes: vec![],
            },
            initial_audio: vec![],
            audio: vec![event.clone(), event],
            end_audio: vec![],
            actors: Default::default(),
        };
        presentation.validate(&BTreeMap::new())?;
        assert_eq!(
            presentation
                .actions()
                .filter_map(CinematicAudio::sound)
                .collect::<Vec<_>>(),
            [282, 282]
        );
        presentation.audio[1].before_dialogue = true;
        assert!(presentation.validate(&BTreeMap::new()).is_err());
        presentation.audio[1].before_dialogue = false;
        presentation.audio[1].tick = 49;
        assert!(presentation.validate(&BTreeMap::new()).is_err());
        presentation.audio[1].tick = 50;
        presentation.initial_audio.push(CinematicAudio::Sound {
            id: 1,
            pan: 64,
            volume: 127,
            slot: None,
            repeat: true,
        });
        assert!(presentation.validate(&BTreeMap::new()).is_err());
        presentation.initial_audio = vec![CinematicAudio::Stop { slot: 16 }];
        assert!(presentation.validate(&BTreeMap::new()).is_err());
        Ok(())
    }
}
