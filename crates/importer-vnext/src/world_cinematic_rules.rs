//! Authored scene controls plus the few native REL tables they refer to.
use crate::{dol, rel::Rel};
use anyhow::{Context, Result, ensure};
use resonance_content::{TextureAnimation, overworld::*};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Catalogue {
    atlas_sequences: BTreeMap<String, AtlasSequence>,
    scenes: BTreeMap<u16, Recipe>,
}

/// Sounds used by overworld presentation outside the visible scene timelines.
pub(crate) fn retained_sounds() -> Result<Vec<u32>> {
    #[derive(Deserialize)]
    struct Retained {
        retained_sounds: Vec<u32>,
    }
    Ok(
        serde_json::from_str::<Retained>(include_str!("world-cinematic-rules.json"))?
            .retained_sounds,
    )
}

#[derive(Deserialize)]
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
struct SoundTable {
    offset: usize,
    count: usize,
}

#[derive(Deserialize)]
struct DialogueRecipe {
    table_offset: usize,
    lines: Vec<DialogueStyle>,
}

#[derive(Deserialize)]
struct DialogueStyle {
    duration: u16,
    speaker: u8,
}

#[derive(Deserialize)]
struct ActorRecipe {
    render: CinematicActor,
    opacity_table: Option<usize>,
    uv: Vec<UvRecipe>,
}

