//! Named battle bodies and their context-specific motion banks.
use super::*;
use crate::{
    diagnostics::Diagnostics,
    library::Snapshot,
    preparation::{ASSET_LIMIT, JSON_LIMIT},
};
use anyhow::{Context, Result, ensure};

pub const PARTY: &str = "battle/party/mappings.json";
pub const EQUIPMENT: &str = "meshes/equipment/mappings.json";

pub mod enemy;
pub mod trails;
pub use trails::{TrailDefinition, TrailStyle};
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachment_nodes: BTreeMap<u8, String>,
    /// Layers with their own motion clock and rig; all others use the first layer.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub independent_layers: BTreeSet<u16>,
    #[serde(flatten, with = "crate::model_parts::layers")]
    pub layers: Vec<crate::model_preview::PreviewPart>,
}

impl TryFrom<ModelPart> for Model {
    type Error = anyhow::Error;
    fn try_from(model: ModelPart) -> Result<Self> {
        Ok(Self {
            attachment_nodes: model.rig.attachment_names()?,
            independent_layers: model.layer_skeletons.into_keys().collect(),
            layers: model.layers,
        })
    }
}

impl Model {
    pub fn prepare(
        mut self,
        read: &mut impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
    ) -> Result<ModelPart> {
        for layer in &mut self.layers {
            prepare_part(layer, &mut *read)?;
        }
        self.assemble(read)
    }

