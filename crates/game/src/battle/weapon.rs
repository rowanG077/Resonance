//! Prepare optional carried models and their independent animation.
use anyhow::{Result, ensure};
use resonance_battle::{Playback, WeaponDefinition, WeaponLayerDefinition, WeaponPlayback};
use resonance_content::{animation::Motion, battle_model::ModelPart, prepared::Files};
use std::{collections::BTreeMap, sync::Arc};

/// Prepare a carried component without copying its owner's body motion bank.
pub(super) fn prepare(
    files: &Files,
    part: &ModelPart,
    slot: u8,
    attachment: u16,
    resources: &[u32],
    playback: Option<WeaponPlayback>,
) -> Result<Arc<WeaponDefinition>> {
    let rig = &part.rig;
    rig.skeleton.validate()?;
    for skeleton in part.layer_skeletons.values() {
        skeleton.validate()?;
    }
    ensure!(
        resources.len() == part.layers.len() && !resources.is_empty(),
        "weapon layer resources differ"
    );
    let layers = part
        .layers
        .iter()
        .zip(resources)
        .map(|(layer, &resource)| {
            let skeleton = part
                .layer_skeletons
                .get(&layer.scene.resource)
                .unwrap_or(&rig.skeleton);
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
            if !matches!(playback, WeaponPlayback::Rigid) {
                for clip in layer.scene.clips.iter().filter(|clip| match playback {
                    WeaponPlayback::Local(initial) => clip.resource_slot == initial.clip,
                    WeaponPlayback::Owner { .. } => true,
                    WeaponPlayback::Rigid => false,
                }) {
                    let motion = Motion::decode(&files.read(&clip.motion)?)?;
                    motion.validate(skeleton)?;
                    ensure!(
                        motions.insert(clip.resource_slot, motion).is_none(),
                        "duplicate weapon motion slot"
                    );
                }
            }
            Ok(WeaponLayerDefinition {
                resource,
                skeleton: skeleton.clone(),
                motions,
                playback,
                secondary_motion: layer.scene.secondary_motion.chains.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Arc::new(WeaponDefinition::new(
        slot,
        attachment,
        layers,
        if matches!(playback, Some(WeaponPlayback::Owner { .. })) {
            ensure!(
                rig.skeleton.bones.len() >= 10,
                "missing linked weapon string bones"
            );
            (0..9).map(|bone| [bone, bone + 1]).collect()
        } else {
            vec![]
        },
    )?))
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