#[derive(Deserialize)]
struct UvRecipe {
    texture: usize,
    clock: CinematicClock,
    #[serde(default)]
    delay_ticks: u32,
    motion: UvMotion,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
struct AtlasSequence {
    denominator: u16,
    runs: Vec<AtlasRun>,
}

#[derive(Deserialize)]
struct AtlasRun {
    ticks: u16,
    frame: u16,
}

#[derive(Deserialize)]
struct LinearPiece {
    start: u32,
    rate: Scalar,
    offset: Scalar,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Scalar {
    Value(f32),
    Native {
        offset: usize,
        #[serde(default)]
        reciprocal: bool,
    },
}

fn float(bytes: &[u8], at: usize) -> Result<f32> {
    Ok(f32::from_bits(dol::u32(bytes, at)?))
}

impl Scalar {
    fn read(self, rel: &Rel) -> Result<f32> {
        let value = match self {
            Self::Value(value) => value,
            Self::Native { offset, reciprocal } => {
                let value = float(rel.slice(5, offset, 4)?, 0)?;
                if reciprocal { 1. / value } else { value }
            }
        };
        ensure!(value.is_finite(), "nonfinite cinematic parameter");
        Ok(value)
    }
}

fn uv_track(
    recipe: UvRecipe,
    rel: &Rel,
    sequences: &BTreeMap<String, AtlasSequence>,
) -> Result<CinematicUv> {
    let axis = match &recipe.motion {
        UvMotion::Scroll { axis, .. }
        | UvMotion::Atlas { axis, .. }
        | UvMotion::Linear { axis, .. } => *axis,
    };
    ensure!(axis < 2, "invalid cinematic UV axis");
    let offset = |value| {
        let mut pair = [0.; 2];
        pair[axis] = value;
        pair
    };
    let (offsets, loop_start) = match recipe.motion {
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
            let sequence = sequences
                .get(&sequence)
                .context("missing cinematic atlas sequence")?;
            ensure!(
                sequence.denominator != 0 && !sequence.runs.is_empty(),
                "invalid cinematic atlas sequence"
            );
            let duration: usize = sequence.runs.iter().map(|run| usize::from(run.ticks)).sum();
            ensure!(
                duration <= 36000 && sequence.runs.iter().all(|run| run.ticks != 0),
                "invalid cinematic atlas duration"
            );
            let begin = if let Some(table) = window_table {
                let row = rel.slice(5, table, 8)?;
                let begin = dol::u32(row, 0)?;
                let end = dol::u32(row, 4)?;
                ensure!(
                    begin <= end && end < 36000 && end - begin + 1 == duration as u32,
                    "cinematic atlas window mismatch"
                );
                begin as usize
            } else {
                0
            };
            let mut offsets = vec![[0.; 2]; begin];
            for run in &sequence.runs {
                offsets.extend(std::iter::repeat_n(
                    offset(f32::from(run.frame) / f32::from(sequence.denominator)),
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
                    && pieces.windows(2).all(|p| p[0].start < p[1].start),
                "invalid cinematic linear UV pieces"
            );
            let pieces = pieces
                .into_iter()
                .map(|piece| Ok((piece.start, piece.rate.read(rel)?, piece.offset.read(rel)?)))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                pieces.iter().all(|piece| piece.0 <= hold),
                "cinematic linear UV piece exceeds duration"
            );
            let offsets = (0..=hold)
                .map(|tick| {
                    let &(start, rate, base) =
                        &pieces[pieces.partition_point(|piece| piece.0 <= tick) - 1];
                    offset(rate.mul_add((tick - start) as f32, base))
                })
                .collect();
            (offsets, hold as usize)
        }
    };
    let animation = TextureAnimation {
        texture: recipe.texture,
        delay_ticks: recipe.delay_ticks,
        offsets,
        loop_start,
    };
    animation.validate()?;
    Ok(CinematicUv {
        clock: recipe.clock,
        animation,
    })
}

pub(crate) struct SceneRule {
    pub world: World,
    pub presentation: CinematicPresentation,
    pub dialogue: Vec<CinematicDialogue>,
}

pub(crate) fn read(rel: &Rel) -> Result<BTreeMap<u16, SceneRule>> {
    let catalogue: Catalogue = serde_json::from_str(include_str!("world-cinematic-rules.json"))?;
    ensure!(
        catalogue.scenes.keys().copied().eq(513..=526),
        "incomplete cinematic rules"
    );
    catalogue
        .scenes
        .into_iter()
        .map(|(id, recipe)| {
            let dialogue = recipe
                .dialogue
                .map(|source| {
                    source
                        .lines
                        .into_iter()
                        .enumerate()
                        .map(|(index, style)| {
                            let at = source.table_offset + index * 12;
                            let row = rel.slice(6, at, 12)?;
                            let (section, offset) = rel.pointer(6, at + 8)?;
                            Ok(CinematicDialogue {
                                tick: dol::u32(row, 0)?,
                                voice: dol::u32(row, 4)?,
                                text: rel.text(section, offset)?,
                                duration: style.duration,
                                speaker: style.speaker,
                            })
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            let mut audio = recipe.audio;
            if let Some(table) = recipe.sound_table {
                ensure!(table.count <= 256, "oversized cinematic sound table");
                for index in 0..table.count {
                    let row = rel.slice(5, table.offset + index * 4, 4)?;
                    audio.push(CinematicAudioEvent {
                        tick: u32::try_from(i16::from_be_bytes(row[..2].try_into()?))?,
                        before_dialogue: false,
                        action: CinematicAudio::Sound {
                            id: i16::from_be_bytes(row[2..4].try_into()?),
                            pan: 64,
                            volume: 127,
                            slot: None,
                            repeat: false,
                        },
                    });
                }
            }
            audio.sort_by_key(|event| (event.tick, !event.before_dialogue));
            let actors = recipe
                .actors
                .into_iter()
                .map(|(actor, mut source)| {
                    if let Some(at) = source.opacity_table {
                        let row = rel.slice(5, at, 4)?;
                        source.render.opacity = Some(CinematicOpacity {
                            begin: u32::from(dol::u16(row, 0)?),
                            end: u32::from(dol::u16(row, 2)?),
                        });
                    }
                    source.render.uv = source
                        .uv
                        .into_iter()
                        .map(|uv| uv_track(uv, rel, &catalogue.atlas_sequences))
                        .collect::<Result<_>>()?;
                    Ok((actor, source.render))
                })
                .collect::<Result<_>>()?;
            Ok((
                id,
                SceneRule {
                    world: recipe.world,
                    presentation: CinematicPresentation {
                        fade: recipe.fade,
                        initial_audio: recipe.initial_audio,
                        audio,
                        end_audio: recipe.end_audio,
                        actors,
                    },
                    dialogue,
                },
            ))
        })
        .collect()
}