    /// Assemble only the rigs needed by these prepared layers. Geometry, rest
    /// transforms and attachment sampling have one editable owner: the GLB.
    fn assemble(
        self,
        mut read: impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
    ) -> Result<ModelPart> {
        ensure!(!self.layers.is_empty(), "empty battle model");
        let primary = &self.layers[0].scene;
        ensure!(
            !self.independent_layers.contains(&primary.resource),
            "primary layer cannot be independent"
        );
        let rig = Rig::from_glb(&read(&primary.mesh)?, &self.attachment_nodes)?;
        let mut layer_skeletons = BTreeMap::new();
        for resource in self.independent_layers {
            let mut matching = self
                .layers
                .iter()
                .filter(|layer| layer.scene.resource == resource);
            let layer = matching.next().context("independent layer is absent")?;
            ensure!(matching.next().is_none(), "independent layer is ambiguous");
            layer_skeletons.insert(resource, Skeleton::from_glb(&read(&layer.scene.mesh)?)?);
        }
        Ok(ModelPart {
            rig,
            layer_skeletons,
            layers: self.layers,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Equipment {
    pub kind: EquipmentKind,
    pub parts: BTreeMap<u8, Model>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trails: BTreeMap<u8, TrailDefinition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentKind {
    Weapon,
    Shield,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDefinition {
    pub parts: BTreeMap<u8, Model>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trails: BTreeMap<u8, TrailDefinition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
}

impl From<Equipment> for WeaponDefinition {
    fn from(equipment: Equipment) -> Self {
        Self {
            parts: equipment.parts,
            trails: equipment.trails,
            uv_animation: equipment.uv_animation,
        }
    }
}

/// An explicit null means the item has no visual attachment. A missing ID is an
/// error, rather than silently turning broken or incomplete content invisible.
pub fn equipment_mappings(snapshot: &mut Snapshot) -> Result<BTreeMap<u32, Option<String>>> {
    snapshot.json(EQUIPMENT, JSON_LIMIT)
}

pub fn equipment_path(snapshot: &mut Snapshot, item: u32) -> Result<Option<String>> {
    let mappings = equipment_mappings(snapshot)?;
    let name = mappings
        .get(&item)
        .with_context(|| format!("unmapped equipment item {item}"))?;
    name.as_ref()
        .map(|name| {
            crate::library::validate_path(name)?;
            Ok(format!("meshes/equipment/{name}/equipment.json"))
        })
        .transpose()
}

pub fn capture_equipment(
    snapshot: &mut Snapshot,
    item: u16,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Attachment> {
    ensure!(!cancelled(), "equipment preparation cancelled");
    let Some(path) = equipment_path(snapshot, u32::from(item))? else {
        return Ok(Attachment::Nonvisual);
    };
    let definition: Equipment = snapshot.json(&path, JSON_LIMIT)?;
    let shield = definition.kind == EquipmentKind::Shield;
    let weapon = read_weapon(definition.into(), diagnostics, &mut |resource| {
        ensure!(!cancelled(), "equipment preparation cancelled");
        snapshot.read(resource, ASSET_LIMIT)
    })
    .with_context(|| path.clone())?;
    for model in weapon.parts.values() {
        for texture in model.layers.iter().flat_map(|layer| &layer.scene.textures) {
            ensure!(!cancelled(), "equipment preparation cancelled");
            diagnostics.attempt(&path, snapshot.read(texture, ASSET_LIMIT))?;
        }
    }
    ensure!(!cancelled(), "equipment preparation cancelled");
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
    diagnostics: &Diagnostics,
    read: &mut impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
) -> Result<Weapon> {
    let parts = definition
        .parts
        .into_iter()
        .map(|(slot, model)| Ok((slot, model.prepare(read)?)))
        .collect::<Result<_>>()?;
    let trails = trails::prepare_trails(definition.trails, &parts, diagnostics, read)?;
    Ok(Weapon {
        parts,
        trails,
        uv_animation: definition.uv_animation,
    })
}

/// Derive timing from current clips; runtime admission checks indexed rig and
/// attachment compatibility. Image payload errors remain independently optional.
pub fn prepare_part(
    part: &mut crate::model_preview::PreviewPart,
    mut read: impl FnMut(&str) -> Result<std::sync::Arc<[u8]>>,
) -> Result<()> {
    part.scene
        .prepare_bones(&read(&part.scene.mesh)?)
        .with_context(|| part.scene.mesh.clone())?;
    for clip in &mut part.scene.clips {
        let motion = clip
            .decode_motion(&read(&clip.motion)?, &part.scene.bone_names)
            .with_context(|| format!("decode battle motion {}", clip.motion))?;
        clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
    }
    part.scene.validate(|_| true)?;
    part.selected_clip()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyDefinition {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachment_nodes: BTreeMap<u8, String>,
    #[serde(flatten, with = "crate::model_parts")]
    pub parts: Vec<crate::ScenePart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<PartAnimations>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub null_motions: BTreeSet<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wings: Option<String>,
    pub victory: crate::battle_victory::document::Selection,
}

/// A costume's body and outline normally share their complete clip bindings.
/// Keep one declaration with explicit targets; independent parts keep their own
/// group. No inheritance, lookup file or recursive reference is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartAnimations {
    pub parts: Vec<usize>,
    #[serde(with = "crate::scene_clips")]
    pub clips: Vec<crate::SceneClip>,
}

impl PartyDefinition {
    pub fn take_model_parts(&mut self) -> Result<Vec<crate::ScenePart>> {
        ensure!(
            self.parts.iter().all(|part| part.clips.is_empty()),
            "party clips belong in the costume's animations section"
        );
        let mut parts = std::mem::take(&mut self.parts);
        let mut assigned = BTreeSet::new();
        for group in &self.animations {
            ensure!(
                !group.parts.is_empty(),
                "party animation group has no target parts"
            );
            for &index in &group.parts {
                ensure!(
                    assigned.insert(index),
                    "party part {index} has multiple animation groups"
                );
                let part = parts
                    .get_mut(index)
                    .with_context(|| format!("missing party animation part {index}"))?;
                part.clips = group.clips.clone();
            }
        }
        Ok(parts)
    }
}

/// Group complete equal binding lists after their paths have been published.
pub fn take_part_animations(parts: &mut [crate::ScenePart]) -> Vec<PartAnimations> {
    let mut groups = Vec::<PartAnimations>::new();
    for (index, part) in parts.iter_mut().enumerate() {
        let clips = std::mem::take(&mut part.clips);
        if clips.is_empty() {
            continue;
        }
        if let Some(group) = groups.iter_mut().find(|group| group.clips == clips) {
            group.parts.push(index);
        } else {
            groups.push(PartAnimations {
                parts: vec![index],
                clips,
            });
        }
    }
    groups
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visuals {
    #[serde(flatten, with = "crate::model_parts")]
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
) -> Result<(crate::battle_victory::document::Selection, Party)> {
    ensure!(!cancelled(), "party preparation cancelled");
    let path = party_path(snapshot, character, costume)?;
    let mut definition: PartyDefinition = snapshot.json(&path, JSON_LIMIT)?;
    crate::library::validate_path(&definition.victory.file)?;
    ensure!(
        !definition.victory.costume.is_empty(),
        "empty victory costume in {path}"
    );
    let mut parts = definition.take_model_parts()?;
    ensure!(!parts.is_empty(), "party has no model parts in {path}");
    for part in &mut parts {
        capture_scene(snapshot, &path, part, diagnostics, cancelled)?;
    }
    let body = Rig::from_glb(
        &snapshot.read(&parts[0].mesh, ASSET_LIMIT)?,
        &definition.attachment_nodes,
    )
    .with_context(|| path.clone())?;
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
        body,
        parts,
        wings,
        null_motions: definition.null_motions,
    };
    ensure!(!cancelled(), "party preparation cancelled");
    Ok((definition.victory, party))
}

fn capture_visuals(
    snapshot: &mut Snapshot,
    path: &str,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<crate::ScenePart>> {
    ensure!(!cancelled(), "party model preparation cancelled");
    let mut model: Visuals = snapshot.json(path, JSON_LIMIT)?;
    ensure!(!model.parts.is_empty(), "invalid party model in {path}");
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
    ensure!(!cancelled(), "battle model preparation cancelled");
    let mesh = snapshot
        .read(&part.mesh, ASSET_LIMIT)
        .with_context(|| format!("{path} mesh {}", part.mesh))?;
    part.prepare_bones(&mesh).with_context(|| path.to_owned())?;
    part.validate(|_| true).with_context(|| path.to_owned())?;
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
        let result = snapshot
            .read(&clip.motion, ASSET_LIMIT)
            .and_then(|bytes| clip.decode_motion(&bytes, &part.bone_names));
        if let Some(motion) = diagnostics.attempt(&clip.motion, result)? {
            clip.duration_seconds = motion.duration_frames / crate::animation::FRAME_HZ;
        }
    }
    ensure!(!cancelled(), "battle model preparation cancelled");
    Ok(())
}
