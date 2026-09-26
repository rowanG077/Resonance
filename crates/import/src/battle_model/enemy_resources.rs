//! Ordinary enemy banks registered by 44E1C and model templates from 44388.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::Art,
    battle_model::{ModelPart, enemy_effects_path, enemy_projectiles_path},
    field_preload::{File, Role},
};
use std::{collections::BTreeMap, path::Path};

pub(super) fn publish(bytes: &[u8], id: u8, output: &Path) -> Result<BTreeMap<String, File>> {
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)
        .with_context(|| format!("enemy {id} resource pointer table"))?;
    let mut files = BTreeMap::new();
    if let Some(projectiles) = members
        .model(0x1c8)
        .with_context(|| format!("enemy {id} projectile member 0x1c8"))?
    {
        let start = crate::read::u32(bytes, 0x1c8)? as usize;
        let table = crate::battle_projectile::read_package_member(projectiles, start)
            .with_context(|| format!("enemy {id} projectile member 0x1c8 at {start:#x}"))?;
        write(&mut files, output, enemy_projectiles_path(id), &table)?;
    }
    if let Some(bank) = members
        .model(0x1cc)
        .with_context(|| format!("enemy {id} effect member 0x1cc"))?
    {
        let mut source = crate::battle_effect::read(bank)
            .with_context(|| format!("enemy {id} effect member 0x1cc"))?;
        let mut textures = BTreeMap::new();
        // The package owns relative bank 2 and its separate slot 14. Runtime
        // preparation binds these to the selected enemy resource generation.
        for (field, slot) in [(0x1d0, 2), (0x1d4, 14)] {
            if let Some(bytes) = members
                .model(field)
                .with_context(|| format!("enemy {id} texture member {field:#x}, slot {slot}"))?
            {
                textures.insert(
                    slot,
                    crate::battle_effect::art::textures_from_source(bytes, output).with_context(
                        || format!("enemy {id} texture member {field:#x}, slot {slot}"),
                    )?,
                );
            }
        }
        let profile = usize::from(crate::read::u16(bytes, 4)?);
        let count = usize::from(
            *bytes
                .get(profile + 0x1e8)
                .context("missing enemy model count")?,
        );
        ensure!(count <= 6, "enemy effect model count exceeds pointer table");
        let mut models = BTreeMap::new();
        let mut decoded = crate::scene::decoded::Package::default();
        for slot in 0..count {
            let Some(model) = members.model(0x180 + slot * 4)? else {
                break;
            };
            let animation = members.animation(0x1b0 + slot * 4, &mut decoded)?;
            let clips = animation
                .as_ref()
                .map(|animation| crate::character::Clip {
                    slot: 0,
                    resource: None,
                    animation,
                })
                .into_iter()
                .collect::<Vec<_>>();
            let mut layers = Vec::new();
            crate::model_preview::layers_with_clips(
                crate::model_preview::Layer {
                    model,
                    outline: members.model(0x198 + slot * 4)?,
                    animation: None,
                    attached_to: None,
                    additive: false,
                },
                &mut layers,
                &format!("battle/enemies/{id:03}/effect-models/{slot}"),
                &clips,
                output,
                &mut decoded,
            )?;
            models.insert(
                slot as u8,
                ModelPart {
                    rig: super::rig(model, super::RigKind::Effect)?,
                    layers,
                },
            );
        }
        let mut art_files = super::files(
            models
                .values()
                .flat_map(|part| part.layers.iter().map(|layer| &layer.scene)),
            output,
        )?;
        for image in textures
            .values()
            .flatten()
            .flat_map(|texture| &texture.images)
        {
            art_files.insert(
                image.path.clone(),
                File {
                    sha256: crate::media::hash_file(&output.join(&image.path))?,
                    bytes: std::fs::metadata(output.join(&image.path))?.len(),
                    roles: [Role::Texture].into(),
                },
            );
        }
        files.extend(art_files.clone());
        source.art = Some(Art {
            source_sha256: crate::digest(bytes),
            textures,
            models,
            files: art_files,
        });
        write(&mut files, output, enemy_effects_path(id), &source)?;
    }
    Ok(files)
}

fn write(
    files: &mut BTreeMap<String, File>,
    output: &Path,
    path: String,
    value: &impl serde::Serialize,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    crate::write_atomic(&output.join(&path), &bytes)?;
    files.insert(
        path,
        File {
            sha256: crate::digest(&bytes),
            bytes: bytes.len() as u64,
            roles: [Role::Data].into(),
        },
    );
    Ok(())
}

/// Reuse the published carried geometry. Rig metadata is read from the same
/// member selected by 44170; the battle loader owns attachment and visibility.
pub(super) fn attachments(
    bytes: &[u8],
    preview: &resonance_content::model_preview::ModelPreview,
) -> Result<(
    BTreeMap<u8, ModelPart>,
    BTreeMap<u8, resonance_content::battle_model::TrailMaterial>,
)> {
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let profile = usize::from(crate::read::u16(bytes, 4)?);
    let count = usize::from(
        *bytes
            .get(profile + 0x1e4)
            .context("missing enemy attachment count")?,
    );
    ensure!(count <= 8, "enemy attachment count exceeds pointer table");
    let mut parts = BTreeMap::new();
    let mut trails = BTreeMap::new();
    for slot in 0..count {
        let package = members
            .model(0x160 + slot * 4)?
            .context("missing enemy attachment")?;
        let sections = crate::field::sections(package)?;
        let Some(range) = sections.get(1).and_then(Option::as_ref) else {
            continue;
        };
        let metadata = sections
            .first()
            .and_then(Option::as_ref)
            .context("missing enemy weapon metadata")?;
        trails.insert(
            slot as u8,
            super::weapon::trail_material(&package[metadata.clone()])?,
        );
        let prefix = format!("kk0{slot}");
        let layers: Vec<_> = preview
            .parts
            .iter()
            .filter(|part| {
                part.attached_to
                    .as_ref()
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .cloned()
            .collect();
        ensure!(!layers.is_empty(), "missing published enemy attachment");
        parts.insert(
            slot as u8,
            ModelPart {
                rig: super::rig(&package[range.clone()], super::RigKind::Weapon)?,
                layers,
            },
        );
    }
    Ok((parts, trails))
}
