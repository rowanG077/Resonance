//! Prepare optional carried models and their independent animation.
use anyhow::{Result, ensure};
use resonance_battle::{
    Playback, WeaponDefinition, WeaponLayerDefinition, WeaponPlayback, WeaponTextureAnimation,
};
use resonance_content::{
    animation::Motion,
    battle_model::{AttachmentAnimation, AttachmentUvMode, ModelPart},
    prepared::Files,
};
use std::{collections::BTreeMap, sync::Arc};

/// Prepare a carried component without copying its owner's body motion bank.
pub(super) fn prepare(
    files: &Files,
    part: &ModelPart,
    slot: u8,
    attachment: u16,
    resources: &[u32],
    playback: Option<WeaponPlayback>,
    animation: Option<&AttachmentAnimation>,
) -> Result<Arc<WeaponDefinition>> {
    let layers = layers(files, part, resources, playback, animation)?;
    let mut weapon = WeaponDefinition::new(
        slot,
        attachment,
        layers,
        if matches!(playback, Some(WeaponPlayback::Owner { .. })) {
            ensure!(
                part.rig.skeleton.bones.len() >= 10,
                "missing linked weapon string bones"
            );
            (0..9).map(|bone| [bone, bone + 1]).collect()
        } else {
            vec![]
        },
    )?;
    weapon.effect_interval = animation
        .map(|animation| animation.effect_interval)
        .filter(|&interval| interval != 0)
        .map(|interval| u16::from(interval) + 1);
    Ok(Arc::new(weapon))
}

pub(super) fn secondary_body(
    files: &Files,
    source: &resonance_content::battle_model::SecondaryBody,
    resources: &[u32],
    attachment: Option<u16>,
    initial: Playback,
) -> Result<Arc<WeaponDefinition>> {
    let layers = layers(
        files,
        &source.model,
        resources,
        Some(WeaponPlayback::Body {
            offset: source.motion_offset,
            initial,
        }),
        None,
    )?;
    Ok(Arc::new(WeaponDefinition::body(attachment, layers)?))
}

fn layers(
    files: &Files,
    part: &ModelPart,
    resources: &[u32],
    playback: Option<WeaponPlayback>,
    animation: Option<&AttachmentAnimation>,
) -> Result<Vec<WeaponLayerDefinition>> {
    let rig = &part.rig;
    rig.skeleton.validate()?;
    for skeleton in part.layer_skeletons.values() {
        skeleton.validate()?;
    }
    ensure!(
        resources.len() == part.layers.len() && !resources.is_empty(),
        "weapon layer resources differ"
    );
    part.layers
        .iter()
        .zip(resources)
        .map(|(layer, &resource)| {
            let skeleton = part
                .layer_skeletons
                .get(&layer.scene.resource)
                .unwrap_or(&rig.skeleton);
            let texture_animation = animation
                .map(|animation| {
                    texture_animation(
                        animation,
                        part.layer_skeletons.contains_key(&layer.scene.resource),
                        layer.scene.textures.len(),
                    )
                })
                .transpose()?
                .flatten();
            ensure!(
                skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name)
                    .eq(layer.scene.bone_names.iter()),
                "weapon layer skeleton differs from its mesh"
            );
            let playback = if let Some(playback) = playback {
                playback
            } else if let Some((_, clip)) = layer.selected_clip()? {
                WeaponPlayback::Local(Playback {
                    clip: clip.resource_slot,
                    frame: 0.,
                    rate: 1.,
                    repeat: true,
                })
            } else {
                WeaponPlayback::Rigid
            };
            let mut motions = BTreeMap::new();
            for clip in &layer.scene.clips {
                let motion = Motion::decode(&files.read(&clip.motion)?)?;
                motion.validate(skeleton)?;
                ensure!(
                    motions.insert(clip.resource_slot, motion).is_none(),
                    "duplicate weapon motion slot"
                );
            }
            Ok(WeaponLayerDefinition {
                resource,
                skeleton: skeleton.clone(),
                motions,
                playback,
                secondary_motion: layer.scene.secondary_motion.chains.clone(),
                texture_animation,
            })
        })
        .collect()
}

fn texture_animation(
    animation: &AttachmentAnimation,
    independent: bool,
    texture_count: usize,
) -> Result<Option<WeaponTextureAnimation>> {
    let channel = animation.channels[usize::from(independent)];
    if channel.count == 0 || matches!(channel.mode, AttachmentUvMode::Disabled) {
        return Ok(None);
    }
    ensure!(
        channel.count == 1,
        "weapon texture animation requires one texture"
    );
    let texture = usize::try_from(channel.texture)?;
    ensure!(
        texture < texture_count,
        "weapon texture animation binding is missing"
    );
    Ok(Some(match channel.mode {
        AttachmentUvMode::ScrollU | AttachmentUvMode::ScrollV => {
            let axis = usize::from(matches!(channel.mode, AttachmentUvMode::ScrollV));
            let mut initial = [0; 2];
            let mut step = [0; 2];
            initial[axis] = channel.initial_scroll;
            step[axis] = i16::from(channel.frames_or_step);
            WeaponTextureAnimation::Scroll {
                texture,
                initial,
                step,
            }
        }
        AttachmentUvMode::Frames | AttachmentUvMode::Sequence => {
            let rows = u8::try_from(channel.frames_or_step)?;
            let order = if matches!(channel.mode, AttachmentUvMode::Frames) {
                (0..rows).collect()
            } else {
                animation.frame_order.clone()
            };
            WeaponTextureAnimation::Atlas {
                texture,
                rows,
                ticks_per_frame: u16::try_from(channel.period)? + 1,
                order,
            }
        }
        mode => anyhow::bail!("weapon texture animation {mode:?} is not prepared"),
    }))
}

/// Weapon playback starts at its idle clip, independently of the body entry phase.
pub fn owner_linked() -> WeaponPlayback {
    WeaponPlayback::Owner {
        offset: 60,
        fallback: 60,
        initial: Playback {
            clip: 60,
            frame: 0.,
            rate: 1.,
            repeat: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle_model::AttachmentUvChannel;

    #[test]
    fn atlas_preparation_keeps_authored_order_and_inclusive_period() -> Result<()> {
        let channel = AttachmentUvChannel {
            count: 1,
            texture: 0,
            mode: AttachmentUvMode::Sequence,
            frames_or_step: 4,
            period: 8,
            initial_scroll: 1,
        };
        let mut animation = AttachmentAnimation {
            channels: [
                channel,
                AttachmentUvChannel {
                    mode: AttachmentUvMode::Frames,
                    period: 5,
                    ..channel
                },
            ],
            frame_order: vec![0, 1, 2, 3, 2, 1],
            effect_interval: 0,
        };
        for (independent, expected_ticks, expected_order) in [
            (false, 9, vec![0, 1, 2, 3, 2, 1]),
            (true, 6, vec![0, 1, 2, 3]),
        ] {
            let Some(WeaponTextureAnimation::Atlas {
                texture,
                rows,
                ticks_per_frame,
                order,
            }) = texture_animation(&animation, independent, 1)?
            else {
                panic!("missing weapon atlas");
            };
            assert_eq!(
                (texture, rows, ticks_per_frame, order),
                (0, 4, expected_ticks, expected_order)
            );
        }
        assert!(texture_animation(&animation, false, 0).is_err());
        animation.channels[0].period = -1;
        assert!(texture_animation(&animation, false, 1).is_err());
        Ok(())
    }
}
