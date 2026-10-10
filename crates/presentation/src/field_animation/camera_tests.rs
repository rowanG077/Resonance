use super::*;
use crate::sparse_animation::affine::Helper;
use bevy::{ecs::system::RunSystemOnce, math::Affine3A};

#[test]
fn camera_facing_preserves_affine_pose_translation_and_attachment_offsets() {
    let mut world = World::new();
    world.init_resource::<Locals>();
    let parent = Transform::from_xyz(30., 40., 50.)
        .with_rotation(Quat::from_rotation_z(0.7))
        .with_scale(Vec3::new(2., 3., 4.));
    let root = world.spawn(parent).id();
    let bone = world.spawn((Transform::IDENTITY, ChildOf(root))).id();
    let tip = world
        .spawn((Transform::from_xyz(0., 8., 0.), ChildOf(bone)))
        .id();
    let local = Affine3A::from_cols(
        Vec3::new(2., 0., 0.).into(),
        Vec3::new(0.5, 3., 0.).into(),
        Vec3::Z.into(),
        Vec3::new(10., 20., 30.).into(),
    );
    let camera = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    world
        .run_system_once(
            move |mut nodes: Query<&mut Transform>, mut poses: ResMut<Locals>| {
                let mut transform = nodes.get_mut(bone).unwrap();
                poses.set(bone, &mut transform, Pose::Affine(local));
                poses.face_camera(bone, &mut transform, camera);
            },
        )
        .unwrap();
    world
        .run_system_once(move |helper: Helper| {
            let result = helper.local(bone).unwrap().global().affine();
            assert_eq!(result.translation, local.translation);
            for point in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!(
                    result
                        .transform_vector3(point)
                        .distance(camera * local.transform_vector3(point))
                        < 0.0001
                );
            }
            let expected = parent.compute_affine().transform_point3(
                Vec3::from(local.translation) + camera * local.transform_vector3(Vec3::Y * 8.),
            );
            assert!(
                helper
                    .compute_global_transform(tip)
                    .unwrap()
                    .translation()
                    .distance(expected)
                    < 0.0001
            );
        })
        .unwrap();
}

#[test]
#[ignore = "requires locally cooked Mana scenery; no devices"]
fn mana_flames_stand_at_the_lamps_and_follow_camera_changes() -> anyhow::Result<()> {
    use resonance_content::{
        animation::{Motion, Skeleton},
        field::FieldAssets,
    };
    use std::{path::PathBuf, sync::Arc};
    let assets = PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
    let manifest: FieldAssets =
        serde_json::from_slice(&std::fs::read(assets.join("fields/map-362.json"))?)?;
    let part = manifest.parts.iter().find(|p| p.resource == 2).unwrap();
    let bytes = std::fs::read(assets.join(&part.mesh))?;
    let skeleton = Skeleton::from_glb(&bytes)?;
    let length = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
    let gltf: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length])?;
    let motion = Arc::new(Motion::decode(&std::fs::read(
        assets.join(&part.clips[0].motion),
    )?)?);
    let mut field = resonance_game::field::FieldSession::new(
        &std::fs::read(assets.join(&manifest.script))?,
        Vec::new(),
        &manifest,
    )?;
    let mut camera = resonance_events::camera::CameraRig::default();
    camera.position = [0., -1000., 0.];
    field.events.world.field_camera = Some(camera);
    let mut app = App::new();
    app.add_plugins(bevy::transform::TransformPlugin);
    crate::sparse_animation::affine::install(&mut app);
    app.insert_resource(crate::field_view::Session(field));
    let root = app.world_mut().spawn(Transform::IDENTITY).id();
    let mut entities = Vec::new();
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let parent = bone.parent.map_or(root, |i| entities[i as usize]);
        entities.push(
            app.world_mut()
                .spawn((
                    Transform {
                        translation: Vec3::from_array(bone.bind.translation),
                        rotation: Quat::from_array(bone.bind.rotation),
                        scale: Vec3::from_array(bone.bind.scale),
                    },
                    bevy::gltf::GltfExtras {
                        value: gltf["nodes"][index]["extras"].to_string(),
                    },
                    ChildOf(parent),
                ))
                .id(),
        );
    }
    let flame_bone = skeleton.bone("fire_12").unwrap();
    let expected = Vec3::from_array(skeleton.sample_point(&motion, 0., flame_bone, [0.; 3])?);
    let count = skeleton.bones.len();
    let rig = app
        .world_mut()
        .run_system_once(
            move |children: Query<&Children>,
                  nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>| {
                Rig::from_scene(root, count, &children, &nodes)
            },
        )
        .unwrap()?;
    assert!(!rig.camera_facing.is_empty());
    app.world_mut().entity_mut(root).insert(rig);
    app.add_systems(
        Update,
        move |state: State,
              mut rigs: Query<&mut Rig>,
              mut nodes: Query<&mut Transform>,
              mut affine: ResMut<Locals>| {
            let camera = state.get().events.world.field_camera.as_ref().unwrap();
            for mut rig in &mut rigs {
                rig.sample(&motion, 0., 1., &mut nodes, &mut affine)
                    .unwrap();
                rig.face_camera(
                    crate::field_view::camera_transform(camera).rotation,
                    &mut nodes,
                    &mut affine,
                )
                .unwrap();
            }
        },
    );
    let flame = entities[flame_bone as usize];
    for position in [[0., -1000., 0.], [1000., 0., 0.], [0., -1000., 0.]] {
        app.world_mut()
            .resource_mut::<crate::field_view::Session>()
            .0
            .events
            .world
            .field_camera
            .as_mut()
            .unwrap()
            .position = position;
        app.update();
        let pose = app.world().get::<GlobalTransform>(flame).unwrap();
        assert!(
            pose.translation().distance(expected) < 0.001,
            "camera movement detached the flame"
        );
        let normal = pose.affine().transform_vector3(Vec3::Z).normalize();
        assert!(normal.dot(Vec3::from_array(position).normalize()) > 0.9999);
    }
    Ok(())
}
