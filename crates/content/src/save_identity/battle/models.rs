//! Contact rigs and motion semantics. Meshes, images, UV animation, preview
//! framing and file locations are not inputs to saved gameplay compatibility.
use super::{ids, record_json, version};
use crate::{
    ScenePart,
    animation::Motion,
    battle_model::document as models,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Context, Result, ensure};
use sha2::Sha256;
use std::collections::BTreeMap;

pub(super) fn party_and_equipment(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    for character in ids(snapshot, models::PARTY)? {
        let costumes = snapshot.resolve(models::PARTY, character, Some("mappings.json"))?;
        for costume in ids(snapshot, &costumes)? {
            ensure!(!cancelled(), "battle identity preparation cancelled");
            let path = snapshot.resolve(&costumes, costume, Some("party.json"))?;
            let entry: models::PartyDefinition = snapshot.json(&path, JSON_LIMIT)?;
            version(entry.version, models::VERSION, &path)?;
            let name = format!("battle/party/{character}/{costume}");
            rig(snapshot, hash, &name, &entry.rig)?;
            record_json(hash, &format!("{name}/null-motions"), &entry.null_motions)?;
            let model: models::Visuals = snapshot.json(&entry.model, JSON_LIMIT)?;
            version(model.version, models::VERSION, &entry.model)?;
            parts(
                snapshot,
                hash,
                &name,
                std::slice::from_ref(model.parts.first().context("party model has no body")?),
                cancelled,
            )?;
        }
    }
    for item in ids(snapshot, models::EQUIPMENT)? {
        ensure!(!cancelled(), "battle identity preparation cancelled");
        let path = snapshot.resolve(models::EQUIPMENT, item, Some("equipment.json"))?;
        let entry: models::Equipment = snapshot.json(&path, JSON_LIMIT)?;
        version(entry.version, models::VERSION, &path)?;
        let name = format!("battle/equipment/{item}");
        match entry.attachment {
            models::AttachmentDefinition::Weapon(value) => {
                record_json(hash, &name, &"weapon")?;
                weapon(snapshot, hash, &name, value, cancelled)?;
            }
            models::AttachmentDefinition::Shield(value) => {
                record_json(hash, &name, &"shield")?;
                weapon(snapshot, hash, &name, value, cancelled)?;
            }
            models::AttachmentDefinition::Nonvisual => record_json(hash, &name, &"nonvisual")?,
        }
    }
    Ok(())
}

pub(super) fn enemy(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    id: u32,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let entry: models::enemy::Definition = snapshot.json(path, JSON_LIMIT)?;
    version(entry.version, models::VERSION, path)?;
    let name = format!("battle/enemy/{id}/model");
    rig(snapshot, hash, &name, &entry.rig)?;
    let preview =
        crate::model_preview::document::read(&entry.model, |path| snapshot.read(path, JSON_LIMIT))?;
    parts(
        snapshot,
        hash,
        &name,
        std::slice::from_ref(
            &preview
                .parts
                .first()
                .context("enemy model has no body")?
                .scene,
        ),
        cancelled,
    )?;
    if let Some(secondary) = entry.secondary_body {
        let name = format!("{name}/secondary");
        record_json(
            hash,
            &name,
            &(secondary.model_slot, secondary.motion_offset),
        )?;
        model(snapshot, hash, &name, &secondary.model, cancelled)?;
    }
    for (index, binding) in entry.attached_models.iter().enumerate() {
        record_json(
            hash,
            &format!("{name}/binding/{index}"),
            &(
                binding.model_slot,
                binding.motion_slot,
                binding.placement,
                binding.transform,
            ),
        )?;
    }
    for (slot, path) in entry.attachments {
        model(
            snapshot,
            hash,
            &format!("{name}/attachment/{slot}"),
            &path,
            cancelled,
        )?;
    }
    for (slot, variant) in entry.weapon_variants {
        model(
            snapshot,
            hash,
            &format!("{name}/weapon-variant/{slot}"),
            &variant.model,
            cancelled,
        )?;
    }
    Ok(())
}

pub(super) fn weapon(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    weapon: models::WeaponDefinition,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    for (slot, path) in weapon.parts {
        model(
            snapshot,
            hash,
            &format!("{name}/weapon/{slot}"),
            &path,
            cancelled,
        )?;
    }
    Ok(())
}

pub(super) fn effect_models(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    use crate::battle_effect::document as effects;
    ensure!(!cancelled(), "battle identity preparation cancelled");
    let entry: effects::Definition = snapshot.json(path, JSON_LIMIT)?;
    version(entry.version, effects::VERSION, path)?;
    if let Some(path) = entry.art {
        let artwork: effects::Artwork = snapshot.json(&path, JSON_LIMIT)?;
        version(artwork.version, effects::VERSION, &path)?;
        for (slot, path) in artwork.models {
            model(
                snapshot,
                hash,
                &format!("{name}/effect-model/{slot}"),
                &path,
                cancelled,
            )?;
        }
    }
    Ok(())
}

fn rig(snapshot: &mut Snapshot, hash: &mut Sha256, name: &str, path: &str) -> Result<()> {
    let rig: crate::battle_model::Rig = snapshot.json(path, JSON_LIMIT)?;
    rig.skeleton.validate()?;
    record_json(hash, &format!("{name}/rig"), &rig)
}

fn model(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    ensure!(!cancelled(), "battle identity preparation cancelled");
    let entry: models::Model = snapshot.json(path, JSON_LIMIT)?;
    version(entry.version, models::VERSION, path)?;
    entry.model.rig.skeleton.validate()?;
    record_json(hash, &format!("{name}/rig"), &entry.model.rig)?;
    record_json(
        hash,
        &format!("{name}/layer-rigs"),
        &entry.model.layer_skeletons,
    )?;
    previews(snapshot, hash, name, &entry.model.layers, cancelled)
}

fn previews(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    layers: &[crate::model_preview::PreviewPart],
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    for (index, layer) in layers.iter().enumerate() {
        let name = format!("{name}/layer/{index}");
        record_json(hash, &format!("{name}/binding"), &layer.animation)?;
        parts(
            snapshot,
            hash,
            &name,
            std::slice::from_ref(&layer.scene),
            cancelled,
        )?;
    }
    Ok(())
}

pub(super) fn parts(
    snapshot: &mut Snapshot,
    hash: &mut Sha256,
    name: &str,
    parts: &[ScenePart],
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    for (index, part) in parts.iter().enumerate() {
        ensure!(!cancelled(), "motion identity preparation cancelled");
        let name = format!("{name}/part/{index}");
        for chain in &part.secondary_motion.chains {
            chain.validate(part.bone_names.len())?;
        }
        record_json(
            hash,
            &name,
            &(part.resource, &part.bone_names, &part.secondary_motion),
        )?;
        let mut clips = BTreeMap::new();
        for clip in &part.clips {
            ensure!(
                clips
                    .insert((clip.animation_resource, clip.resource_slot), &clip.motion)
                    .is_none(),
                "duplicate gameplay motion slot in {name}"
            );
        }
        for ((resource, slot), path) in clips {
            ensure!(!cancelled(), "motion identity preparation cancelled");
            let motion = Motion::decode(&snapshot.read(path, ASSET_LIMIT)?)?;
            motion.validate_bones(part.bone_names.len())?;
            record_json(hash, &format!("{name}/motion/{resource:?}/{slot}"), &motion)?;
        }
    }
    Ok(())
}
