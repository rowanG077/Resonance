//! Stored spell packages selected by 3EB74 and bound by 3E580.
use crate::{
    battle_model::{RigKind, files, rig},
    character::Clip,
    model_preview::{Layer, PointerMembers, layers_with_clips},
    rel::Rel,
    scene::decoded::Package,
    source_assets::{Sources, read_range},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_model::ModelPart,
    battle_scene::{Scene, Texture},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, path::Path};

/// Publish the selected scene through the same path used by full cooking.
pub fn publish(extracted: &Path, technique: u16, output: &Path) -> Result<String> {
    publish_source(
        extracted,
        &Sources::read(extracted)?,
        technique,
        output,
        "battle",
    )
}

pub(crate) fn publish_source(
    extracted: &Path,
    sources: &Sources,
    technique: u16,
    output: &Path,
    prefix: &str,
) -> Result<String> {
    let module = Rel::read(&extracted.join("files").join(&sources.module))?;
    let table = module
        .at((5, 0xfe0))?
        .get(..0x1e4)
        .context("truncated spell directory")?;
    let index = usize::from(technique.checked_sub(200).context("invalid stored spell")?);
    let start = crate::read::u32(table, index * 4)?;
    ensure!(start != 0, "spell {technique} has no stored package");
    // 3EE38..3EE54 scans following nonzero entries; it does not use a CAB directory.
    let end = table[index * 4 + 4..]
        .chunks_exact(4)
        .map(|row| crate::read::u32(row, 0))
        .find(|value| !matches!(value, Ok(0)))
        .context("stored spell has no range end")??;
    let bytes = read_range(
        &extracted.join("files").join(&sources.magic),
        start as usize..end as usize,
    )?;
    let path = format!("{prefix}/scenes/{technique:03}.json");
    let scene = read(
        &bytes,
        &format!("{prefix}/scenes/{technique:03}.effects.json"),
        output,
    )?;
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&scene)?)?;
    Ok(path)
}

fn read(bytes: &[u8], effects_path: &str, output: &Path) -> Result<Scene> {
    let members = PointerMembers::new(bytes, 4..0x114)?;
    // These optional members have no consumer in the current stored scene.
    for field in [0xfc, 0x104, 0x108, 0x10c, 0x110] {
        ensure!(
            members.model(field)?.is_none(),
            "unsupported stored scene member {field:#x}"
        );
    }
    let effects = crate::battle_effect::read(members.model(4)?.context("missing scene effects")?)?;
    let effect_bytes = serde_json::to_vec(&effects)?;
    crate::write_atomic(&output.join(effects_path), &effect_bytes)?;
    let textures = members
        .model(8)?
        .map(|bytes| crate::texture::decode_source(bytes)?.write(output))
        .transpose()?
        .map(|catalogue| catalogue.textures)
        .unwrap_or_default()
        .into_iter()
        .map(|texture| {
            let texture = texture.context("invalid scene texture")?;
            Ok(Texture {
                images: (0..texture.images.len())
                    .map(|i| texture.image(i))
                    .collect::<Result<_>>()?,
                sampler: texture.sampler,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut decoded = Package::default();
    let mut models = BTreeMap::new();
    for slot in 0..10 {
        let model = members.model(12 + slot * 4)?;
        let outline = members.model(52 + slot * 4)?;
        let motions = (0..4)
            .map(|motion| members.animation(92 + slot * 16 + motion * 4, &mut decoded))
            .collect::<Result<Vec<_>>>()?;
        let Some(model) = model else {
            ensure!(
                outline.is_none() && motions.iter().all(Option::is_none),
                "scene animation or outline has no model"
            );
            continue;
        };
        let clips = motions
            .iter()
            .enumerate()
            .filter_map(|(slot, motion)| {
                motion.as_ref().map(|animation| Clip {
                    slot: slot as u16,
                    resource: None,
                    animation,
                })
            })
            .collect::<Vec<_>>();
        let mut layers = Vec::new();
        layers_with_clips(
            Layer {
                model,
                outline,
                animation: None,
                attached_to: None,
                additive: false,
            },
            &mut layers,
            &format!("battle/scene/{slot}"),
            &clips,
            output,
            &mut decoded,
        )?;
        models.insert(
            slot as u8,
            ModelPart {
                rig: rig(model, RigKind::Effect)?,
                layers,
            },
        );
    }
    let mut files = files(
        models
            .values()
            .flat_map(|model| model.layers.iter().map(|layer| &layer.scene)),
        output,
    )?;
    files.insert(
        effects_path.into(),
        File {
            sha256: crate::digest(&effect_bytes),
            bytes: effect_bytes.len() as u64,
            roles: [Role::Data].into(),
        },
    );
    for image in textures.iter().flat_map(|texture| &texture.images) {
        files.insert(
            image.path.clone(),
            File {
                sha256: crate::media::hash_file(&output.join(&image.path))?,
                bytes: std::fs::metadata(output.join(&image.path))?.len(),
                roles: [Role::Texture].into(),
            },
        );
    }
    Ok(Scene {
        source_sha256: crate::digest(bytes),
        effects: effects_path.into(),
        textures,
        models,
        action: members
            .model(0x100)?
            .map(crate::battle_action::bundle)
            .transpose()?,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted original discs; publishes only Nurse resources"]
    fn original_nurse_package_preserves_instances_sparse_clips_and_effect_sources() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let mut previous = None;
        for disc in [1, 2] {
            let output = tempfile::tempdir()?;
            let path = publish(&local.join(format!("disc{disc}")), 237, output.path())?;
            let bytes = std::fs::read(output.path().join(path))?;
            let scene: Scene = serde_json::from_slice(&bytes)?;
            assert_eq!(
                scene.models.keys().copied().collect::<Vec<_>>(),
                [0, 1, 2, 3]
            );
            let effects: resonance_content::battle_effect::SourceBank =
                serde_json::from_slice(&std::fs::read(output.path().join(&scene.effects))?)?;
            assert_eq!(effects.programs.len(), 7);
            assert_eq!(scene.textures.len(), 2);
            assert_eq!(
                scene
                    .action
                    .as_ref()
                    .context("missing scene action")?
                    .phases
                    .len(),
                4
            );
            let mut motions = std::collections::BTreeSet::new();
            for model in scene.models.values() {
                assert_eq!(model.layers.len(), 2);
                assert!(model.rig.volumes.is_empty() && model.rig.attack_groups.is_empty());
                assert!(model.rig.transform_kinds.iter().all(|&kind| kind == 1));
                for layer in &model.layers {
                    assert_eq!(layer.scene.clips.len(), 1);
                    let clip = &layer.scene.clips[0];
                    assert_eq!(clip.resource_slot, 0);
                    let motion = resonance_content::animation::Motion::decode(&std::fs::read(
                        output.path().join(&clip.motion),
                    )?)?;
                    motion.validate(&model.rig.skeleton)?;
                }
                motions.insert(&model.layers[0].scene.clips[0].motion);
                assert_eq!(
                    model.layers[0].scene.mesh,
                    scene.models[&0].layers[0].scene.mesh
                );
            }
            // Four distinct source ranges contain the same clip. Playback is
            // independent, while content-addressed sparse curves remain shared.
            assert_eq!(motions.len(), 1);
            for (path, file) in &scene.files {
                assert_eq!(
                    file.sha256,
                    crate::media::hash_file(&output.path().join(path))?
                );
            }
            if let Some(previous) = &previous {
                assert_eq!(&bytes, previous);
            }
            previous = Some(bytes);
        }
        Ok(())
    }
}
