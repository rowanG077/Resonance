//! Shared and enemy effect models use the ordinary mesh and animation pipeline.
#[cfg(test)]
use super::super::effect_program::MagicArchive;
use super::super::effect_program::magic_member;
use super::all::PackageModel;
use crate::{
    model_preview::{Layer, layers, layers_with_clips},
    read::{u16 as half, u32 as word},
    scene::SourceClip,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::{
        effect_program::{BattleEffectPrograms, EffectCommand, ModelRef, Modifier},
        visual::{EffectModel, Rig},
    },
    model_preview::ModelPreview,
};
use std::path::Path;
#[cfg(test)]
use std::{borrow::Cow, collections::BTreeSet};

#[cfg(test)]
pub(super) fn preflight_magic(extracted: &Path, effects: &BattleEffectPrograms) -> Result<()> {
    let required = effects
        .models()
        .filter_map(|model| match model {
            ModelRef::Magic { package, index } => Some((package, index)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if required.is_empty() {
        return Ok(());
    }
    let archive = MagicArchive::read(extracted)?;
    let mut errors = Vec::new();
    for (package, index) in required {
        let result: Result<()> = (|| {
            ensure!(index < 10, "magic model index exceeds package");
            let bytes = archive.package(package)?;
            let binding = ModelRef::Magic { package, index };
            let (model, outline) =
                magic_layers(bytes, index, effects.externally_animated(binding))?;
            crate::model_preview::preflight(Layer {
                model,
                outline: outline.as_deref(),
                animation: None,
                attached_to: None,
                additive: false,
            })?;
            controlled_rig(
                model,
                outline.as_deref(),
                binding,
                &model_clips(bytes, index)?,
                effects,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            errors.push(format!("magic model {package}/{index}: {error:#}"));
        }
    }
    ensure!(
        errors.is_empty(),
        "effect model preflight failed:\n{}",
        errors.join("\n")
    );
    Ok(())
}

/// Authored resources remain cookable before any associated controller is supported.
pub(super) fn package_model(
    bytes: &[u8],
    binding: ModelRef,
    output: &Path,
) -> Result<PackageModel> {
    let (kind, package, index) = match binding {
        ModelRef::Magic { package, index } => ("magic", package, index),
        ModelRef::Skill { package, index } => ("skill", package, index),
        _ => anyhow::bail!("expected effect package model"),
    };
    ensure!(index < 10, "effect model index exceeds package");
    let resource = magic_member(bytes, 12 + usize::from(index) * 4)?
        .context("missing requested effect model")?;
    let outline = magic_member(bytes, 52 + usize::from(index) * 4)?;
    let clips = model_clips(bytes, index)?;
    let mut parts = Vec::new();
    layers_with_clips(
        Layer {
            model: resource,
            outline,
            animation: None,
            attached_to: None,
            additive: false,
        },
        &mut parts,
        &format!("battle/effects/models/{kind}/{package}/{index}/authored"),
        &clips,
        output,
    )
    .with_context(|| format!("effect model {binding:?}"))?;
    let model = ModelPreview {
        scale: 1.,
        elevation: 0.,
        parts,
        hidden_geometry: Vec::new(),
        node_scales: Vec::new(),
    };
    let model = PackageModel {
        binding,
        model,
        rig: authored_rig(resource, &clips)?,
        outline: outline.map(super::super::pose::skeleton).transpose()?,
    };
    model.validate()?;
    Ok(model)
}

#[cfg(test)]
type MagicLayers<'a> = (&'a [u8], Option<Cow<'a, [u8]>>);

#[cfg(test)]
fn magic_layers(bytes: &[u8], index: u8, controlled: bool) -> Result<MagicLayers<'_>> {
    let model = magic_member(bytes, 12 + usize::from(index) * 4)?
        .context("missing requested magic model")?;
    let outline = magic_member(bytes, 52 + usize::from(index) * 4)?
        .map(|outline| {
            if controlled {
                normalize_outline(model, outline)
            } else {
                Ok(Cow::Borrowed(outline))
            }
        })
        .transpose()?;
    Ok((model, outline))
}

#[cfg(test)]
fn normalize_outline<'a>(primary: &[u8], outline: &'a [u8]) -> Result<Cow<'a, [u8]>> {
    use super::super::pose;
    let primary = pose::skeleton(primary)?;
    let skeleton = pose::skeleton(outline)?;
    ensure!(
        primary.bones.len() == skeleton.bones.len()
            && primary
                .bones
                .iter()
                .zip(&skeleton.bones)
                .all(|(a, b)| a.parent == b.parent),
        "effect outline has a different ordinal hierarchy"
    );
    let bytes = pose::model(outline)?;
    let model = crate::model::Model::parse(bytes)?;
    let offset = pose::model_range(outline)?.start;
    let mut normalized = Cow::Borrowed(outline);
    for ((primary, outline), node) in primary.bones.iter().zip(&skeleton.bones).zip(&model.nodes) {
        if primary.bind == outline.bind {
            continue;
        }
        ensure!(
            node.data_offset != 0,
            "effect outline bind has no writable transform"
        );
        let start = offset + node.data_offset as usize;
        let data = normalized.to_mut();
        // Outline drawing copies primary global AND skin matrices by ordinal.
        // Cook those binds too, so mesh export derives the same inverse binds.
        // Preserve the outline's geometry, labels and non-transform flags/words.
        let flags = word(data, start)? | 0x0d00_0000;
        data[start..start + 4].copy_from_slice(&flags.to_be_bytes());
        for (index, value) in primary
            .bind
            .scale
            .into_iter()
            .chain(primary.bind.rotation)
            .chain(primary.bind.translation)
            .enumerate()
        {
            let at = start + (index + 1) * 4;
            data[at..at + 4].copy_from_slice(&value.to_bits().to_be_bytes());
        }
    }
    pose_joints(&primary, Some(&pose::skeleton(&normalized)?))?;
    Ok(normalized)
}

fn model_clips(bytes: &[u8], index: u8) -> Result<Vec<SourceClip<'_>>> {
    (0..4)
        .filter_map(|slot| {
            magic_member(bytes, 92 + usize::from(index) * 16 + slot * 4)
                .transpose()
                .map(|result| {
                    result.map(|bytes| SourceClip {
                        slot: slot as u16,
                        bytes,
                        resource: None,
                    })
                })
        })
        .collect()
}

fn authored_rig(resource: &[u8], clips: &[SourceClip<'_>]) -> Result<Rig> {
    let source = super::super::pose::RigSource::read(resource)?;
    let rig = Rig {
        skeleton: source.skeleton,
        motions: clips
            .iter()
            .map(|clip| {
                source
                    .bindings
                    .motion(clip.bytes)
                    .map(|motion| (clip.slot, motion))
            })
            .collect::<Result<_>>()?,
        attack_groups: Default::default(),
        effect_groups: Default::default(),
        weapon_bones: Default::default(),
    };
    rig.validate()?;
    Ok(rig)
}

pub(super) fn validate_controls(
    binding: ModelRef,
    rig: &Rig,
    effects: &BattleEffectPrograms,
) -> Result<()> {
    use resonance_content::battle::effects::EffectBank;
    let (bank, index) = match binding {
        ModelRef::Magic { package, index } => (EffectBank::Magic(package), index),
        ModelRef::Skill { package, index } => (EffectBank::Skill(package), index),
        _ => anyhow::bail!("expected animated effect package model"),
    };
    for program in effects.programs.iter().filter(|p| p.id.bank == bank) {
        for emission in &program.emissions {
            let modifiers = match &emission.command {
                EffectCommand::Particle { modifiers, .. }
                | EffectCommand::ModifyRetained { modifiers, .. } => modifiers,
                _ => continue,
            };
            for modifier in modifiers {
                if let Modifier::PlayModelAnimation { animation } = modifier
                    && animation.model == index
                {
                    let clip = rig
                        .motions
                        .get(&u16::from(animation.clip))
                        .with_context(|| {
                            format!("effect model {binding:?} lacks clip {}", animation.clip)
                        })?;
                    ensure!(
                        animation.rate.abs() <= clip.duration_frames,
                        "effect model playback skips more than a complete clip per tick"
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn controlled_rig(
    resource: &[u8],
    outline: Option<&[u8]>,
    binding: ModelRef,
    clips: &[SourceClip<'_>],
    effects: &BattleEffectPrograms,
) -> Result<(Option<Rig>, Vec<Vec<u16>>)> {
    if !effects.externally_animated(binding) {
        return Ok((None, Vec::new()));
    }
    let rig = authored_rig(resource, clips)?;
    validate_controls(binding, &rig, effects)?;
    let outline = outline.map(super::super::pose::skeleton).transpose()?;
    let pose_joints = pose_joints(&rig.skeleton, outline.as_ref())?;
    Ok((Some(rig), pose_joints))
}

#[cfg(test)]
fn pose_joints(
    primary: &resonance_content::battle::pose::Skeleton,
    outline: Option<&resonance_content::battle::pose::Skeleton>,
) -> Result<Vec<Vec<u16>>> {
    primary.validate()?;
    let joints: Vec<_> = (0..primary.bones.len() as u16).collect();
    let mut layers = vec![joints.clone()];
    if let Some(outline) = outline {
        outline.validate()?;
        // Outline draws copy the primary matrices by ordinal. Local poses are equivalent
        // only when both authored hierarchies and bind transforms agree.
        ensure!(
            primary.bones.len() == outline.bones.len()
                && primary
                    .bones
                    .iter()
                    .zip(&outline.bones)
                    .all(|(a, b)| a.parent == b.parent && a.bind == b.bind),
            "effect outline needs a global pose mapping for its different hierarchy or bind transforms"
        );
        layers.push(joints);
    }
    Ok(layers)
}

pub(super) fn enemy_model(bytes: &[u8], binding: ModelRef, output: &Path) -> Result<EffectModel> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy effect package");
    let metadata = usize::from(half(bytes, 4)?);
    let count = *bytes
        .get(metadata + 0x1e8)
        .context("missing enemy effect model count")?;
    let (monster, index, animation_model) = match binding {
        ModelRef::Enemy { monster, index } => (monster, index, index),
        ModelRef::EnemyAnimated {
            monster,
            index,
            animation_model,
        } => (monster, index, animation_model),
        _ => anyhow::bail!("expected enemy effect model binding"),
    };
    ensure!(
        count <= 6 && index < count && animation_model < count,
        "enemy effect model or animation exceeds source closure"
    );
    let resource = |at| -> Result<Option<&[u8]>> {
        let offset = word(bytes, at)? as usize;
        (offset != 0)
            .then(|| {
                bytes
                    .get(offset..)
                    .context("enemy effect resource exceeds package")
            })
            .transpose()
    };
    let animation = resource(0x1b0 + usize::from(animation_model) * 4)?;
    ensure!(
        !binding.retains_animation() || animation.is_some(),
        "missing inherited effect animation"
    );
    let suffix = if binding.retains_animation() && index != animation_model {
        format!("-animation-{animation_model}")
    } else {
        String::new()
    };
    let offset = usize::from(index) * 4;
    cook_model(
        binding,
        Layer {
            model: resource(0x180 + offset)?.context("missing declared enemy effect model")?,
            outline: resource(0x198 + offset)?,
            animation,
            attached_to: None,
            additive: false,
        },
        &format!("battle/effects/models/enemies/{monster}/{index}{suffix}"),
        output,
    )
}

pub(super) fn cook_model(
    binding: ModelRef,
    layer: Layer<'_>,
    name: &str,
    output: &Path,
) -> Result<EffectModel> {
    let mut parts = Vec::new();
    layers(layer, &mut parts, name, output).with_context(|| format!("effect model {binding:?}"))?;
    let model = ModelPreview {
        scale: 1.,
        elevation: 0.,
        parts,
        hidden_geometry: Vec::new(),
        node_scales: Vec::new(),
    };
    model.validate()?;
    Ok(EffectModel {
        binding,
        model,
        rig: None,
        pose_joints: Vec::new(),
    })
}

#[cfg(test)]
#[path = "effects/pose_tests.rs"]
mod pose_tests;

#[test]
#[ignore = "requires privately extracted original magic model"]
fn original_spread_model_keeps_sixteen_independent_draws_over_seven_meshes() {
    use sha2::{Digest, Sha256};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = magic_member(archive.package(9).unwrap(), 12)
        .unwrap()
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "aa48803e4944d98d2ef9a0df3132bdbf635ce8cab2b98ba8e522197c5618310d"
    );
    crate::geometry::preflight_section(source).unwrap();
    let crate::geometry::DecodedGeometry {
        manifest,
        gltf: scene,
        ..
    } = crate::geometry::decode_section(
        source,
        crate::geometry::DecodeMode::Runtime,
        |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(manifest.model_nodes.len(), 16);
    assert_eq!(manifest.objects.len(), 16);
    assert_eq!(scene["meshes"].as_array().unwrap().len(), 16);
    // Source node order:0,1,2,3,2,4,5,2, then object6 eight times. Equal priorities retain that order.
    let expected_nodes = [0, 1, 2, 4, 7, 3, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    let expected_objects = [0, 1, 2, 2, 2, 3, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6];
    let mut triangles = 0;
    for (index, draw) in manifest.objects.iter().enumerate() {
        let node = expected_nodes[index];
        assert_eq!(draw.source_index, expected_objects[index]);
        assert_eq!(draw.model_node, Some(node));
        assert_eq!(draw.draw_order, node as u32);
        let children = scene["nodes"][node]["children"].as_array().unwrap();
        assert_eq!(children.len(), 1);
        let mesh_node = children[0].as_u64().unwrap() as usize;
        assert_eq!(
            scene["nodes"][mesh_node]["mesh"].as_u64(),
            Some(index as u64)
        );
        let primitive = &scene["meshes"][index]["primitives"][0];
        let indices = primitive["indices"].as_u64().unwrap() as usize;
        triangles += scene["accessors"][indices]["count"].as_u64().unwrap() / 3;
        assert_eq!(draw.tev_modes, [1]);
        assert_eq!(draw.texture_commands, [0x11110000]);
    }
    assert_eq!(triangles, 512);
    // Repeated nodes share authored vertex/index buffers, while owning separate mesh/material slots.
    assert_eq!(
        manifest
            .objects
            .iter()
            .map(|o| o.position_accessor)
            .collect::<BTreeSet<_>>()
            .len(),
        7
    );
    for (left, right) in [(2, 3), (2, 4), (8, 15)] {
        assert_eq!(
            scene["meshes"][left]["primitives"][0]["indices"],
            scene["meshes"][right]["primitives"][0]["indices"]
        );
        assert_ne!(
            manifest.objects[left].model_node,
            manifest.objects[right].model_node
        );
    }
    // Independently pinned original instance transforms; three uses of object2 must not collapse.
    for (node, expected) in [
        (2, [-154.19748, 112.03102, 43.81299]),
        (4, [-40.479874, -0.0000097073225, -191.33418]),
        (7, [157.85867, -43.557117, 106.914986]),
    ] {
        for (actual, expected) in manifest.model_nodes[node]
            .translation
            .into_iter()
            .zip(expected)
        {
            assert!((actual - expected).abs() < 0.00001);
        }
    }
}
