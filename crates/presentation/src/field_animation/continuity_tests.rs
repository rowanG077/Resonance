//! Sparse scene clips must preserve bones that lose their controller.
use super::*;
use resonance_content::{
    animation::{Motion, Skeleton},
    field::FieldAssets,
};

#[test]
#[ignore = "requires locally cooked Triet seal; no devices"]
fn triet_barrier_stays_raised_between_its_open_and_close_animations() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let assets: FieldAssets =
        serde_json::from_slice(&std::fs::read(root.join("fields/map-221.json"))?)?;
    let model = &assets
        .actors
        .iter()
        .find(|a| a.resource == 0xffee_000b)
        .unwrap()
        .parts[0];
    let skeleton = Skeleton::from_glb(&std::fs::read(root.join(&model.mesh))?)?;
    let mut world = World::new();
    let bones = skeleton
        .bones
        .iter()
        .map(|bone| {
            let rest = Transform {
                translation: Vec3::from_array(bone.bind.translation),
                rotation: Quat::from_array(bone.bind.rotation),
                scale: Vec3::from_array(bone.bind.scale),
            };
            (world.spawn_empty().id(), rest)
        })
        .collect();
    let mut rig = Rig::new(bones);
    let tube = usize::from(skeleton.bone("Tube25").unwrap());
    let cylinder = usize::from(skeleton.bone("Cylinder02").unwrap());
    // FIR_D03 plays 80 (rise), 84 (hold), then 88 (lower). Clip 84
    // animates only the orbiting effects; its omitted cylinders stay raised.
    let mut raised = None;
    for slot in [80, 84, 88] {
        let clip = model
            .clips
            .iter()
            .find(|c| c.animation_resource.is_none() && c.resource_slot == slot)
            .unwrap();
        let motion = Motion::decode(&std::fs::read(root.join(&clip.motion))?)?;
        for frame in 0..=motion.duration_frames as usize {
            rig.authored_channels.fill(0);
            let poses = rig.sample_tracks(&motion, frame as f32)?;
            for (i, mut pose) in poses {
                rig.blend_bone(i, &mut pose, 1., true);
            }
            if slot == 84 {
                assert_eq!(
                    rig.presented[tube].pose.global().translation().z,
                    raised.unwrap(),
                    "barrier dropped during its hold animation"
                );
                assert!(
                    (rig.presented[cylinder]
                        .pose
                        .global()
                        .affine()
                        .matrix3
                        .z_axis
                        .length()
                        - 1.)
                        .abs()
                        < 0.001,
                    "barrier collapsed during its hold animation"
                );
            }
        }
        if slot == 80 {
            raised = Some(rig.presented[tube].pose.global().translation().z);
        }
    }
    assert!(
        rig.presented[cylinder]
            .pose
            .global()
            .affine()
            .matrix3
            .z_axis
            .length()
            < 0.001
    );
    Ok(())
}
