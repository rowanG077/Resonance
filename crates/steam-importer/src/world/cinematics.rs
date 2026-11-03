//! Native world scene media uses the shared GC staging and editable scene roots.
use crate::{
    audio_effects, field_scene, output, package::Package, pe, preview_model, source::Source,
    text::Bank,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, TextureAnimation,
    camera_motion::CameraMotion,
    library::Mappings,
    overworld::{self as world, document},
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};
mod windows;

#[derive(Deserialize)]
struct Catalogue {
    scenes: BTreeMap<u16, Recipe>,
    atlas_sequences: BTreeMap<String, AtlasSequence>,
}
#[derive(Deserialize)]
struct Recipe {
    world: world::World,
    fade: world::CinematicFade,
    initial_audio: Vec<world::CinematicAudio>,
    audio: Vec<world::CinematicAudioEvent>,
    end_audio: Vec<world::CinematicAudio>,
    actors: BTreeMap<u8, Actor>,
    dialogue: Option<Dialogue>,
}
#[derive(Deserialize)]
struct Actor {
    render: world::CinematicActor,
    opacity_table: Option<usize>,
    uv: Vec<UvRecipe>,
}
#[derive(Deserialize)]
struct Dialogue {
    lines: Vec<Line>,
}
#[derive(Deserialize)]
struct Line {
    duration: u16,
    speaker: u8,
}
#[derive(Deserialize)]
struct Names {
    name: String,
    actors: BTreeMap<u8, String>,
}
#[derive(Deserialize)]
struct UvRecipe {
    texture: usize,
    clock: world::CinematicClock,
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
        pieces: Vec<Piece>,
    },
}
#[derive(Deserialize)]
struct AtlasSequence {
    denominator: u16,
    runs: Vec<Run>,
}
#[derive(Deserialize)]
struct Run {
    ticks: u16,
    frame: u16,
}
#[derive(Deserialize)]
struct Piece {
    start: u32,
    rate: Scalar,
    offset: Scalar,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Scalar {
    Value(f32),
    Original {
        offset: usize,
        #[serde(default)]
        reciprocal: bool,
    },
}
impl Scalar {
    fn read(self) -> Result<f32> {
        // These are GC presentation style constants, verified against section 5
        // of the current GC source. Native offsets are importer recipe identities.
        let value = match self {
            Self::Value(value) => value,
            Self::Original { offset, reciprocal } => {
                let value = match offset {
                    0x8dc => 0.,
                    0xa7c => 1. / 300.,
                    0xa80 => 0.4,
                    0xa84 => 1. / 600.,
                    0xa34 => 0.6,
                    0xc70 => 40.,
                    _ => anyhow::bail!("unknown GC cinematic style scalar {offset:#x}"),
                };
                if reciprocal { 1. / value } else { value }
            }
        };
        ensure!(value.is_finite(), "nonfinite cinematic UV scalar");
        Ok(value)
    }
}

fn uv(
    recipe: UvRecipe,
    sequences: &BTreeMap<String, AtlasSequence>,
    windows: &BTreeMap<usize, (u32, u32)>,
) -> Result<world::CinematicUv> {
    let axis = match &recipe.motion {
        UvMotion::Scroll { axis, .. }
        | UvMotion::Atlas { axis, .. }
        | UvMotion::Linear { axis, .. } => *axis,
    };
    ensure!(axis < 2, "invalid cinematic UV axis");
    let point = |value| {
        let mut p = [0.; 2];
        p[axis] = value;
        p
    };
    let (offsets, loop_start) = match recipe.motion {
        UvMotion::Scroll { period, .. } => {
            ensure!((1..=36000).contains(&period), "invalid cinematic UV period");
            (
                (0..period)
                    .map(|tick| point(tick as f32 / period as f32))
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
            let duration = sequence
                .runs
                .iter()
                .map(|r| usize::from(r.ticks))
                .sum::<usize>();
            ensure!(
                sequence.denominator > 0
                    && (1..=36000).contains(&duration)
                    && sequence.runs.iter().all(|r| r.ticks > 0),
                "invalid cinematic atlas sequence"
            );
            let begin = if let Some(table) = window_table {
                let &(begin, end) = windows
                    .get(&table)
                    .context("missing cinematic atlas window")?;
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
                    point(f32::from(run.frame) / f32::from(sequence.denominator)),
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
                .map(|p| Ok((p.start, p.rate.read()?, p.offset.read()?)))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                pieces.iter().all(|p| p.0 <= hold),
                "cinematic linear UV piece exceeds hold"
            );
            let offsets = (0..=hold)
                .map(|tick| {
                    let &(start, rate, base) = &pieces[pieces.partition_point(|p| p.0 <= tick) - 1];
                    point(rate.mul_add((tick - start) as f32, base))
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
    Ok(world::CinematicUv {
        clock: recipe.clock,
        animation,
    })
}

fn opacity(image: &pe::Image<'_>) -> Result<Vec<world::CinematicOpacity>> {
    ensure!(
        image.slice(0x4b0480, 12)? == [0x83, 0xc7, 4, 0x81, 0xc3, 0, 0x2d, 0, 0, 0x83, 0xff, 8],
        "cinematic opacity actor stride changed"
    );
    super::stack_array(image, -0x24, &[(0x4b0344, 7, 7), (0x4b037a, 1, 7)])?
        .chunks_exact(4)
        .map(|row| {
            let begin = u32::from(pe::half(row, 0)?);
            let end = u32::from(pe::half(row, 2)?);
            ensure!(
                begin < end && end <= 36000,
                "invalid cinematic opacity window"
            );
            Ok(world::CinematicOpacity { begin, end })
        })
        .collect()
}

fn sound(id: i16) -> Result<i16> {
    let native = match id {
        177 => 0x200,
        24 => 1,
        183 => 0x400,
        182 => 0x402,
        438 => 0x300,
        437 => 0x301,
        445 => 0x500,
        160 => 0x501,
        _ => anyhow::bail!("unbound GC-style cinematic sound {id}"),
    };
    Ok(audio_effects::id(native)?.try_into()?)
}
fn sound_table(image: &pe::Image<'_>, id: u16) -> Result<Vec<world::CinematicAudioEvent>> {
    let run = match id {
        516 => Some((0x4b09dd, 5, -0x18)),
        517 => Some((0x4b0a34, 7, -0x20)),
        518 => Some((0x4b0aa2, 4, -0x14)),
        519 => Some((0x4b0af2, 2, -0xc)),
        520 => Some((0x4b0b34, 3, -0x10)),
        522 | 523 => Some((0x4b0bb2, 5, -0x18)),
        _ => None,
    };
    let bytes = if let Some((at, count, origin)) = run {
        super::stack_array(image, origin, &[(at, count, 7)])?
    } else if id == 521 {
        [
            u16::try_from(pe::word(image.slice(0x4b0b8e, 4)?, 0)?)?.to_le_bytes(),
            u16::try_from(pe::word(image.slice(0x4b0ba4, 4)?, 0)?)?.to_le_bytes(),
        ]
        .concat()
    } else {
        Vec::new()
    };
    bytes
        .chunks_exact(4)
        .map(|row| {
            Ok(world::CinematicAudioEvent {
                tick: u32::from(pe::half(row, 0)?),
                before_dialogue: false,
                action: world::CinematicAudio::Sound {
                    id: audio_effects::id(pe::half(row, 2)?)?.try_into()?,
                    pan: 64,
                    volume: 127,
                    slot: None,
                    repeat: false,
                },
            })
        })
        .collect()
}
fn dialogue(
    image: &pe::Image<'_>,
    bank: &Bank<'_>,
    language: u32,
    source: Dialogue,
) -> Result<Vec<world::CinematicDialogue>> {
    ensure!(
        source.lines.len() == 9,
        "world crash dialogue count changed"
    );
    let captions = pe::word(image.slice(0x8df978 + language * 4, 4)?, 0)?;
    let voice = pe::word(image.slice(0x4b04de, 4)?, 0)?;
    source
        .lines
        .into_iter()
        .zip(image.slice(0x8df990, 9 * 12)?.chunks_exact(12))
        .map(|(style, row)| {
            let text = pe::word(image.slice(captions + pe::word(row, 8)? * 4, 4)?, 0)?;
            Ok(world::CinematicDialogue {
                tick: pe::word(row, 0)?,
                duration: style.duration,
                speaker: style.speaker,
                voice: voice
                    .checked_add(pe::word(row, 4)?)
                    .context("world dialogue voice overflow")?,
                text: bank.menu(text, encoding_rs::WINDOWS_1252)?,
            })
        })
        .collect()
}

pub(super) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    ensure!(
        image.text(0x86ff58)? == "e%02d.pac",
        "cinematic source filename template changed"
    );
    let catalogue: Catalogue = serde_json::from_str(include_str!(
        "../../../importer/src/world-cinematic-rules.json"
    ))?;
    let names: BTreeMap<u16, Names> = serde_json::from_str(include_str!(
        "../../../importer/src/world-cinematic-names.json"
    ))?;
    ensure!(
        catalogue.scenes.keys().copied().eq(513..=526) && names.keys().copied().eq(513..=526),
        "incomplete world cinematics"
    );
    let opacity = opacity(&image)?;
    let windows = windows::read(&image)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let mut mappings = BTreeMap::new();
    let (mut models, mut uv_tracks, mut opacity_tracks, mut lines) = (0, 0, 0, 0);
    for (id, recipe) in catalogue.scenes {
        let names = &names[&id];
        let owner = format!("world/cinematics/{}", names.name);
        let bytes = source.require(&format!("e{:02}.pac", id - 513))?;
        let directory = Package::parse(&bytes)?;
        let camera = field_scene::camera(
            &source.require(
                directory
                    .reference(0)?
                    .context("missing cinematic camera")?,
            )?,
        )?;
        let camera_path = format!("{owner}/camera.glb");
        output::write(&library.join(&camera_path), &camera)?;
        let camera = CameraMotion::decode(&camera)?;
        let mut actors = BTreeMap::new();
        let mut scene_actors = BTreeMap::new();
        for (&slot, name) in &names.actors {
            ensure!(
                recipe.actors.contains_key(&slot),
                "cinematic actor lacks GC staging"
            );
            let bytes = source.require(
                directory
                    .reference(usize::from(slot))?
                    .context("missing cinematic actor package")?,
            )?;
            let actor = Package::parse(&bytes)?;
            let mut motions = Vec::new();
            for (slot, address) in [0x4ae5c8, 0x4ae5ca, 0x4ae5d0, 0x4ae5d2]
                .into_iter()
                .enumerate()
            {
                let index = usize::from(
                    pe::half(image.slice(address, 2)?, 0)?
                        .checked_sub(1)
                        .context("invalid cinematic motion slot")?,
                );
                if index < actor.len() {
                    if let Some(name) = actor.reference(index)? {
                        motions.push((slot as u16, name.to_owned()));
                    }
                }
            }
            let path = format!("{owner}/actors/{name}");
            let (mut scene, _) = preview_model::part(
                source,
                library,
                actor.reference(0)?.context("missing cinematic body")?,
                &path,
                "body",
                &motions,
            )
            .with_context(|| format!("world cinematic {id} actor {slot}"))?;
            if matches!(id, 514 | 515) && slot == 1 && scene.textures.len() == 9 {
                scene.appearance = Some(AppearanceTextures {
                    eyes: Some(7),
                    mouth: Some(6),
                    variant: None,
                    costume: None,
                });
            }
            let path = format!("{path}/model.json");
            output::json(
                &library.join(&path),
                &document::Model {
                    parts: vec![scene.clone()],
                },
            )?;
            actors.insert(slot, path);
            scene_actors.insert(slot, vec![scene]);
            models += 1;
        }
        let dialogue = recipe
            .dialogue
            .map(|d| dialogue(&image, &bank, source.language.slot(), d))
            .transpose()?
            .unwrap_or_default();
        lines += dialogue.len();
        let mut presentation = world::CinematicPresentation {
            fade: recipe.fade,
            initial_audio: recipe.initial_audio,
            audio: recipe.audio,
            end_audio: recipe.end_audio,
            actors: recipe
                .actors
                .into_iter()
                .map(|(slot, mut actor)| {
                    ensure!(actor.render.uv.is_empty(), "duplicate cinematic UV source");
                    if let Some(offset) = actor.opacity_table {
                        ensure!(
                            id == 516
                                && (1..=8).contains(&slot)
                                && offset == 0x324 + usize::from(slot - 1) * 4,
                            "unknown cinematic opacity selector"
                        );
                        actor.render.opacity = Some(opacity[usize::from(slot - 1)]);
                        opacity_tracks += 1;
                    }
                    actor.render.uv = actor
                        .uv
                        .into_iter()
                        .map(|recipe| uv(recipe, &catalogue.atlas_sequences, &windows))
                        .collect::<Result<_>>()?;
                    uv_tracks += actor.render.uv.len();
                    Ok((slot, actor.render))
                })
                .collect::<Result<_>>()?,
        };
        for action in presentation
            .initial_audio
            .iter_mut()
            .chain(presentation.audio.iter_mut().map(|e| &mut e.action))
            .chain(&mut presentation.end_audio)
        {
            if let world::CinematicAudio::Sound { id, .. } = action {
                *id = sound(*id)?;
            }
        }
        presentation.audio.extend(sound_table(&image, id)?);
        presentation
            .audio
            .sort_by_key(|event| (event.tick, !event.before_dialogue));
        world::Cinematic {
            world: recipe.world,
            presentation: presentation.clone(),
            camera,
            actors: scene_actors,
            dialogue: dialogue.clone(),
        }
        .validate(|_| true)?;
        output::json(
            &library.join(&owner).join("cinematic.json"),
            &document::Presentation {
                world: recipe.world,
                presentation,
                camera: camera_path,
                actors,
                dialogue,
            },
        )?;
        mappings.insert(u32::from(id), names.name.clone());
    }
    output::json(
        &library.join(document::CINEMATICS),
        &Mappings::new(mappings)?,
    )?;
    eprintln!(
        "  World cinematics: 14 scenes, {models} actors, {uv_tracks} UV tracks, {opacity_tracks} opacity tracks and {lines} dialogue lines"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn gc_style_linear_tracks_keep_the_authored_windows_and_hold() -> Result<()> {
        let recipe: UvRecipe = serde_json::from_value(
            serde_json::json!({"texture":1,"clock":"scene","delay_ticks":1,"motion":{"kind":"linear","axis":1,"hold":240,"pieces":[{"start":0,"offset":{"offset":2268},"rate":{"offset":2684}},{"start":60,"offset":{"offset":2688},"rate":{"offset":2692}},{"start":210,"offset":{"offset":2612},"rate":{"offset":2684}}]}}),
        )?;
        let track = uv(recipe, &BTreeMap::new(), &BTreeMap::new())?;
        assert_eq!(track.animation.offsets.len(), 241);
        assert_eq!(track.animation.loop_start, 240);
        assert_eq!(track.animation.offsets[60], [0., 0.4]);
        assert_eq!(track.animation.offsets[210], [0., 0.6]);
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks native world cinematics"]
    fn installed_world_cinematics_cook_into_shared_documents() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root)
    }
}
