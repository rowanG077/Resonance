//! Original cinematic tables plus translated native setup/controller parameters.
use crate::{read::Field, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::{TextureAnimation, overworld::*};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    provenance: String,
    retained_sounds: Vec<i16>,
    atlas_sequences: BTreeMap<String, AtlasSequence>,
    scenes: BTreeMap<u16, Recipe>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    world: World,
    fade: CinematicFade,
    initial_audio: Vec<CinematicAudio>,
    audio: Vec<CinematicAudioEvent>,
    end_audio: Vec<CinematicAudio>,
    sound_table: Option<SoundTable>,
    actors: BTreeMap<u8, ActorRecipe>,
    dialogue: Option<DialogueRecipe>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DialogueRecipe {
    table_offset: usize,
    archive: String,
    member: usize,
    first_voice: u32,
    lines: Vec<DialogueStyle>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DialogueStyle {
    duration: u16,
    speaker: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundTable {
    offset: usize,
    count: usize,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ActorRecipe {
    render: CinematicActor,
    opacity_table: Option<usize>,
    uv: Vec<UvRecipe>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UvRecipe {
    texture: usize,
    clock: CinematicClock,
    #[serde(default)]
    delay_ticks: u32,
    motion: UvMotion,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum UvMotion {
    Scroll {
        axis: usize,
        period: u32,
    },
    Atlas {
        axis: usize,
        sequence: String,
        window_table: Option<usize>,
    },
    Linear {
        axis: usize,
        hold: u32,
        pieces: Vec<LinearPiece>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AtlasRun {
    ticks: u16,
    frame: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AtlasSequence {
    denominator: u16,
    runs: Vec<AtlasRun>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinearPiece {
    start: u32,
    rate: Scalar,
    offset: Scalar,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Scalar {
    Value(f32),
    Original {
        offset: usize,
        #[serde(default)]
        reciprocal: bool,
    },
}

impl Scalar {
    fn read(self, rel: &Rel) -> Result<f32> {
        let value = match self {
            Self::Value(value) => value,
            Self::Original { offset, reciprocal } => {
                let value = f32::read(rel.at((5, offset))?, 0)?;
                if reciprocal { 1. / value } else { value }
            }
        };
        ensure!(value.is_finite(), "nonfinite cinematic parameter");
        Ok(value)
    }
}

#[derive(Clone)]
pub(super) struct Definition {
    pub world: World,
    pub presentation: CinematicPresentation,
    pub dialogue: Vec<CinematicDialogue>,
    pub voices: Option<crate::media::field_audio::WorldVoiceArchive>,
}

pub(super) fn read(rel: &Rel) -> Result<(BTreeMap<u16, Definition>, Vec<i16>)> {
    let recipes: Catalogue = serde_json::from_str(include_str!("overworld_cinematics.json"))?;
    ensure!(
        recipes.scenes.len() == 14 && recipes.scenes.keys().copied().eq(513..=526),
        "incomplete cinematic recipes"
    );
    let mut sounds: BTreeSet<_> = recipes.retained_sounds.into_iter().collect();
    ensure!(
        sounds.iter().all(|id| *id >= 0),
        "invalid retained world sound"
    );
    let scenes = recipes
        .scenes
        .into_iter()
        .map(|(id, recipe)| {
            let (dialogue, voices) = if let Some(source) = recipe.dialogue {
                ensure!(
                    !source.lines.is_empty() && source.lines.len() <= 256,
                    "invalid cinematic dialogue count"
                );
                resonance_content::validate_asset_path(&source.archive)?;
                let dialogue = source
                    .lines
                    .into_iter()
                    .enumerate()
                    .map(|(index, style)| {
                        let offset = source
                            .table_offset
                            .checked_add(index * 12)
                            .context("cinematic dialogue offset overflow")?;
                        let row = rel.at((6, offset))?;
                        Ok(CinematicDialogue {
                            tick: u32::read(row, 0)?,
                            voice: u32::read(row, 4)?,
                            text: rel.text(rel.pointer(6, offset + 8)?)?,
                            duration: style.duration,
                            speaker: style.speaker,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let voices = crate::media::field_audio::WorldVoiceArchive {
                    source: source.archive,
                    member: source.member,
                    first_resource: source.first_voice,
                };
                (dialogue, Some(voices))
            } else {
                (vec![], None)
            };
            let mut audio = recipe.audio;
            if let Some(table) = recipe.sound_table {
                ensure!(table.count <= 256, "oversized cinematic sound table");
                let bytes = rel.at((5, table.offset))?;
                for index in 0..table.count {
                    let tick = u32::try_from(i16::read(bytes, index * 4)?)?;
                    let sound = i16::read(bytes, index * 4 + 2)?;
                    audio.push(CinematicAudioEvent {
                        tick,
                        before_dialogue: false,
                        action: CinematicAudio::Sound {
                            id: sound,
                            pan: 64,
                            volume: 127,
                            slot: None,
                            repeat: false,
                        },
                    });
                }
            }
            // Stable sorting retains native row order for simultaneous sounds.
            audio.sort_by_key(|event| (event.tick, !event.before_dialogue));
            let actors = recipe
                .actors
                .into_iter()
                .map(|(actor, recipe)| {
                    let mut render = recipe.render;
                    ensure!(
                        render.uv.is_empty() && render.opacity.is_none(),
                        "cinematic recipe duplicates extracted tracks"
                    );
                    if let Some(offset) = recipe.opacity_table {
                        let bytes = rel.at((5, offset))?;
                        render.opacity = Some(CinematicOpacity {
                            begin: u32::from(u16::read(bytes, 0)?),
                            end: u32::from(u16::read(bytes, 2)?),
                        });
                    }
                    render.uv = recipe
                        .uv
                        .into_iter()
                        .map(|uv| uv.prepare(rel, &recipes.atlas_sequences))
                        .collect::<Result<_>>()?;
                    Ok((actor, render))
                })
                .collect::<Result<_>>()?;
            let presentation = CinematicPresentation {
                provenance: recipes.provenance.clone(),
                fade: recipe.fade,
                initial_audio: recipe.initial_audio,
                audio,
                end_audio: recipe.end_audio,
                actors,
            };
            sounds.extend(presentation.actions().filter_map(CinematicAudio::sound));
            Ok((
                id,
                Definition {
                    world: recipe.world,
                    presentation,
                    dialogue,
                    voices,
                },
            ))
        })
        .collect::<Result<_>>()?;
    Ok((scenes, sounds.into_iter().collect()))
}

impl UvRecipe {
    fn prepare(
        self,
        rel: &Rel,
        sequences: &BTreeMap<String, AtlasSequence>,
    ) -> Result<CinematicUv> {
        let axis = match &self.motion {
            UvMotion::Scroll { axis, .. }
            | UvMotion::Atlas { axis, .. }
            | UvMotion::Linear { axis, .. } => *axis,
        };
        ensure!(axis < 2, "invalid cinematic UV axis");
        let offset = |value| {
            let mut out = [0.; 2];
            out[axis] = value;
            out
        };
        let interpolation = if matches!(self.motion, UvMotion::Atlas { .. }) {
            resonance_content::TextureInterpolation::Step
        } else {
            resonance_content::TextureInterpolation::Scroll
        };
        let (offsets, loop_start) = match self.motion {
            UvMotion::Scroll { period, .. } => {
                ensure!(
                    (1..=36000).contains(&period),
                    "invalid cinematic scroll period"
                );
                (
                    (0..period)
                        .map(|tick| offset(tick as f32 / period as f32))
                        .collect(),
                    0,
                )
            }
            UvMotion::Atlas {
                sequence,
                window_table,
                ..
            } => {
                let AtlasSequence { denominator, runs } = sequences
                    .get(&sequence)
                    .context("missing cinematic atlas sequence")?;
                ensure!(
                    *denominator != 0 && !runs.is_empty() && runs.iter().all(|run| run.ticks != 0),
                    "invalid cinematic atlas sequence"
                );
                let duration: usize = runs.iter().map(|run| usize::from(run.ticks)).sum();
                ensure!(duration <= 36000, "cinematic atlas sequence is too long");
                let begin = if let Some(table) = window_table {
                    let bytes = rel.at((5, table))?;
                    let begin = u32::read(bytes, 0)?;
                    let end = u32::read(bytes, 4)?;
                    ensure!(
                        begin <= end && end < 36000 && end - begin + 1 == duration as u32,
                        "cinematic atlas window differs from controller duration"
                    );
                    begin as usize
                } else {
                    0
                };
                let mut offsets = vec![[0.; 2]; begin];
                for run in runs {
                    offsets.extend(std::iter::repeat_n(
                        offset(f32::from(run.frame) / f32::from(*denominator)),
                        usize::from(run.ticks),
                    ));
                }
                let loop_start = if window_table.is_some() {
                    offsets.push([0.; 2]);
                    offsets.len() - 1
                } else {
                    0
                };
                (offsets, loop_start)
            }
            UvMotion::Linear { hold, pieces, .. } => {
                ensure!(
                    hold < 36000
                        && pieces.first().is_some_and(|p| p.start == 0)
                        && pieces.windows(2).all(|p| p[0].start < p[1].start)
                        && pieces.iter().all(|p| p.start <= hold),
                    "invalid cinematic linear UV pieces"
                );
                let pieces = pieces
                    .into_iter()
                    .map(|piece| Ok((piece.start, piece.rate.read(rel)?, piece.offset.read(rel)?)))
                    .collect::<Result<Vec<_>>>()?;
                let offsets = (0..=hold)
                    .map(|tick| {
                        let &(start, rate, base) =
                            &pieces[pieces.partition_point(|p| p.0 <= tick) - 1];
                        // Original controller uses single-precision fused multiply/add.
                        offset(rate.mul_add((tick - start) as f32, base))
                    })
                    .collect();
                (offsets, hold as usize)
            }
        };
        let track = TextureAnimation {
            texture: self.texture,
            interpolation,
            delay_ticks: self.delay_ticks,
            offsets,
            loop_start,
        };
        track.validate()?;
        Ok(CinematicUv {
            clock: self.clock,
            animation: track,
        })
    }
}
