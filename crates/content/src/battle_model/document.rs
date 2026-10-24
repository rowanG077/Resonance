//! Named battle bodies and their context-specific motion banks.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Context, Result, ensure};

pub const VERSION: u32 = 1;
pub const PARTY: &str = "battle/party/mappings.json";
pub const EQUIPMENT: &str = "meshes/equipment/mappings.json";

pub mod enemy;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub version: u32,
    pub model: ModelPart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equipment {
    pub version: u32,
    pub attachment: AttachmentDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentDefinition {
    Weapon(WeaponDefinition),
    Shield(WeaponDefinition),
    Nonvisual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDefinition {
    pub parts: BTreeMap<u8, String>,
    pub trails: BTreeMap<u8, TrailMaterial>,
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
}

pub fn capture_equipment(
    snapshot: &mut Snapshot,
    item: u16,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Attachment> {
    ensure!(!cancelled(), "equipment preparation cancelled");
    let path = snapshot.resolve(EQUIPMENT, u32::from(item), Some("equipment.json"))?;
    let definition: Equipment = snapshot.json(&path, JSON_LIMIT)?;
    ensure!(
        definition.version == VERSION,
        "unsupported equipment version in {path}"
    );
    let (weapon, shield) = match definition.attachment {
        AttachmentDefinition::Nonvisual => return Ok(Attachment::Nonvisual),
        AttachmentDefinition::Weapon(weapon) => (weapon, false),
        AttachmentDefinition::Shield(weapon) => (weapon, true),
    };
    let mut parts = BTreeMap::new();
    for (slot, path) in weapon.parts {
        let model = read_model(&path, &mut |resource| {
            ensure!(!cancelled(), "equipment preparation cancelled");
            snapshot.read(resource, ASSET_LIMIT)
        })?;
        for texture in model.layers.iter().flat_map(|layer| &layer.scene.textures) {
            ensure!(!cancelled(), "equipment preparation cancelled");
            diagnostics.attempt(&path, snapshot.read(texture, ASSET_LIMIT))?;
        }
        parts.insert(slot, model);
    }
    ensure!(!cancelled(), "equipment preparation cancelled");
    let weapon = Weapon {
        source_sha256: String::new(),

        parts,
        trails: weapon.trails,
        uv_animation: weapon.uv_animation,
    };
    Ok(if shield {
        Attachment::Shield(weapon)
    } else {
        Attachment::Weapon(weapon)
    })
}

/// A scene-owned temporary weapon uses the same editable model bindings as an
/// equipment weapon, without inventing an equipment item ID.
pub fn read_weapon(
    definition: WeaponDefinition,
    read: &mut impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
) -> Result<Weapon> {
    let parts = definition
        .parts
        .into_iter()
        .map(|(slot, path)| Ok((slot, read_model(&path, read)?)))
        .collect::<Result<_>>()?;
    Ok(Weapon {
        source_sha256: String::new(),

        parts,
        trails: definition.trails,
        uv_animation: definition.uv_animation,
    })
}

pub fn read_model(
    path: &str,
    read: &mut impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
) -> Result<ModelPart> {
    crate::library::validate_path(path)?;
    let bytes = read(path)?;
    ensure!(
        bytes.len() <= JSON_LIMIT,
        "model document exceeds size limit: {path}"
    );
    let mut model: Model =
        serde_json::from_slice(&bytes).with_context(|| format!("decode battle model {path}"))?;
    ensure!(
        model.version == VERSION,
        "unsupported battle model version in {path}"
    );
    ensure!(
        !model.model.layers.is_empty(),
        "empty battle model in {path}"
    );
    for part in &mut model.model.layers {
        prepare_part(part, &mut *read).with_context(|| path.to_owned())?;
    }
    model.model.rig.skeleton.validate()?;
    Ok(model.model)
}

/// Derive timing from current clips; runtime admission checks indexed rig and
/// attachment compatibility. Image payload errors remain independently optional.
pub fn prepare_part(
    part: &mut crate::model_preview::PreviewPart,
    mut read: impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
) -> Result<()> {
    read(&part.scene.mesh)?;
    for clip in &mut part.scene.clips {
        let motion = crate::animation::Motion::decode(&read(&clip.motion)?)
            .with_context(|| format!("decode battle motion {}", clip.motion))?;
        motion.validate_bones(part.scene.bone_names.len())?;
        clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
    }
    part.scene.validate(|_| true)?;
    part.selected_clip()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyDefinition {
    pub version: u32,
    pub rig: String,
    pub model: String,
    pub null_motions: BTreeSet<u16>,
    pub wings: Option<String>,
    pub victory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visuals {
    pub version: u32,
    pub parts: Vec<crate::ScenePart>,
}

pub fn party_path(snapshot: &mut Snapshot, character: u8, costume: u8) -> Result<String> {
    ensure!(
        (1..=9).contains(&character),
        "invalid party character {character}"
    );
    let costumes = snapshot.resolve(PARTY, u32::from(character), Some("mappings.json"))?;
    snapshot.resolve(&costumes, u32::from(costume), Some("party.json"))
}

pub fn capture_party(
    snapshot: &mut Snapshot,
    character: u8,
    costume: u8,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<(PartyDefinition, Party)> {
    ensure!(!cancelled(), "party preparation cancelled");
    let path = party_path(snapshot, character, costume)?;
    let definition: PartyDefinition = snapshot.json(&path, JSON_LIMIT)?;
    ensure!(
        definition.version == VERSION,
        "unsupported party version in {path}"
    );
    crate::library::validate_path(&definition.victory)?;
    let body: Rig = snapshot.json(&definition.rig, JSON_LIMIT)?;
    body.skeleton.validate()?;
    ensure!(
        body.attachments
            .values()
            .all(|&bone| usize::from(bone) < body.skeleton.bones.len()),
        "invalid party attachment in {}",
        definition.rig
    );
    let parts = capture_visuals(snapshot, &definition.model, diagnostics, cancelled)?;
    ensure!(
        body.skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(parts[0].bone_names.iter()),
        "party rig differs from model in {path}"
    );
    ensure!(
        parts
            .iter()
            .flat_map(|part| &part.clips)
            .all(|clip| !definition.null_motions.contains(&clip.resource_slot)),
        "party motion is both empty and populated in {path}"
    );
    let wings = if let Some(path) = &definition.wings {
        let result =
            capture_visuals(snapshot, path, diagnostics, cancelled).and_then(|mut parts| {
                ensure!(parts.len() == 1, "party wings need one model in {path}");
                Ok(parts.remove(0))
            });
        ensure!(!cancelled(), "party preparation cancelled");
        diagnostics.attempt(path, result)?
    } else {
        None
    };
    let party = Party {
        body_sha256: String::new(),
        motion_sha256: String::new(),

        body,
        parts,
        wings,
        null_motions: definition.null_motions.clone(),
    };
    ensure!(!cancelled(), "party preparation cancelled");
    Ok((definition, party))
}

fn capture_visuals(
    snapshot: &mut Snapshot,
    path: &str,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<crate::ScenePart>> {
    ensure!(!cancelled(), "party model preparation cancelled");
    let mut model: Visuals = snapshot.json(path, JSON_LIMIT)?;
    ensure!(
        model.version == VERSION && !model.parts.is_empty(),
        "invalid party model in {path}"
    );
    for part in &mut model.parts {
        capture_scene(snapshot, path, part, diagnostics, cancelled)?;
    }
    Ok(model.parts)
}

/// Capture editable body resources with independently optional images and clips.
/// Keep failed clip declarations for the runtime's existing admission diagnostics.
pub(crate) fn capture_scene(
    snapshot: &mut Snapshot,
    path: &str,
    part: &mut crate::ScenePart,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    part.validate(|_| true).with_context(|| path.to_owned())?;
    ensure!(!cancelled(), "battle model preparation cancelled");
    snapshot
        .read(&part.mesh, ASSET_LIMIT)
        .with_context(|| format!("{path} mesh {}", part.mesh))?;
    for texture in &part.textures {
        ensure!(!cancelled(), "battle model preparation cancelled");
        diagnostics.attempt(path, snapshot.read(texture, ASSET_LIMIT))?;
    }
    let mut slots = BTreeSet::new();
    for clip in &mut part.clips {
        ensure!(
            slots.insert((clip.animation_resource, clip.resource_slot)),
            "duplicate battle motion slot in {path}"
        );
        ensure!(!cancelled(), "battle motion preparation cancelled");
        let result = (|| {
            let motion =
                crate::animation::Motion::decode(&snapshot.read(&clip.motion, ASSET_LIMIT)?)?;
            motion.validate_bones(part.bone_names.len())?;
            Ok(motion)
        })();
        if let Some(motion) = diagnostics.attempt(&clip.motion, result)? {
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
    }
    ensure!(!cancelled(), "battle model preparation cancelled");
    Ok(())
}
