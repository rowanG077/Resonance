//! Prepare optional carried models and their independent animation.
use anyhow::{Result, ensure};
use resonance_battle::{
    Playback, ResourceId, WeaponDefinition, WeaponLayerDefinition, WeaponPlayback,
    WeaponTextureAnimation,
};
use resonance_content::{
    battle_model::{AttachmentAnimation, AttachmentTextureAnimation, ModelPart},
    prepared::Files,
};
use std::{collections::BTreeMap, sync::Arc};

/// Prepare a carried component without copying its owner's body motion bank.
pub(super) fn prepare(
    files: &Files,
    part: &ModelPart,
    slot: u8,
    attachment: u16,
    resources: &[ResourceId],
    playback: Option<WeaponPlayback>,
    animation: Option<&AttachmentAnimation>,
) -> Result<Arc<WeaponDefinition>> {
    let layers = layers(files, part, resources, playback, animation)?;
    let mut weapon = WeaponDefinition::new(
        resonance_battle::WeaponSlot(slot),
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
        .and_then(|animation| animation.effect_every_ticks)
        .map(|interval| interval.get());
    Ok(Arc::new(weapon))
}

pub(super) fn secondary_body(
    files: &Files,
    source: &resonance_content::battle_model::SecondaryBody,
    resources: &[ResourceId],
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
    resources: &[ResourceId],
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
                let motion = clip.decode_for_skeleton(&files.read(&clip.motion)?, skeleton)?;
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
                secondary_motion: layer
                    .scene
                    .secondary_motion
                    .prepare(&layer.scene.bone_names)?,
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
    let Some(channel) = (if independent {
        &animation.independent
    } else {
        &animation.primary
    }) else {
        return Ok(None);
    };
    let (texture, prepared) = match channel {
        AttachmentTextureAnimation::Scroll {
            texture,
            step_milli_uv,
            offset_milli_uv,
        } => {
            let texture = usize::from(*texture);
            (
                texture,
                WeaponTextureAnimation::Scroll {
                    texture,
                    initial: *offset_milli_uv,
                    step: *step_milli_uv,
                },
            )
        }
        AttachmentTextureAnimation::Atlas {
            texture,
            rows,
            ticks_per_frame,
            frames,
        } => {
            let texture = usize::from(*texture);
            let order = if frames.is_empty() {
                (0..*rows).collect()
            } else {
                frames.clone()
            };
            (
                texture,
                WeaponTextureAnimation::Atlas {
                    texture,
                    rows: *rows,
                    ticks_per_frame: ticks_per_frame.get(),
                    order,
                },
            )
        }
    };
    ensure!(
        texture < texture_count,
        "weapon texture animation binding is missing"
    );
    Ok(Some(prepared))
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

    #[test]
    #[ignore = "requires current stored battle scenes; metadata only"]
    fn stored_scene_weapon_controls_preserve_disabled_and_signed_scroll_channels() -> Result<()> {
        use anyhow::Context;
        use resonance_content::{battle_scene::document, library::Library};
        let root =
            std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
        let mut snapshot = Library::open(std::path::Path::new(&root), &[])?.snapshot();
        // Equipment and enemy controls are compared with both native sources
        // in import. These four scene-owned weapons complete that corpus.
        for (mapping, id) in [
            (document::TECHNIQUES, 301),
            (document::TECHNIQUES, 312),
            (document::TECHNIQUES, 314),
            (document::SKILLS, 228),
        ] {
            let path = snapshot.resolve(mapping, id, Some("scene.json"))?;
            let scene: document::Definition = snapshot.json(&path, 16 * 1024 * 1024)?;
            let weapon = scene.weapon.context("missing scene weapon")?;
            assert!(weapon.parts.keys().copied().eq([0]));
            let expected = if mapping == document::SKILLS {
                serde_json::json!({"0":{
                    "primary":{"kind":"scroll","step_milli_uv":[0,80]},
                    "independent":{"kind":"scroll","step_milli_uv":[0,-12]}
                }})
            } else {
                serde_json::json!({})
            };
            assert_eq!(
                serde_json::to_value(weapon.uv_animation)?,
                expected,
                "{path}"
            );
        }
        Ok(())
    }

    #[test]
    fn scroll_preparation_keeps_signed_axes_offsets_and_independent_selection() -> Result<()> {
        let mut animation: AttachmentAnimation = serde_json::from_value(serde_json::json!({
            "primary":{"kind":"scroll","texture":1,"step_milli_uv":[80,0],"offset_milli_uv":[-128,4]},
            "independent":{"kind":"scroll","texture":2,"step_milli_uv":[0,-12],"offset_milli_uv":[7,-9]}
        }))?;
        for (independent, expected) in [
            (false, (1, [-128, 4], [80, 0])),
            (true, (2, [7, -9], [0, -12])),
        ] {
            let Some(WeaponTextureAnimation::Scroll {
                texture,
                initial,
                step,
            }) = texture_animation(&animation, independent, 3)?
            else {
                panic!("missing scroll channel")
            };
            assert_eq!((texture, initial, step), expected);
            assert!(texture_animation(&animation, independent, expected.0).is_err());
        }
        animation.primary = None;
        assert!(texture_animation(&animation, false, 0)?.is_none());
        assert!(texture_animation(&animation, true, 3)?.is_some());
        assert!(texture_animation(&AttachmentAnimation::default(), true, 0)?.is_none());
        Ok(())
    }

    #[test]
    fn atlas_preparation_keeps_authored_order_and_direct_timing() -> Result<()> {
        let animation: AttachmentAnimation = serde_json::from_value(serde_json::json!({
            "primary":{"kind":"atlas", "rows":4, "ticks_per_frame":9, "frames":[0,1,2,3,2,1]},
            "independent":{"kind":"atlas", "rows":4, "ticks_per_frame":6}
        }))?;
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
        assert!(
            serde_json::from_value::<AttachmentAnimation>(serde_json::json!({
                "primary":{"kind":"atlas", "rows":4, "ticks_per_frame":0}
            }))
            .is_err()
        );
        Ok(())
    }
}
