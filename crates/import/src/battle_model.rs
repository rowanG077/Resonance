//! Battle skeletons and carried-model attachment slots.
mod enemy_resources;
pub mod party;
pub mod weapon;
use crate::{geometry, model::Model};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::{Bone, Skeleton, Transform, TransformChannels},
    battle_model::{Enemy, Rig, enemy_path},
};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RigKind {
    Body,
    Weapon,
    Effect,
}

pub(crate) fn rig(resource: &[u8], kind: RigKind) -> Result<Rig> {
    let (_, resource) = geometry::model_resource(resource)?;
    let model = Model::parse(&resource[geometry::skeleton_range(resource)?])?;
    ensure!(
        model.nodes.iter().all(|node| node.transform_kind == 1),
        "unsupported battle model bone transform"
    );
    let names = model
        .names
        .as_ref()
        .context("battle model has no bone names")?;
    ensure!(
        names.iter().all(|name| !name.is_empty()),
        "battle model has unnamed bones"
    );
    let skeleton = Skeleton {
        bones: geometry::model_node_info(&model)
            .into_iter()
            .zip(&model.nodes)
            .map(|(node, source)| Bone {
                name: node.name,
                parent: source.parent.map(|parent| parent as u16),
                bind_channels: TransformChannels(
                    source
                        .data_words
                        .first()
                        .map_or(0, |word| (word >> 24) as u8),
                ),
                bind: Transform {
                    translation: node.translation,
                    rotation: node.rotation,
                    scale: node.scale,
                },
            })
            .collect(),
    };
    skeleton.validate()?;
    let mut result = Rig {
        skeleton,
        attachments: Default::default(),
    };
    if kind == RigKind::Body {
        for (index, name) in names.iter().enumerate() {
            if name
                .as_bytes()
                .get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"kk"))
            {
                let slot = name
                    .as_bytes()
                    .get(3)
                    .copied()
                    .filter(u8::is_ascii_digit)
                    .context("invalid carried-model attachment slot")?
                    - b'0';
                result.attachments.insert(slot, u16::try_from(index)?);
            }
        }
    }
    Ok(result)
}

pub(crate) fn publish_enemy(
    bytes: &[u8],
    voices: &crate::battle_voice::Source,
    default_guard_bonus: u8,
    id: u8,
    preview: &resonance_content::model_preview::ModelPreview,
    library: &Path,
    output: &Path,
) -> Result<[String; 2]> {
    let definition = crate::battle_enemy::read(bytes, id, voices, default_guard_bonus)?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let mut source = Enemy {
        source_sha256: definition.source_sha256.clone(),
        body_variant: enemy_body_variant(bytes)?,
        attachments: Default::default(),
        weapon_variants: Default::default(),
        trails: Default::default(),
        body: rig(
            members.model(0x18)?.context("missing enemy body")?,
            RigKind::Body,
        )?,
        files: Default::default(),
    };
    source.files = files(preview.parts.iter().map(|part| &part.scene), library)?;
    source.files.extend(enemy_resources::publish(
        bytes,
        id,
        &source.body.skeleton,
        output,
    )?);
    (source.attachments, source.trails) = enemy_resources::attachments(bytes, preview)?;
    publish_enemy_carried_motions(bytes, &definition.actions, &mut source, output)?;
    publish_enemy_weapon_variants(bytes, &mut source, output)?;
    let path = enemy_path(id);
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&source)?)?;
    let definition_path = resonance_content::battle_enemy::path(id);
    crate::write_atomic(
        &output.join(&definition_path),
        &serde_json::to_vec(&definition)?,
    )?;
    Ok([definition_path, path])
}

/// Refresh carried action clips without converting their geometry again.
pub fn publish_enemy_carried_motions(
    bytes: &[u8],
    actions: &resonance_content::battle_action::EnemyActions,
    source: &mut Enemy,
    output: &Path,
) -> Result<Vec<String>> {
    ensure!(
        source.source_sha256 == crate::digest(bytes),
        "enemy carried motion source differs"
    );
    enemy_resources::carried_motions(bytes, actions, source, output)
}

/// Persistent appearance rows are separate from animated expression channels.
pub fn enemy_body_variant(
    bytes: &[u8],
) -> Result<Option<resonance_content::battle_profile::TextureChannel>> {
    let metadata = usize::from(crate::read::u16(bytes, 4)?);
    let channel = bytes
        .get(metadata + 0xee..metadata + 0xf0)
        .context("truncated enemy body atlas channel")?;
    Ok(
        (channel[1] != 0).then_some(resonance_content::battle_profile::TextureChannel {
            texture: channel[0],
            frames: channel[1],
        }),
    )
}

/// Publish optional carried artwork without rebuilding an enemy's body or motions.
pub fn publish_enemy_weapon_variants(
    bytes: &[u8],
    source: &mut Enemy,
    output: &Path,
) -> Result<()> {
    ensure!(
        source.source_sha256 == crate::digest(bytes),
        "enemy weapon source differs"
    );
    source.weapon_variants = enemy_resources::weapon_variants(bytes, output)?;
    source.files.extend(files(
        source
            .weapon_variants
            .values()
            .flat_map(|variant| variant.model.layers.iter().map(|layer| &layer.scene)),
        output,
    )?);
    Ok(())
}

pub(crate) fn files<'a>(
    parts: impl IntoIterator<Item = &'a resonance_content::ScenePart>,
    library: &Path,
) -> Result<std::collections::BTreeMap<String, resonance_content::field_preload::File>> {
    use resonance_content::field_preload::{File, Role};
    let mut files = std::collections::BTreeMap::<String, File>::new();
    for part in parts {
        for (path, role) in std::iter::once((&part.mesh, Role::Mesh))
            .chain(part.textures.iter().map(|path| (path, Role::Texture)))
            .chain(part.clips.iter().map(|clip| (&clip.motion, Role::Data)))
        {
            resonance_content::validate_asset_path(path)?;
            if let Some(file) = files.get_mut(path) {
                file.roles.insert(role);
            } else {
                let asset = library.join(path);
                files.insert(
                    path.clone(),
                    File {
                        sha256: crate::media::hash_file(&asset)?,
                        bytes: std::fs::metadata(asset)?.len(),
                        roles: [role].into(),
                    },
                );
            }
        }
    }
    Ok(files)
}

/// Selected development cooking uses the same publisher as Monster Book preparation.
pub fn publish_enemies(
    extracted: &Path,
    monsters: &[resonance_content::monster::Monster],
    library: &Path,
    output: &Path,
) -> Result<Vec<String>> {
    ensure!(
        monsters.len() == resonance_content::monster::MONSTER_COUNT,
        "incomplete enemy metadata"
    );
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
    let archive = extracted.join("files").join(sources.enemy);
    let voices = crate::battle_voice::read(&usual)?;
    let guard_bonus = crate::battle_recoil::guard_defaults(&crate::rel::Rel::read(
        &extracted.join("files").join(&sources.module),
    )?)?[0];
    monsters
        .iter()
        .enumerate()
        .map(|(id, monster)| {
            publish_enemy(
                &crate::source_assets::enemy_package(&archive, &usual, id as u16)?,
                &voices,
                guard_bonus,
                id as u8,
                &monster.preview,
                library,
                output,
            )
        })
        .collect::<Result<Vec<_>>>()
        .map(|paths| paths.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests;
