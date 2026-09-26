//! Battle rig operands from the shared model decoder and native classifier 1BD18.
mod enemy_resources;
pub mod party;
pub mod weapon;
use crate::{geometry, model::Model, read::u16 as half};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::{Bone, Skeleton, Transform, TransformChannels},
    battle_model::{Enemy, Rig, Volume, enemy_path},
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
        transform_kinds: model.nodes.iter().map(|node| node.transform_kind).collect(),
        volumes: vec![],
        target_bones: vec![],
        attack_groups: if kind == RigKind::Body {
            (0..12).map(|slot| (slot, vec![])).collect()
        } else {
            Default::default()
        },
        attachments: Default::default(),
    };
    if kind == RigKind::Effect {
        return Ok(result);
    }
    for (index, name) in names.iter().enumerate() {
        let bone = u16::try_from(index)?;
        let prefix = name.as_bytes().get(..2).unwrap_or_default();
        if kind == RigKind::Body
            && (prefix.eq_ignore_ascii_case(b"mo") || prefix.eq_ignore_ascii_case(b"dm"))
        {
            // The original computes a byte, not an integer parsed from a label.
            // Decorative `mon_hair` consequently carries into the flag bits.
            let mut value = tag(name, 2)?.wrapping_sub(b'0');
            if let Some(digit) = name
                .as_bytes()
                .get(3)
                .filter(|digit| digit.is_ascii_digit())
            {
                value = value.wrapping_mul(10).wrapping_add(*digit - b'0');
            }
            let flags = u16::from(value)
                + if prefix.eq_ignore_ascii_case(b"mo") {
                    0x60
                } else {
                    0x20
                };
            if flags & 0x60 != 0 {
                result.volumes.push(Volume {
                    bone,
                    radius: f32::from(flags & 15) * 10.,
                    hurt: flags & 0x20 != 0,
                    body: flags & 0x40 != 0,
                });
            }
            if prefix.eq_ignore_ascii_case(b"mo") && flags & 0x210 == 0 {
                result.target_bones.push(bone);
            }
        } else if prefix.eq_ignore_ascii_case(b"at")
            || (kind == RigKind::Body && prefix.eq_ignore_ascii_case(b"kk"))
        {
            // Actor classification uses AT's fourth byte; carried weapons use
            // its third byte (153BC). Their group is relative to the instance.
            let column = if kind == RigKind::Weapon { 2 } else { 3 };
            let slot = tag(name, column)?.wrapping_sub(b'0') as i8;
            let slot = u8::try_from(slot).context("negative battle bone group")?;
            if prefix.eq_ignore_ascii_case(b"at") {
                result.attack_groups.entry(slot).or_default().push(bone);
            } else {
                result.attachments.insert(slot, bone);
            }
        } else if kind == RigKind::Body
            && ![b"pa", b"ab", b"ef", b"ki", b"ns"]
                .iter()
                .any(|tag| prefix.eq_ignore_ascii_case(*tag))
            && ![
                "Bone_ude",
                "Bone_te",
                "Bone_yu",
                "Bone_ring",
                "obj_nuno01",
                "manto",
            ]
            .iter()
            .any(|part| name.contains(part))
        {
            result.target_bones.push(bone);
        }
    }
    Ok(result)
}

fn tag(name: &str, column: usize) -> Result<u8> {
    let bytes = name
        .as_bytes()
        .get(..=column)
        .context("truncated battle bone tag")?;
    ensure!(
        bytes.is_ascii() && !bytes.contains(&b'\\'),
        "escaped battle bone tag {name}"
    );
    Ok(bytes[column])
}

fn enemy(bytes: &[u8]) -> Result<Enemy> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("missing enemy profile")?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let (name, hidden_name_units) = enemy_name(bytes)?;
    let strategy = enemy_strategy(bytes)?;
    Ok(Enemy {
        source_sha256: crate::digest(bytes),
        name,
        hidden_name_units,
        profile: crate::battle_profile::read(metadata)?,
        actions: crate::battle_action::enemy::read(bytes)?,
        target_strategy: strategy[0],
        guard_preference: strategy[2],
        attachments: Default::default(),
        trails: Default::default(),
        body: rig(
            members.model(0x18)?.context("missing enemy body")?,
            RigKind::Body,
        )?,
        files: Default::default(),
    })
}

fn enemy_strategy(bytes: &[u8]) -> Result<[u8; 3]> {
    let start = usize::from(half(bytes, 6)?);
    Ok(bytes
        .get(start..start + 3)
        .context("truncated enemy strategy")?
        .try_into()
        .unwrap())
}

fn enemy_name(bytes: &[u8]) -> Result<(String, u16)> {
    let start = usize::from(half(bytes, 6)?) + 4;
    let field = bytes
        .get(start..start + 24)
        .context("truncated enemy name")?;
    let source = crate::read::c_string(field, 0)?;
    let (name, _, invalid) = encoding_rs::SHIFT_JIS.decode(source);
    ensure!(!invalid && !name.is_empty(), "invalid enemy name");
    Ok((name.into_owned(), (source.len() >> 1) as u16))
}

pub(crate) fn publish_enemy(
    bytes: &[u8],
    id: u8,
    preview: &resonance_content::model_preview::ModelPreview,
    library: &Path,
    output: &Path,
) -> Result<String> {
    let mut source = enemy(bytes)?;
    source.files = files(preview.parts.iter().map(|part| &part.scene), library)?;
    source
        .files
        .extend(enemy_resources::publish(bytes, id, output)?);
    (source.attachments, source.trails) = enemy_resources::attachments(bytes, preview)?;
    let path = enemy_path(id);
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&source)?)?;
    Ok(path)
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
pub fn publish_enemies(extracted: &Path, library: &Path, output: &Path) -> Result<Vec<String>> {
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
    let archive = extracted.join("files").join(sources.enemy);
    (0..resonance_content::monster::MONSTER_COUNT)
        .map(|id| {
            let monster: resonance_content::monster::Monster = serde_json::from_slice(
                &std::fs::read(library.join(format!("monsters/{id:03}.json")))?,
            )?;
            publish_enemy(
                &crate::source_assets::enemy_package(&archive, &usual, id as u16)?,
                id as u8,
                &monster.preview,
                library,
                output,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;
