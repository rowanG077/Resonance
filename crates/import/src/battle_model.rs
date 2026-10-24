//! Battle skeletons and carried-model attachment slots.
mod enemy_resources;
mod fixture;
pub mod party;
pub mod weapon;
use crate::{geometry, model::Model};
use anyhow::{Context, Result, ensure};
pub use fixture::bind_party_motion;
use resonance_content::{
    animation::{Bone, Skeleton, Transform, TransformChannels},
    battle_model::{Enemy, Rig},
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
pub(crate) fn prepare_enemy(
    bytes: &[u8],
    usual: &[u8],
    voices: &crate::battle_voice::Source,
    default_guard_bonus: u8,
    id: u8,
    preview: &resonance_content::model_preview::ModelPreview,
    shared: &mut crate::battle_effect::SharedPalettes,
    output: &Path,
) -> Result<PreparedEnemy> {
    let definition = crate::battle_enemy::read(bytes, usual, id, voices, default_guard_bonus)?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let mut source = Enemy {
        source_sha256: definition.source_sha256.clone(),
        secondary_body: secondary_body(bytes, preview)?,
        body_variant: enemy_body_variant(bytes)?,
        attached_models: enemy_resources::attached_models(bytes)?,
        attachments: Default::default(),
        uv_animation: Default::default(),
        weapon_variants: Default::default(),
        trails: Default::default(),
        body: rig(
            members.model(0x18)?.context("missing enemy body")?,
            RigKind::Body,
        )?,
    };
    let effects = enemy_resources::prepare(bytes, id, &source.body.skeleton, shared, output)?;
    (source.attachments, source.trails) = enemy_resources::attachments(bytes, preview)?;
    publish_enemy_carried_motions(bytes, &definition.actions, &mut source, output)?;
    publish_enemy_weapon_variants(bytes, &mut source, output)?;
    bind_enemy_attachment_animation(bytes, &mut source)?;
    Ok(PreparedEnemy {
        preview: preview.clone(),
        artwork: source,
        rules: definition,
        effects,
    })
}

/// Native enemy data handed directly to its named resource writer.
#[derive(Clone)]
pub struct PreparedEnemy {
    pub preview: resonance_content::model_preview::ModelPreview,
    pub artwork: Enemy,
    pub rules: resonance_content::battle_enemy::Definition,
    pub effects: resonance_content::battle_effect::SourceBank,
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
pub fn bind_enemy_attachment_animation(bytes: &[u8], source: &mut Enemy) -> Result<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests;
