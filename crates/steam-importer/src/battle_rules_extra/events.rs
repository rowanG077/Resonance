//! Native battle programs and the two authored Vidarr actor recipes.
use crate::{
    animation::Animation, battle_party_mappings, model, output, package::Package, pe,
    resources::Resources, script, source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, AtlasChannel, SceneClip,
    animation::Skeleton,
    battle_audio::Selection,
    battle_event::document::{self, Actor, ActorSource, Definition},
    battle_model::document::PartyDefinition,
    field_audio::{document as audio, speech},
    library::Mappings,
    model_preview::ModelPreview,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::{BattleEventId, PartyCharacterId};
use symphonia_script::{
    NativeCall, Program,
    message::{Message, Token},
    scenario,
};

fn clip(
    source: &Source,
    library: &Path,
    rig: &model::Rig,
    skeleton: &Skeleton,
    native: &str,
    path: String,
    slot: u16,
    resource: Option<u32>,
) -> Result<SceneClip> {
    let motion = Animation::read(&source.require(native)?)?.bind(rig, skeleton)?;
    output::write(&library.join(&path), &motion.encode(skeleton)?)?;
    Ok(SceneClip {
        motion: path,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: resource,
        pose_chains: Vec::new(),
    })
}

fn party(
    source: &Source,
    library: &Path,
    resources: &Resources,
    image: &pe::Image<'_>,
    character: u8,
    external: &[u32],
) -> Result<Actor> {
    let name = PartyCharacterId::try_from(character)?
        .resource_name()
        .context("unnamed event party actor")?;
    let path = format!("battle/party/{name}/default/party.json");
    let actor: PartyDefinition = serde_json::from_slice(&fs::read(library.join(&path))?)?;
    let skeleton = Skeleton::from_glb(&fs::read(library.join(&actor.parts[0].mesh))?)?;
    let bytes = source.require(battle_party_mappings::source_name(image, character, 0)?)?;
    let package = Package::parse(&bytes)?;
    let rig = model::Rig::read(
        &source.require(package.reference(0)?.context("event party body absent")?)?,
    )?;
    let clips = external
        .iter()
        .map(|&id| {
            clip(
                source,
                library,
                &rig,
                &skeleton,
                resources.loaded(id)?,
                format!("battle/events/actors/{name}/motion-{id:08x}.glb"),
                0,
                Some(id),
            )
        })
        .collect::<Result<_>>()?;
    Ok(Actor {
        source: ActorSource::Party { path },
        clips,
        appearance: None,
    })
}

fn soldier(source: &Source, library: &Path, resources: &Resources) -> Result<Actor> {
    let bytes = source.require(resources.loaded(0x2005d)?)?;
    let package = Package::parse(&bytes)?;
    let rig =
        model::Rig::read(&source.require(package.reference(0)?.context("soldier body absent")?)?)?;
    let path = "meshes/figurines/renegade-0150/model.json";
    let preview: ModelPreview = serde_json::from_slice(&fs::read(library.join(path))?)?;
    let skeleton = Skeleton::from_glb(&fs::read(library.join(&preview.parts[0].scene.mesh))?)?;
    let mut clips = Vec::new();
    for slot in [3u16, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17] {
        clips.push(clip(
            source,
            library,
            &rig,
            &skeleton,
            package
                .reference(usize::from(slot) + 2)?
                .context("soldier motion absent")?,
            format!("battle/events/actors/soldier/motion-{slot}.glb"),
            slot,
            None,
        )?);
    }
    clips.push(clip(
        source,
        library,
        &rig,
        &skeleton,
        resources.loaded(0x10aa0)?,
        "battle/events/actors/soldier/motion-00010aa0.glb".into(),
        0,
        Some(0x10aa0),
    )?);
    Ok(Actor {
        source: ActorSource::Model {
            path: path.into(),
            body_parts: None,
        },
        clips,
        appearance: Some(AppearanceTextures {
            eyes: None,
            mouth: Some(2),
            variant: Some(AtlasChannel {
                texture: 1,
                frames: 4,
            }),
            costume: None,
        }),
    })
}

fn select(
    values: impl IntoIterator<Item = Option<i32>>,
    available: &BTreeSet<u32>,
) -> Result<BTreeSet<u32>> {
    let mut selected = BTreeSet::new();
    for value in values {
        match value {
            Some(value) if value >= 0 => {
                let id = value as u32;
                ensure!(available.contains(&id), "unmapped event audio {id}");
                selected.insert(id);
            }
            Some(_) => (),
            None => selected.extend(available),
        }
    }
    Ok(selected)
}

fn audio(source: &Source, library: &Path, bytes: &[u8], messages: &[Message]) -> Result<Selection> {
    let catalogue = |path| -> Result<BTreeSet<u32>> {
        let mappings: Mappings = serde_json::from_slice(&fs::read(library.join(path))?)?;
        Ok(mappings.iter().map(|(id, _)| id).collect())
    };
    let music = catalogue(audio::MUSIC)?;
    let mut effects = catalogue(audio::EFFECTS)?;
    let voices: speech::Mappings = serde_json::from_slice(&fs::read(library.join(audio::SPEECH))?)?;
    let voices = voices.keys().copied().collect();
    let program = Program::decode(bytes)?;
    let analysis = scenario::analyze(bytes)?;
    let arguments = |call, arity, position| -> Result<Vec<Option<i32>>> {
        Ok(
            scenario::calls::analyzed_arguments(&program, &analysis, call, arity)?
                .into_values()
                .map(|args| args[position])
                .collect(),
        )
    };
    let music = select(arguments(NativeCall::AudioCommand, 1, 0)?, &music)?;
    let mut sounds = BTreeSet::new();
    for (call, arity, position) in crate::script_audio::CALLS {
        let values = arguments(call, arity, position)?;
        for &id in values.iter().flatten().filter(|&&id| id >= 0) {
            if !effects.contains(&(id as u32)) {
                crate::audio_effects::bind_unused(source, library, id as u32)?;
                effects.insert(id as u32);
            }
        }
        sounds.extend(select(values, &effects)?);
    }
    let mut requested = arguments(NativeCall::PlayVoice, 1, 0)?;
    for message in messages {
        for token in &message.tokens {
            if let Token::Control {
                opcode: 9,
                expression,
            } = token
            {
                requested.push(expression.constant());
            }
        }
    }
    Ok(Selection {
        sounds: sounds
            .into_iter()
            .map(u16::try_from)
            .collect::<std::result::Result<_, _>>()?,
        streams: BTreeSet::new(),
        voices: select(requested, &voices)?,
        music: music
            .into_iter()
            .map(u16::try_from)
            .collect::<std::result::Result<_, _>>()?,
    })
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let resources = Resources::read(source)?;
    let mut actors = BTreeMap::new();
    for (character, external) in [
        (1, &[0x3008e, 0x300a8][..]),
        (2, &[0x4008e, 0x400a8][..]),
        (3, &[0x5008e, 0x500a8][..]),
        (9, &[0xb008e][..]),
    ] {
        let name = PartyCharacterId::try_from(character)?
            .resource_name()
            .context("unnamed event party actor")?;
        actors.insert(
            name,
            party(source, library, &resources, &image, character, external)?,
        );
    }
    actors.insert("soldier", soldier(source, library, &resources)?);
    actors.insert(
        "vidarr",
        Actor {
            source: ActorSource::Model {
                path: "meshes/monsters/vidarr-0219/model.json".into(),
                body_parts: Some(2),
            },
            clips: Vec::new(),
            appearance: None,
        },
    );
    for (name, actor) in actors {
        output::json(
            &library.join(format!("battle/events/actors/{name}.json")),
            &actor,
        )?;
    }
    let actor_path = |name| format!("battle/events/actors/{name}.json");
    let mut scripts = BTreeMap::new();
    let mut events = BTreeMap::new();
    let mut owners = BTreeMap::<Vec<u8>, String>::new();
    for event in BattleEventId::ALL {
        let id = event.raw();
        let Some((bytes, encoding)) = source.read_encoded(&format!("btl_ev{id:03}.so"))? else {
            ensure!(!matches!(id, 2 | 3), "missing playable battle event {id}");
            continue;
        };
        let decoded = script::decode(&bytes, encoding)?;
        let name = owners
            .entry(decoded.bytes.clone())
            .or_insert_with(|| event.resource_name().unwrap().into())
            .clone();
        let script = format!("battle/scripts/{name}.ssb");
        output::write(&library.join(&script), &decoded.bytes)?;
        let messages =
            (!decoded.messages.is_empty()).then(|| format!("battle/scripts/{name}-messages.json"));
        if let Some(path) = &messages {
            output::json(&library.join(path), &decoded.messages)?;
        }
        scripts.insert(u32::from(id), format!("{name}.ssb"));
        if matches!(id, 2 | 3) {
            let name = event.resource_name().unwrap();
            let mut actors = BTreeMap::from([
                (1, actor_path("lloyd")),
                (2, actor_path("colette")),
                (3, actor_path("genis")),
                (17, actor_path("vidarr")),
            ]);
            if id == 2 {
                actors.insert(0x2005d, actor_path("soldier"));
            } else {
                actors.insert(9, actor_path("kratos"));
            }
            let definition = Definition {
                script,
                messages,
                blink: None,
                audio: audio(source, library, &decoded.bytes, &decoded.messages)
                    .with_context(|| format!("battle event {id} audio"))?,
                actors,
            };
            output::json(
                &library.join(format!("battle/events/{name}/event.json")),
                &definition,
            )?;
            events.insert(u32::from(id), name.into());
        }
    }
    if let Some((bytes, encoding)) = source.read_encoded("btl.so")? {
        let common = script::decode(&bytes, encoding)?;
        output::write(&library.join("battle/scripts/common.ssb"), &common.bytes)?;
        if !common.messages.is_empty() {
            output::json(
                &library.join("battle/scripts/common-messages.json"),
                &common.messages,
            )?;
        }
    }
    let count = scripts.len();
    output::json(
        &library.join("battle/scripts/mappings.json"),
        &Mappings::new(scripts)?,
    )?;
    output::json(&library.join(document::MAPPINGS), &Mappings::new(events)?)?;
    eprintln!("  Battle events: {count} source programs and two playable actor bindings");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with party and enemy models"]
    fn installed_battle_events_cook_and_capture_shared_scenes() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let source = Source::open(
            &std::path::PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        cook(&source, &root)?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        for id in [2, 3] {
            let scene = document::capture(&mut library.snapshot(), id, &diagnostics, &|| false)?;
            ensure!(scene.actors.len() == 5, "incomplete event cast");
            ensure!(
                !scene.audio.sounds.is_empty() && !scene.audio.voices.is_empty(),
                "event audio missing"
            );
            let bytes = fs::read(root.join(scene.script))?;
            Program::decode(&bytes)?;
            scenario::analyze(&bytes)?;
            for actor in scene.actors.values() {
                let mut model_slots = BTreeSet::new();
                for clip in &actor.parts[0].clips {
                    if clip.animation_resource.is_none() {
                        ensure!(
                            model_slots.insert(clip.resource_slot),
                            "duplicate event model clip"
                        );
                    }
                    resonance_content::animation::Motion::decode_for_skeleton(
                        &fs::read(root.join(&clip.motion))?,
                        &actor.skeleton,
                    )?;
                }
            }
        }
        Ok(())
    }
}
