//! Battle event actors reference shared models and publish only their own motions.
use crate::{
    battle_party_mappings, battle_party_model, character_models, dol, field_core, text, world_gpl,
    world_motion,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, AtlasChannel, SceneClip,
    battle_audio::Selection,
    battle_event::document::{Actor, ActorSource, Definition},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use symphonia_constants::{BattleEventId, PartyCharacterId};

/// Read one bounded member from a DOL-declared native resource archive.
fn resource(root: &Path, exe: &[u8], id: u32) -> Result<Vec<u8>> {
    let group = id >> 16;
    ensure!(
        (1..=14).contains(&group),
        "unsupported battle event resource group"
    );
    let row = dol::slice(exe, 0x801f_86b8 + (group - 1) * 12, 4)?;
    let declared = text::string(exe, dol::u32(row, 0)?)?;
    let path = battle_party_mappings::native_path(&root.join("disc1/files"), &declared)?
        .with_context(|| format!("missing event resource archive {declared}"))?;
    let mut file = File::open(&path)?;
    let size = file.metadata()?.len();
    let mut header = [0; 4];
    file.read_exact(&mut header)?;
    let count = u64::from(u32::from_be_bytes(header));
    let index = u64::from(id & 0xffff);
    ensure!(
        index < count && 4 + count * 8 <= size,
        "invalid event resource index {id:#x}"
    );
    file.seek(SeekFrom::Start(4 + index * 8))?;
    let mut entry = [0; 8];
    file.read_exact(&mut entry)?;
    if entry[4..] == [0; 4] {
        file.seek(SeekFrom::Start(4))?;
        file.read_exact(&mut entry)?;
    }
    let start = u64::from(u32::from_be_bytes(entry[..4].try_into()?));
    let length = u64::from(u32::from_be_bytes(entry[4..].try_into()?));
    ensure!(
        start >= 4 + count * 8 && length > 0 && start + length <= size,
        "event resource {id:#x} exceeds archive"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; usize::try_from(length)?];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn clip(motion: String, slot: u16, bank: Option<u32>) -> SceneClip {
    SceneClip {
        motion,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: bank,
        pose_chains: Vec::new(),
    }
}

fn party_actor(root: &Path, exe: &[u8], character: u8, external: &[u32]) -> Result<Actor> {
    let name = PartyCharacterId::try_from(character)?
        .resource_name()
        .context("unnamed event party character")?;
    let bank = battle_party_model::motion_bank(root, character, 0)?;
    let clips = external
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let slot = if index == 0 { 0 } else { 26 };
            let native = battle_party_model::expanded_motion(&resource(root, exe, id)?)?;
            let shared = field_core::optional_section(&bank, slot + 2)?
                .context("event clip has no shared party motion")?;
            ensure!(
                native == battle_party_model::expanded_motion(shared)?,
                "event character {name} motion {id:#x} differs from shared slot {slot}"
            );
            Ok(clip(
                format!(
                    "battle/party/{name}/default/animations/{}.glb",
                    resonance_content::battle_motion::name(slot as u16)
                ),
                0,
                Some(id),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Actor {
        source: ActorSource::Party {
            path: format!("battle/party/{name}/default/party.json"),
        },
        clips,
        appearance: None,
    })
}

fn soldier(root: &Path, exe: &[u8]) -> Result<(Actor, Vec<(String, Vec<u8>)>)> {
    let package = resource(root, exe, 0x2005d)?;
    let body = field_core::optional_section(&package, 0)?.context("soldier body is absent")?;
    let (paths, alpha, _) = character_models::texture_paths(
        character_models::palette(body)?,
        "meshes/characters/renegade",
        "body",
        |_, _| {},
    )?;
    let (glb, _) = world_gpl::static_model(body, paths, alpha)?;
    let bindings = world_gpl::animation_bindings(body)?;
    let base = "battle/events/models/soldier-0002005d/animations";
    let mut files = Vec::new();
    let mut clips = Vec::new();
    for slot in [3_u16, 6, 7, 8, 9, 10, 12, 14, 15, 16, 17] {
        let source = field_core::optional_section(&package, usize::from(slot) + 2)?
            .with_context(|| format!("soldier motion {slot} is absent"))?;
        let expanded = battle_party_model::expanded_motion(source)?;
        let path = format!("{base}/motion-{slot}.glb");
        files.push((
            path.clone(),
            world_motion::encode_bound(&expanded, &glb, &bindings)?,
        ));
        clips.push(clip(path, slot, None));
    }
    let aliased =
        field_core::optional_section(&package, 15)?.context("soldier motion 13 is absent")?;
    let original =
        field_core::optional_section(&package, 12)?.context("soldier motion 10 is absent")?;
    ensure!(
        aliased == original,
        "soldier motion 13 does not alias motion 10"
    );
    clips.push(clip(format!("{base}/motion-10.glb"), 13, None));
    let external = battle_party_model::expanded_motion(&resource(root, exe, 0x10aa0)?)?;
    let local = battle_party_model::expanded_motion(
        field_core::optional_section(&package, 9)?.context("soldier motion 7 is absent")?,
    )?;
    ensure!(
        external == local,
        "soldier external pose differs from shared local motion 7"
    );
    clips.push(clip(format!("{base}/motion-7.glb"), 0, Some(0x10aa0)));
    Ok((
        Actor {
            source: ActorSource::Model {
                path: "meshes/figurines/renegade-0150/model.json".into(),
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
        },
        files,
    ))
}

fn write(library: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    crate::output::write(&library.join(path), bytes)
}

fn actor_path(name: &str) -> String {
    format!("battle/events/actors/{name}.json")
}

/// Resource selections for the two fixed native SSB programs. Sound names are
/// catalogue IDs; this small maintained list lives beside their event recipes.
fn audio(rescue: bool) -> Selection {
    let mut sounds: BTreeSet<u16> = [
        1, 2, 3, 4, 6, 28, 29, 30, 31, 32, 33, 38, 41, 42, 43, 62, 87, 104, 107, 110, 129, 130,
        132, 191, 194, 236, 263, 270, 272, 287, 289, 330, 414,
    ]
    .into_iter()
    .collect();
    let voices = if rescue {
        sounds.extend([54, 63, 72, 195, 199]);
        [655401, 655402, 655403, 655404].into_iter().collect()
    } else {
        sounds.insert(111);
        [655400].into_iter().collect()
    };
    Selection {
        sounds,
        streams: BTreeSet::new(),
        voices,
        music: BTreeSet::new(),
    }
}

/// Convert shared actor references and the soldier's native local motions.
pub fn cook_battle_event_media(root: &Path, library: &Path, selected: Option<u16>) -> Result<()> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let events = match selected {
        Some(id) => {
            ensure!(
                matches!(id, 2 | 3),
                "battle event {id} has no authored actor binding"
            );
            vec![BattleEventId::try_from(id)?]
        }
        None => vec![BattleEventId::VidarrOpening, BattleEventId::VidarrRescue],
    };
    let common = [
        ("lloyd", party_actor(root, &exe, 1, &[0x3008e, 0x300a8])?),
        ("colette", party_actor(root, &exe, 2, &[0x4008e, 0x400a8])?),
        ("genis", party_actor(root, &exe, 3, &[0x5008e, 0x500a8])?),
        (
            "vidarr",
            Actor {
                source: ActorSource::Model {
                    path: "meshes/monsters/vidarr-0219/model.json".into(),
                    body_parts: Some(2),
                },
                clips: Vec::new(),
                appearance: None,
            },
        ),
    ];
    for (name, actor) in common {
        write(
            library,
            &actor_path(name),
            &resonance_asset_writer::json::encode(&actor)?,
        )?;
    }
    for event in events {
        let name = event.resource_name().context("battle event has no name")?;
        let mut actors = BTreeMap::from([
            (1, actor_path("lloyd")),
            (2, actor_path("colette")),
            (3, actor_path("genis")),
            (17, actor_path("vidarr")),
        ]);
        if event.raw() == 2 {
            let (soldier, files) = soldier(root, &exe)?;
            write(
                library,
                &actor_path("soldier"),
                &resonance_asset_writer::json::encode(&soldier)?,
            )?;
            actors.insert(0x2005d, actor_path("soldier"));
            for (path, bytes) in files {
                write(library, &path, &bytes)?;
            }
        } else {
            let kratos = party_actor(root, &exe, 9, &[0xb008e])?;
            write(
                library,
                &actor_path("kratos"),
                &resonance_asset_writer::json::encode(&kratos)?,
            )?;
            actors.insert(9, actor_path("kratos"));
        }
        let definition = Definition {
            script: format!("battle/scripts/{name}.ssb"),
            messages: Some(format!("battle/scripts/{name}-messages.json")),
            blink: None,
            audio: audio(event.raw() == 3),
            actors,
        };
        write(
            library,
            &format!("battle/events/{name}/event.json"),
            &resonance_asset_writer::json::encode(&definition)?,
        )?;
        ensure!(
            matches!(name, "vidarr-opening" | "vidarr-rescue"),
            "unknown event media owner"
        );
    }
    Ok(())
}
