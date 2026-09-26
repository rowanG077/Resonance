//! Carried geometry and contacts share one prepared model and one sampled pose.
use anyhow::{Context, Result, ensure};
use resonance_battle::{Anchor, ModelDefinition, Playback, WeaponDefinition, WeaponPlayback};
use resonance_content::{animation::Motion, battle_model::ModelPart, prepared::Files};
use std::{collections::BTreeMap, sync::Arc};

/// The encounter allocates the presentation resource ID before attaching the
/// weapon. Returned AT groups address the same actor contacts as body groups.
pub fn attach(
    files: &Files,
    model: &mut ModelDefinition,
    part: &ModelPart,
    slot: u8,
    attachment: u16,
    resource: u32,
    playback: WeaponPlayback,
) -> Result<Vec<Vec<u16>>> {
    ensure!(
        usize::from(attachment) < model.skeleton.bones.len()
            && model.weapons.iter().all(|weapon| weapon.slot != slot),
        "invalid or duplicate weapon attachment"
    );
    let rig = &part.rig;
    rig.skeleton.validate()?;
    ensure!(
        rig.transform_kinds.len() == rig.skeleton.bones.len()
            && rig.transform_kinds.iter().all(|&kind| kind == 1),
        "unsupported weapon bone transform"
    );
    let primary = &part
        .layers
        .first()
        .context("missing weapon model layer")?
        .scene;
    for layer in &part.layers {
        ensure!(
            rig.skeleton
                .bones
                .iter()
                .map(|bone| &bone.name)
                .eq(layer.scene.bone_names.iter())
                && primary
                    .clips
                    .iter()
                    .map(|clip| (clip.resource_slot, &clip.motion))
                    .eq(layer
                        .scene
                        .clips
                        .iter()
                        .map(|clip| (clip.resource_slot, &clip.motion)))
                && layer.scene.secondary_motion.chains.is_empty(),
            "unsupported weapon layer bindings or secondary motion"
        );
    }
    let mut motions = BTreeMap::new();
    for clip in &primary.clips {
        let motion = Motion::decode(&files.read(&clip.motion)?)?;
        motion.validate(&rig.skeleton)?;
        ensure!(
            motions.insert(clip.resource_slot, motion).is_none(),
            "duplicate weapon motion slot"
        );
    }
    match playback {
        WeaponPlayback::Rigid => ensure!(
            motions.is_empty(),
            "rigid weapon requires unanimated layers"
        ),
        WeaponPlayback::Owner {
            fallback, initial, ..
        } => ensure!(
            motions.contains_key(&fallback) && motions.contains_key(&initial.clip),
            "owner-linked weapon motions are missing; recook weapons"
        ),
    }
    let count = rig
        .attack_groups
        .keys()
        .next_back()
        .map_or(0, |&group| usize::from(group) + 1);
    ensure!(count <= 12, "invalid weapon contact group");
    let start = model.anchors.len()
        + model
            .weapons
            .iter()
            .map(|weapon| weapon.anchors.len())
            .sum::<usize>();
    let mut anchors = Vec::new();
    let mut groups = vec![Vec::new(); count];
    for (&group, bones) in &rig.attack_groups {
        ensure!(bones.len() <= 7, "too many weapon contact bones");
        for &bone in bones {
            ensure!(
                usize::from(bone) < rig.skeleton.bones.len(),
                "invalid weapon contact bone"
            );
            groups[usize::from(group)].push((start + anchors.len()).try_into()?);
            anchors.push(Anchor {
                bone,
                offset: [0.; 3],
            });
        }
    }
    ensure!(start + anchors.len() <= 256, "too many battle anchors");
    model.weapons.push(Arc::new(WeaponDefinition {
        slot,
        resource,
        attachment,
        skeleton: rig.skeleton.clone(),
        motions,
        playback,
        anchors,
        links: if matches!(playback, WeaponPlayback::Owner { .. }) {
            ensure!(
                rig.skeleton.bones.len() >= 10,
                "missing linked weapon string bones"
            );
            (0..9).map(|bone| [bone, bone + 1]).collect()
        } else {
            vec![]
        },
    }));
    Ok(groups)
}

/// Constructor 15B20 samples owner bank slot 60 at frame zero, independently of
/// the body's entry phase. Subsequent 2C05C requests select body clip + 60.
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
