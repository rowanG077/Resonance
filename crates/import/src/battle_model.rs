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

#[expect(
    clippy::too_many_arguments,
    reason = "Keep source identity, prepared resources and publication paths explicit at this boundary."
)]
pub(crate) fn publish_enemy(
    bytes: &[u8],
    usual: &[u8],
    voices: &crate::battle_voice::Source,
    default_guard_bonus: u8,
    id: u8,
    preview: &resonance_content::model_preview::ModelPreview,
    shared: &mut crate::battle_effect::SharedPalettes,
    library: &Path,
    output: &Path,
) -> Result<[String; 2]> {
    let definition = crate::battle_enemy::read(bytes, usual, id, voices, default_guard_bonus)?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let mut source = Enemy {
        source_sha256: definition.source_sha256.clone(),
        secondary_body: secondary_body(bytes, preview)?,
        body_variant: enemy_body_variant(bytes)?,
        attachments: Default::default(),
        uv_animation: Default::default(),
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
        shared,
        output,
    )?);
    (source.attachments, source.trails) = enemy_resources::attachments(bytes, preview)?;
    publish_enemy_carried_motions(bytes, &definition.actions, &mut source, output)?;
    publish_enemy_weapon_variants(bytes, &mut source, output)?;
    refresh_enemy_attachment_animation(bytes, &mut source)?;
    let path = enemy_path(id);
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&source)?)?;
    let definition_path = resonance_content::battle_enemy::path(id);
    crate::write_atomic(
        &output.join(&definition_path),
        &serde_json::to_vec(&definition)?,
    )?;
    Ok([definition_path, path])
}

/// Refresh extra-body bindings without converting geometry or motion data.
pub fn refresh_enemy_secondary_body(
    bytes: &[u8],
    preview: &resonance_content::model_preview::ModelPreview,
    source: &mut Enemy,
) -> Result<()> {
    ensure!(
        source.source_sha256 == crate::digest(bytes),
        "secondary body source differs"
    );
    source.secondary_body = secondary_body(bytes, preview)?;
    Ok(())
}

fn secondary_body(
    bytes: &[u8],
    preview: &resonance_content::model_preview::ModelPreview,
) -> Result<Option<resonance_content::battle_model::SecondaryBody>> {
    use resonance_content::battle_model::{ModelPart, SecondaryBody};
    let profile = usize::from(crate::read::u16(bytes, 4)?);
    if crate::read::u16(bytes, profile + 0xb4)? & 0x400 == 0 {
        return Ok(None);
    }
    let model_slot = *bytes
        .get(profile + 0xbc)
        .context("missing secondary body slot")?;
    let motion_offset = u16::from(
        *bytes
            .get(profile + 0xbd)
            .context("missing secondary body motion offset")?,
    );
    let model_count = *bytes
        .get(profile + 0x1e8)
        .context("missing secondary body model count")?;
    ensure!(
        model_count <= 6 && model_slot < model_count && motion_offset != 0,
        "invalid secondary body binding"
    );
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let body = rig(
        members
            .model(0x180 + usize::from(model_slot) * 4)?
            .context("missing secondary body model")?,
        RigKind::Effect,
    )?;
    let mut layers = Vec::new();
    for part in &preview.parts {
        if part.animation == Some(motion_offset)
            && part
                .scene
                .bone_names
                .iter()
                .eq(body.skeleton.bones.iter().map(|bone| &bone.name))
        {
            let mut layer = part.clone();
            layer
                .scene
                .clips
                .retain(|clip| clip.resource_slot >= motion_offset);
            layers.push(layer);
        }
    }
    ensure!(
        !layers.is_empty() && layers.len() <= 2,
        "secondary body preview binding is missing or ambiguous"
    );
    ensure!(
        layers
            .iter()
            .all(|layer| layer.attached_to == layers[0].attached_to),
        "secondary body layers use different anchors"
    );
    Ok(Some(SecondaryBody {
        model_slot,
        motion_offset,
        model: ModelPart {
            rig: body,
            layer_skeletons: Default::default(),
            layers,
        },
    }))
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

/// Refresh carried texture/effect settings while retaining all prepared artwork.
pub fn refresh_enemy_attachment_animation(bytes: &[u8], source: &mut Enemy) -> Result<()> {
    ensure!(
        source.source_sha256 == crate::digest(bytes),
        "enemy attachment source differs"
    );
    enemy_resources::attachment_animation(bytes, source)
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
    let mut shared = crate::battle_effect::SharedPalettes::read(&usual, output)?;
    let guard_bonus = crate::battle_recoil::guard_defaults(&crate::rel::Rel::read(
        &extracted.join("files").join(&sources.module),
    )?)?[0];
    let paths = monsters
        .iter()
        .enumerate()
        .map(|(id, monster)| {
            publish_enemy(
                &crate::source_assets::enemy_package(&archive, &usual, id as u16)?,
                &usual,
                &voices,
                guard_bonus,
                id as u8,
                &monster.preview,
                &mut shared,
                library,
                output,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    shared.publish()?;
    Ok(paths.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests;
