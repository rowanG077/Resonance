//! Controllers bind authored effect layers without decoding the original packages again.
use super::{Archive, Asset, Directory, Sources, Visual};
use crate::battle::visual::{all::PackageModel, effects::validate_controls};
use crate::scene::glb::Glb;
use anyhow::{Context, Result, ensure};
use resonance_content::battle::{
    effect_program::{BattleEffectPrograms, ModelRef},
    pose::Skeleton,
    visual::EffectModel,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

pub(in crate::battle::visual) fn effect_packages(
    root: &Path,
    disc: u8,
    sources: &Sources,
    effects: &BattleEffectPrograms,
) -> Result<Vec<EffectModel>> {
    models(root, disc, sources, effects.models(), effects)
}

fn models(
    root: &Path,
    disc: u8,
    sources: &Sources,
    bindings: impl IntoIterator<Item = ModelRef>,
    effects: &BattleEffectPrograms,
) -> Result<Vec<EffectModel>> {
    let mut directories = BTreeMap::new();
    bindings
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|binding| *binding != ModelRef::ColetteWeapon)
        .map(|binding| {
            let (source, name) = match binding {
                ModelRef::Magic { package, index } => (
                    sources.archive(Archive::Magic),
                    format!("magic-{package}-model-{index}"),
                ),
                ModelRef::Skill { package, index } => (
                    sources.archive(Archive::Skill),
                    format!("skill-{package}-model-{index}"),
                ),
                ModelRef::Common { index } => {
                    (sources.usual.as_str(), format!("common-model-{index}"))
                }
                ModelRef::Enemy { monster, index } => (
                    sources.enemy.as_str(),
                    format!("enemy-{monster}-model-{index}"),
                ),
                ModelRef::EnemyAnimated {
                    monster,
                    index,
                    animation_model,
                } => (
                    sources.enemy.as_str(),
                    format!("enemy-{monster}-model-{index}-animation-{animation_model}"),
                ),
                ModelRef::ColetteWeapon => unreachable!(),
            };
            (|| {
                if !directories.contains_key(source) {
                    directories.insert(source, Directory::open(root, disc, source)?);
                }
                let visual = directories[source].read(Asset::EffectModel(binding), &name)?;
                let model = match (binding, visual) {
                    (
                        ModelRef::Magic { .. } | ModelRef::Skill { .. },
                        Visual::PackageModel(model),
                    ) if model.binding == binding => bind(root, model, effects)?,
                    (
                        ModelRef::Common { .. }
                        | ModelRef::Enemy { .. }
                        | ModelRef::EnemyAnimated { .. },
                        Visual::EffectModel(model),
                    ) if model.binding == binding => model,
                    _ => anyhow::bail!("wrong cooked effect model kind or binding; rerun cook-all"),
                };
                model.validate_pose_joints()?;
                Ok(model)
            })()
            .with_context(|| format!("binding effect model {binding:?}"))
        })
        .collect()
}

fn bind(
    root: &Path,
    mut authored: PackageModel,
    effects: &BattleEffectPrograms,
) -> Result<EffectModel> {
    let controlled = effects.externally_animated(authored.binding);
    let mut pose_joints = Vec::new();
    if controlled {
        validate_controls(authored.binding, &authored.rig, effects)?;
        let primary = &authored.rig.skeleton;
        let joints: Vec<_> = (0..u16::try_from(primary.bones.len())?).collect();
        pose_joints.push(joints.clone());
        if let Some(outline) = &authored.outline {
            let source = Glb::read(&root.join(&authored.model.parts[0].scene.mesh))?;
            let destination = Glb::read(&root.join(&authored.model.parts[1].scene.mesh))?;
            if let Some(bytes) = controlled_outline(source, destination, primary, outline)? {
                let path = format!("battle/effects/controlled/{}.glb", crate::digest(&bytes));
                fs::create_dir_all(root.join("battle/effects/controlled"))?;
                fs::write(root.join(&path), bytes)?;
                authored.model.parts[1].scene.mesh = path;
            }
            pose_joints.push(joints);
        }
    }
    let model = EffectModel {
        binding: authored.binding,
        model: authored.model,
        rig: controlled.then_some(authored.rig),
        pose_joints,
    };
    model.validate_pose_joints()?;
    Ok(model)
}

