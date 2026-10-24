//! Enemy model references keep native component slots and explicit shared bodies.
use super::*;
use crate::model_preview::{ModelPreview, document as previews};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub rig: String,
    pub model: String,
    pub secondary_body: Option<Secondary>,
    pub body_variant: Option<crate::battle_profile::TextureChannel>,
    pub attached_models: Vec<AttachedModel>,
    pub attachments: BTreeMap<u8, String>,
    pub uv_animation: BTreeMap<u8, AttachmentAnimation>,
    pub weapon_variants: BTreeMap<u8, Variant>,
    pub trails: BTreeMap<u8, TrailMaterial>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secondary {
    pub model_slot: u8,
    pub motion_offset: u16,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub model: String,
    pub trail: TrailMaterial,
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
    null_motions: &BTreeSet<u16>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Captured> {
    ensure!(!cancelled(), "enemy model preparation cancelled");
    let definition: Definition = snapshot.json(path, JSON_LIMIT)?;
    ensure!(
        definition.version == VERSION,
        "unsupported enemy model in {path}"
    );
    let body: Rig = snapshot.json(&definition.rig, JSON_LIMIT)?;
    validate_rig(&body)?;
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
    ensure!(
        body.skeleton
            .bones
            .iter()
            .map(|bone| &bone.name)
            .eq(primary.bone_names.iter()),
        "enemy rig differs from model in {path}"
    );
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
    let mut model = |path: &str| -> Result<ModelPart> {
        ensure!(!cancelled(), "enemy model preparation cancelled");
        let mut document: Model = snapshot.json(path, JSON_LIMIT)?;
        ensure!(
            document.version == VERSION && !document.model.layers.is_empty(),
            "invalid enemy component in {path}"
        );
        validate_rig(&document.model.rig)?;
        for layer in &mut document.model.layers {
            capture_scene(snapshot, path, &mut layer.scene, diagnostics, cancelled)?;
            layer.selected_clip()?;
            let skeleton = document
                .model
                .layer_skeletons
                .get(&layer.scene.resource)
                .unwrap_or(&document.model.rig.skeleton);
            skeleton.validate()?;
            ensure!(
                skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name)
                    .eq(layer.scene.bone_names.iter()),
                "enemy component rig differs from model in {path}"
            );
        }
        Ok(document.model)
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
                model: model(&secondary.model)?,
            })
        })
        .transpose()?;
    let attachments = definition
        .attachments
        .into_iter()
        .map(|(slot, path)| Ok((slot, model(&path)?)))
        .collect::<Result<_>>()?;
    let weapon_variants = definition
        .weapon_variants
        .into_iter()
        .map(|(slot, variant)| {
            Ok((
                slot,
                WeaponVariant {
                    model: model(&variant.model)?,
                    trail: variant.trail,
                    animation: variant.animation,
                },
            ))
        })
        .collect::<Result<_>>()?;
    ensure!(!cancelled(), "enemy model preparation cancelled");
    Ok(Captured {
        preview,
        source: Enemy {
            source_sha256: String::new(),

            body,
            secondary_body,
            body_variant: definition.body_variant,
            attached_models: definition.attached_models,
            attachments,
            uv_animation: definition.uv_animation,
            weapon_variants,
            trails: definition.trails,
        },
    })
}

fn validate_rig(rig: &Rig) -> Result<()> {
    rig.skeleton.validate()?;
    ensure!(
        rig.attachments
            .values()
            .all(|&bone| usize::from(bone) < rig.skeleton.bones.len()),
        "invalid enemy attachment bone"
    );
    Ok(())
}
