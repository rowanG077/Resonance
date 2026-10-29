//! Enemy model references keep native component slots and explicit shared bodies.
use super::*;
use crate::model_preview::{ModelPreview, document as previews};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachment_nodes: BTreeMap<u8, String>,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_body: Option<Secondary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_variant: Option<crate::AtlasChannel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attached_models: Vec<AttachedModel>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachments: BTreeMap<u8, Model>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weapon_variants: BTreeMap<u8, Variant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trails: BTreeMap<u8, TrailDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secondary {
    pub model_slot: u8,
    pub motion_offset: u16,
    pub model: Model,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub model: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trail: Option<TrailDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<AttachmentAnimation>,
}

#[derive(Debug)]
pub struct Captured {
    pub preview: ModelPreview,
    pub source: Enemy,
}

pub fn capture(
    snapshot: &mut Snapshot,
    path: &str,
    definition: Definition,
    null_motions: &BTreeSet<u16>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Captured> {
    ensure!(!cancelled(), "enemy model preparation cancelled");
    let mut preview = previews::read(&definition.model, |resource| {
        snapshot.read(resource, JSON_LIMIT)
    })?;
    for part in &mut preview.parts {
        capture_scene(
            snapshot,
            &definition.model,
            &mut part.scene,
            diagnostics,
            cancelled,
        )?;
    }
    preview.validate()?;
    let primary = &preview.parts[0].scene;
    let body = Rig::from_glb(
        &snapshot.read(&primary.mesh, ASSET_LIMIT)?,
        &definition.attachment_nodes,
    )
    .with_context(|| path.to_owned())?;
    ensure!(
        primary
            .clips
            .iter()
            .all(|clip| !null_motions.contains(&clip.resource_slot)),
        "enemy motion is both empty and populated in {path}"
    );
    for binding in &definition.attached_models {
        binding.transform.validate()?;
        if let AttachmentPlacement::Bone(bone) = binding.placement {
            ensure!(
                usize::from(bone) < body.skeleton.bones.len(),
                "invalid attached-model bone in {path}"
            );
        }
    }
    let model = |mut document: Model, snapshot: &mut Snapshot| -> Result<ModelPart> {
        ensure!(!cancelled(), "enemy model preparation cancelled");
        for layer in &mut document.layers {
            capture_scene(snapshot, path, &mut layer.scene, diagnostics, cancelled)?;
            layer.selected_clip()?;
        }
        let model = document
            .assemble(|resource| snapshot.read(resource, ASSET_LIMIT))
            .with_context(|| path.to_owned())?;
        for layer in &model.layers {
            let skeleton = model
                .layer_skeletons
                .get(&layer.scene.resource)
                .unwrap_or(&model.rig.skeleton);
            ensure!(
                skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name)
                    .eq(layer.scene.bone_names.iter()),
                "enemy component rig differs from model in {path}"
            );
        }
        Ok(model)
    };
    let secondary_body = definition
        .secondary_body
        .map(|secondary| -> Result<_> {
            ensure!(
                secondary.motion_offset != 0,
                "empty secondary motion offset in {path}"
            );
            Ok(SecondaryBody {
                model_slot: secondary.model_slot,
                motion_offset: secondary.motion_offset,
                model: model(secondary.model, snapshot)?,
            })
        })
        .transpose()?;
    let attachments = definition
        .attachments
        .into_iter()
        .map(|(slot, document)| Ok((slot, model(document, snapshot)?)))
        .collect::<Result<_>>()?;
    let mut weapon_variants = BTreeMap::new();
    for (slot, variant) in definition.weapon_variants {
        let model = model(variant.model, snapshot)?;
        let trail = variant
            .trail
            .map(|definition| {
                diagnostics.attempt(
                    "battle weapon trail",
                    definition.prepare(&model.rig.skeleton, |path| snapshot.read(path, JSON_LIMIT)),
                )
            })
            .transpose()?
            .flatten();
        weapon_variants.insert(
            slot,
            WeaponVariant {
                model,
                trail,
                animation: variant.animation,
            },
        );
    }
    let trails = trails::prepare_trails(definition.trails, &attachments, diagnostics, |path| {
        snapshot.read(path, JSON_LIMIT)
    })?;
    ensure!(!cancelled(), "enemy model preparation cancelled");
    Ok(Captured {
        preview,
        source: Enemy {
            body,
            secondary_body,
            body_variant: definition.body_variant,
            attached_models: definition.attached_models,
            attachments,
            uv_animation: definition.uv_animation,
            weapon_variants,
            trails,
        },
    })
}