/// Outline draws copy the primary global and skin matrices by ordinal. Matching local
/// poses therefore require both the primary TRS and its inverse-bind matrices.
fn controlled_outline(
    primary: Glb,
    mut outline: Glb,
    primary_rig: &Skeleton,
    outline_rig: &Skeleton,
) -> Result<Option<Vec<u8>>> {
    ensure!(
        primary_rig.bones.len() == outline_rig.bones.len()
            && primary_rig
                .bones
                .iter()
                .zip(&outline_rig.bones)
                .all(|(a, b)| a.parent == b.parent),
        "effect outline has a different ordinal hierarchy"
    );
    primary.validate_skeleton(primary_rig)?;
    outline.validate_skeleton(outline_rig)?;
    let mut changed = false;
    for index in 0..primary_rig.bones.len() {
        for field in ["translation", "rotation", "scale"] {
            let value = &primary.json["nodes"][index][field];
            if outline.json["nodes"][index][field] != *value {
                outline.json["nodes"][index][field] = value.clone();
                changed = true;
            }
        }
    }
    if let Some(destination) = outline.inverse_binds(outline_rig.bones.len())? {
        let source = primary
            .inverse_binds(primary_rig.bones.len())?
            .context("skinned outline lacks primary inverse binds")?;
        if outline.binary[destination.clone()] != primary.binary[source.clone()] {
            outline.binary[destination].copy_from_slice(&primary.binary[source]);
            changed = true;
        }
    }
    changed
        .then(|| crate::scene::pack_glb(&outline.json, &mut outline.binary))
        .transpose()
}

impl Glb {
    fn validate_skeleton(&self, skeleton: &Skeleton) -> Result<()> {
        skeleton.validate()?;
        let count = skeleton.bones.len();
        let nodes = self.json["nodes"]
            .as_array()
            .context("missing cooked GLB nodes")?;
        ensure!(nodes.len() >= count, "cooked GLB has incomplete joints");
        let mut parents = vec![None; count];
        for (index, bone) in skeleton.bones.iter().enumerate() {
            let node = &nodes[index];
            ensure!(
                node["name"].as_str() == Some(bone.name.as_str())
                    && node["matrix"].is_null()
                    && serde_json::from_value::<[f32; 3]>(node["translation"].clone())?
                        == bone.bind.translation
                    && serde_json::from_value::<[f32; 4]>(node["rotation"].clone())?
                        == bone.bind.rotation
                    && serde_json::from_value::<[f32; 3]>(node["scale"].clone())?
                        == bone.bind.scale,
                "cooked GLB joint {index} differs from its authored skeleton"
            );
            for child in node["children"]
                .as_array()
                .context("missing cooked joint children")?
            {
                let child = integer(child)?;
                ensure!(child < nodes.len(), "cooked GLB child exceeds nodes");
                if child < count {
                    ensure!(
                        parents[child].replace(index as u16).is_none(),
                        "cooked GLB joint has multiple parents"
                    );
                }
            }
        }
        ensure!(
            parents
                .iter()
                .copied()
                .eq(skeleton.bones.iter().map(|bone| bone.parent)),
            "cooked GLB joint hierarchy differs from its skeleton"
        );
        Ok(())
    }

    fn inverse_binds(&self, count: usize) -> Result<Option<Range<usize>>> {
        let skins = self.json["skins"]
            .as_array()
            .context("missing cooked GLB skins")?;
        if skins.is_empty() {
            return Ok(None);
        }
        ensure!(skins.len() == 1, "expected one cooked GLB skin");
        let skin = &skins[0];
        let joints = skin["joints"].as_array().context("missing skin joints")?;
        ensure!(
            joints.len() == count
                && joints
                    .iter()
                    .enumerate()
                    .all(|(index, joint)| joint.as_u64() == Some(index as u64)),
            "cooked skin joints are not ordinal"
        );
        let accessor = &self.json["accessors"][integer(&skin["inverseBindMatrices"])?];
        ensure!(
            accessor["type"] == "MAT4"
                && accessor["componentType"] == 5126
                && integer(&accessor["count"])? == count
                && accessor["sparse"].is_null()
                && accessor["normalized"].as_bool().is_none_or(|value| !value),
            "invalid cooked inverse-bind accessor"
        );
        let view = &self.json["bufferViews"][integer(&accessor["bufferView"])?];
        ensure!(
            view["buffer"] == 0 && view["byteStride"].is_null(),
            "invalid cooked inverse-bind buffer view"
        );
        let offset = optional_offset(&accessor["byteOffset"])?;
        let start = optional_offset(&view["byteOffset"])? + offset;
        let end = start + count * 64;
        ensure!(
            offset + count * 64 <= integer(&view["byteLength"])? && end <= self.binary.len(),
            "cooked inverse binds exceed buffer"
        );
        Ok(Some(start..end))
    }
}

fn integer(value: &Value) -> Result<usize> {
    usize::try_from(value.as_u64().context("expected a cooked GLB integer")?).map_err(Into::into)
}

fn optional_offset(value: &Value) -> Result<usize> {
    if value.is_null() {
        Ok(0)
    } else {
        integer(value)
    }
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "enemy_effect_tests.rs"]
mod enemy_tests;
