//! One battle party body and its native motion bank, directly to editable assets.
use crate::{
    battle_party_mappings, character_models, compression, dol, enemy_package::EnemyPackages,
    field_core, world_gpl, world_motion,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    SceneClip,
    animation::Skeleton,
    battle_model::document::{PartyDefinition, take_part_animations},
    battle_victory::document::Selection,
    field::document::Model,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::PartyCharacterId;

pub(crate) fn motion_bank(extracted: &Path, character: u8, costume: u8) -> Result<Vec<u8>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let name = battle_party_mappings::source_name(&exe, 0x801f_abf4, character, costume)?;
    let path = battle_party_mappings::native_path(&extracted.join("disc1/files"), &name)?
        .with_context(|| format!("missing native battle motions {name}"))?;
    compression::cabinet(&fs::read(path)?)
}

pub(crate) fn duel_motion(extracted: &Path) -> Result<Vec<u8>> {
    let mut enemies = EnemyPackages::open(&extracted.join("disc1"))?;
    let package = enemies.read(242)?;
    let at = 0x20 + 60 * 4;
    let start = dol::u32(&package, at)? as usize;
    ensure!(
        start >= 0x1e8 && start < package.len(),
        "missing Lloyd duel motion"
    );
    let end = (0x18..0x1e8)
        .step_by(4)
        .map(|field| dol::u32(&package, field).map(|offset| offset as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&offset| offset > start)
        .min()
        .unwrap_or(package.len());
    Ok(package[start..end].to_vec())
}

pub(crate) fn expanded_motion(native: &[u8]) -> Result<Vec<u8>> {
    if dol::u32(native, 0)? == 0x007b_7960 {
        Ok(native.to_vec())
    } else {
        compression::decode(native)
    }
}

fn clip(path: String, slot: u16, pose_chains: Vec<String>) -> SceneClip {
    SceneClip {
        motion: path,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: None,
        pose_chains,
    }
}

fn near_value(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(a), serde_json::Value::Number(b)) => a
            .as_f64()
            .zip(b.as_f64())
            .is_some_and(|(a, b)| (a - b).abs() <= 0.00001),
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| near_value(a, b))
        }
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| near_value(a, b)))
        }
        _ => a == b,
    }
}

/// A party battle rig and its native motion bank.
pub fn cook_battle_party_model(
    extracted: &Path,
    library: &Path,
    character: u8,
    costume: u8,
) -> Result<()> {
    ensure!(
        (1..=9).contains(&character) && costume < 5,
        "invalid party costume"
    );
    let name = PartyCharacterId::try_from(character)?
        .resource_name()
        .context("unnamed party character")?;
    let variant = battle_party_mappings::costume_name(character, costume)?;
    for earlier in 0..costume {
        ensure!(
            battle_party_mappings::costume_name(character, earlier)? != variant,
            "costume {character}:{costume} shares a cooked name with costume {earlier}; cook its canonical owner"
        );
    }
    let base = format!("battle/party/{name}/{variant}");
    let default = format!("battle/party/{name}/default");
    if costume != 0 {
        ensure!(
            library
                .join(format!("{default}/animations/idle.glb"))
                .is_file(),
            "cook the default battle body before its costumes"
        );
    }
    let body_name = character_models::party_body_name(character, costume)?;
    let model_base = if body_name == "default" {
        format!("meshes/characters/{name}")
    } else {
        format!("meshes/characters/{name}/{body_name}")
    };
    let model: Model = serde_json::from_slice(
        &fs::read(library.join(format!("{model_base}/model.json")))
            .with_context(|| format!("cook character model {model_base} before battle"))?,
    )?;
    let [mut body_part, mut outline_part]: [_; 2] = model
        .parts
        .try_into()
        .map_err(|_| anyhow::anyhow!("party model requires its body and outline"))?;
    let body_glb = fs::read(library.join(&body_part.mesh))?;
    body_part.prepare_bones(&body_glb)?;
    let body = character_models::party_body(extracted, character, costume)?;
    let bindings = world_gpl::animation_bindings(&body)?;
    // Native motion bytes can be reused only when the receiving rig has the
    // same joint hierarchy and bind pose; costume GPL floats can differ by a
    // few millionths after conversion.
    let shared_rig = if costume == 0 {
        false
    } else {
        let default_glb =
            fs::read(library.join(format!("meshes/characters/{name}/meshes/body.glb")))?;
        near_value(
            &serde_json::to_value(Skeleton::from_glb(&body_glb)?)?,
            &serde_json::to_value(Skeleton::from_glb(&default_glb)?)?,
        )
    };
    let attachment_nodes = body_part
        .bone_names
        .iter()
        .filter_map(|name| {
            name.strip_prefix("kk")?
                .parse::<u8>()
                .ok()
                .map(|slot| (slot, name.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        !attachment_nodes.is_empty(),
        "party battle weapon targets are missing"
    );

    let bank = motion_bank(extracted, character, costume)?;
    let default_bank = shared_rig
        .then(|| motion_bank(extracted, character, 0))
        .transpose()?;
    let count = dol::u32(&bank, 0)? as usize;
    ensure!((3..=256).contains(&count), "invalid battle motion bank");
    let mut files = BTreeMap::new();
    let mut clips = Vec::new();
    let mut null_motions = BTreeSet::new();
    for index in 2..count {
        let slot = u16::try_from(index - 2)?;
        let Some(native) = field_core::optional_section(&bank, index)? else {
            null_motions.insert(slot);
            continue;
        };
        let motion = expanded_motion(native)?;
        let reused = if let Some(default) = &default_bank {
            field_core::optional_section(default, index)?
                .map(expanded_motion)
                .transpose()?
                .is_some_and(|baseline| baseline == motion)
        } else {
            false
        };
        let owner = if reused { &default } else { &base };
        let path = format!(
            "{owner}/animations/{}.glb",
            resonance_content::battle_motion::name(slot)
        );
        if reused {
            ensure!(
                library.join(&path).is_file(),
                "shared party motion {slot} is not cooked"
            );
        } else {
            let glb = world_motion::encode_bound(&motion, &body_glb, &bindings)
                .with_context(|| format!("party {character}:{costume} battle motion {slot}"))?;
            ensure!(
                files.insert(path.clone(), glb).is_none(),
                "duplicate battle motion path"
            );
        }
        let pose_chains = character_models::pose_chains(
            &motion,
            &bindings,
            &body_part.bone_names,
            &body_part.secondary_motion,
        )?;
        clips.push(clip(path, slot, pose_chains));
    }
    if character == 1 {
        let motion = expanded_motion(&duel_motion(extracted)?)?;
        let owner = if shared_rig { &default } else { &base };
        let path = format!("{owner}/animations/slot-100.glb");
        if shared_rig {
            ensure!(
                library.join(&path).is_file(),
                "shared Lloyd duel motion is not cooked"
            );
        } else {
            let glb = world_motion::encode_bound(&motion, &body_glb, &bindings)?;
            ensure!(
                files.insert(path.clone(), glb).is_none(),
                "duplicate duel motion"
            );
        }
        clips.push(clip(
            path,
            100,
            character_models::pose_chains(
                &motion,
                &bindings,
                &body_part.bone_names,
                &body_part.secondary_motion,
            )?,
        ));
    }
    body_part.clips = clips.clone();
    outline_part.clips = clips;
    let mut parts = vec![body_part, outline_part];
    let animations = take_part_animations(&mut parts);
    let document = PartyDefinition {
        attachment_nodes,
        parts,
        animations,
        null_motions,
        // The shared field wing document also supplies battle's idle slot 12.
        wings: (character == 2).then(|| "meshes/characters/colette/wings/model.json".into()),
        victory: Selection {
            file: format!("battle/party/{name}/victory.json"),
            costume: variant,
        },
    };
    files.insert(
        format!("{base}/party.json"),
        resonance_asset_writer::json::encode(&document)?,
    );
    ensure!(
        files.keys().all(|path| !library.join(path).exists()),
        "battle party output already exists"
    );
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}
