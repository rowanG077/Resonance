//! Affine local poses for authored matrix keys and their attached scene instances.
use bevy::{ecs::system::SystemParam, math::Affine3A, prelude::*};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pose {
    Trs(Transform),
    Affine(Affine3A),
}

impl From<Transform> for Pose {
    fn from(value: Transform) -> Self {
        Self::Trs(value)
    }
}

impl Pose {
    pub fn global(self) -> GlobalTransform {
        match self {
            Self::Trs(value) => value.into(),
            Self::Affine(value) => value.into(),
        }
    }

    pub fn mix(self, to: Self, weight: f32) -> Self {
        match (self, to) {
            (Self::Trs(from), Self::Trs(to)) => Self::Trs(Transform {
                translation: from.translation.lerp(to.translation, weight),
                rotation: from.rotation.slerp(to.rotation, weight),
                scale: from.scale.lerp(to.scale, weight),
            }),
            // Matrix keys carry no matching TRS channels for cross-fades.
            _ => to,
        }
    }
}

pub(crate) fn rotation(matrix: Affine3A) -> Quat {
    Quat::from_array(
        resonance_content::animation::matrix_rotation(Mat4::from(matrix).to_cols_array_2d())
            .expect("invalid matrix rotation"),
    )
}

#[derive(Resource, Default)]
pub(crate) struct Locals {
    poses: BTreeMap<Entity, (Affine3A, Transform)>,
    worlds: BTreeMap<Entity, GlobalTransform>,
    translations: BTreeMap<Entity, Vec3>,
}
impl Locals {
    /// Dynamics and outline copying can write a world matrix even when
    /// an ancestor has zero scale. No local matrix can represent that result.
    pub fn set_world(&mut self, entity: Entity, pose: GlobalTransform) {
        assert!(pose.affine().is_finite(), "nonfinite world pose");
        let mut matrix = pose.affine();
        matrix.translation -=
            bevy::math::Vec3A::from(self.translations.get(&entity).copied().unwrap_or_default());
        self.worlds.insert(entity, matrix.into());
    }

    pub fn get(&self, entity: Entity, transform: Transform) -> Pose {
        self.poses
            .get(&entity)
            .map_or(Pose::Trs(transform), |&(matrix, reference)| {
                assert_eq!(
                    transform, reference,
                    "affine bone changed without an explicit pose operation"
                );
                Pose::Affine(matrix)
            })
    }

    pub fn set(&mut self, entity: Entity, transform: &mut Transform, pose: Pose) {
        match pose {
            Pose::Trs(value) => {
                self.poses.remove(&entity);
                *transform = value;
            }
            Pose::Affine(matrix) => {
                self.poses.insert(entity, (matrix, *transform));
            }
        }
    }

    /// Rotate about the bone origin in its parent's axes, retaining scale and shear.
    pub fn rotate(&mut self, entity: Entity, transform: &mut Transform, delta: Quat) {
        let pose = match self.get(entity, *transform) {
            Pose::Trs(mut value) => {
                value.rotation = delta * value.rotation;
                value.into()
            }
            Pose::Affine(mut value) => {
                value.matrix3 = bevy::math::Mat3A::from_quat(delta) * value.matrix3;
                Pose::Affine(value)
            }
        };
        self.set(entity, transform, pose);
    }

    pub fn face_camera(&mut self, entity: Entity, transform: &mut Transform, camera: Quat) {
        self.rotate(entity, transform, camera);
    }

    pub fn scale(&mut self, entity: Entity, transform: &mut Transform, scale: Vec3) {
        let pose = match self.get(entity, *transform) {
            Pose::Trs(value) => value.with_scale(scale).into(),
            Pose::Affine(mut value) => {
                let matrix = &mut value.matrix3;
                for (axis, length) in [&mut matrix.x_axis, &mut matrix.y_axis, &mut matrix.z_axis]
                    .into_iter()
                    .zip(scale.to_array())
                {
                    *axis = axis.normalize_or_zero() * length;
                }
                Pose::Affine(value)
            }
        };
        self.set(entity, transform, pose);
    }
    pub fn translation_boundary(&mut self, entity: Entity) {
        self.translations.entry(entity).or_default();
    }

    pub fn translate(&mut self, entity: Entity, delta: Vec3) {
        *self.translations.entry(entity).or_default() += delta;
    }
}

/// The same current local pose feeds attachment queries and final propagation.
#[derive(SystemParam)]
pub(crate) struct Helper<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    transforms: Query<'w, 's, &'static Transform>,
    affine: Option<Res<'w, Locals>>,
}
impl Helper<'_, '_> {
    pub fn local(&self, entity: Entity) -> Result<Pose, bevy::ecs::query::QueryEntityError> {
        let transform = *self.transforms.get(entity)?;
        Ok(self
            .affine
            .as_ref()
            .map_or(Pose::Trs(transform), |a| a.get(entity, transform)))
    }

    pub fn has_affine(&self, entity: Entity) -> bool {
        self.affine.as_ref().is_some_and(|affine| {
            std::iter::once(entity)
                .chain(self.parents.iter_ancestors(entity))
                .any(|e| affine.poses.contains_key(&e))
        })
    }

    fn world_override(&self, entity: Entity) -> Option<GlobalTransform> {
        self.affine.as_ref()?.worlds.get(&entity).copied()
    }

    pub fn has_world_translation(&self, entity: Entity) -> bool {
        self.affine
            .as_ref()
            .is_some_and(|affine| affine.translations.contains_key(&entity))
    }

    fn translated(&self, entity: Entity, pose: GlobalTransform) -> GlobalTransform {
        let Some(affine) = self
            .affine
            .as_ref()
            .filter(|affine| !affine.translations.is_empty())
        else {
            return pose;
        };
        let offset = std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find_map(|entity| affine.translations.get(&entity));
        let mut matrix = pose.affine();
        matrix.translation += bevy::math::Vec3A::from(offset.copied().unwrap_or_default());
        matrix.into()
    }

    pub fn compute_global_transform(
        &self,
        entity: Entity,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        self.global_with(entity, &BTreeMap::new())
    }

    /// Parent overrides let attached actors resolve in dependency order.
    pub fn global_with(
        &self,
        entity: Entity,
        locals: &BTreeMap<Entity, Pose>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let mut world = GlobalTransform::IDENTITY;
        for node in std::iter::once(entity).chain(self.parents.iter_ancestors(entity)) {
            if let Some(local) = locals.get(&node) {
                world = local.global() * world;
            } else if let Some(pose) = self.world_override(node) {
                world = pose * world;
                break;
            } else {
                world = self.local(node)?.global() * world;
            }
        }
        Ok(self.translated(entity, world))
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Locals>()
        .add_systems(PreUpdate, clear)
        .add_systems(
            PostUpdate,
            propagate
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
}

fn clear(mut affine: ResMut<Locals>, mut transforms: Query<&mut Transform>) {
    let worlds = std::mem::take(&mut affine.worlds);
    let translations = std::mem::take(&mut affine.translations);
    for entity in std::mem::take(&mut affine.poses)
        .into_keys()
        .chain(worlds.into_keys())
        .chain(translations.into_keys())
    {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            // A dropped matrix clip must restore ordinary descendant globals too.
            transform.set_changed();
        }
    }
}

fn propagate(
    affine: Res<Locals>,
    helper: Helper,
    children: Query<&Children>,
    mut globals: Query<&mut GlobalTransform>,
) {
    let mut seen = BTreeSet::new();
    for &root in affine
        .poses
        .keys()
        .chain(affine.worlds.keys())
        .chain(affine.translations.keys())
    {
        for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
            if seen.insert(entity) {
                let pose = helper
                    .compute_global_transform(entity)
                    .expect("affine hierarchy must have local transforms");
                *globals
                    .get_mut(entity)
                    .expect("affine hierarchy must have global transforms") = pose;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_animation::Binding;
    use resonance_content::animation::{FRAME_HZ, Motion, Transform as Bind};

    #[derive(Resource)]
    struct Playback(Option<f32>);

    #[test]
    fn platform_and_child_effect_receive_independent_world_translations() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        install(&mut app);
        let root = app.world_mut().spawn(Transform::IDENTITY).id();
        let platform = app
            .world_mut()
            .spawn((Transform::IDENTITY, ChildOf(root)))
            .id();
        let effect = app
            .world_mut()
            .spawn((Transform::from_xyz(0., 0., 20.), ChildOf(platform)))
            .id();
        let child = app
            .world_mut()
            .spawn((Transform::from_xyz(0., 0., 30.), ChildOf(effect)))
            .id();
        let mesh = app
            .world_mut()
            .spawn((Transform::IDENTITY, ChildOf(effect)))
            .id();
        app.add_systems(
            Update,
            move |mut affine: ResMut<Locals>, mut once: Local<bool>| {
                if *once {
                    return;
                }
                *once = true;
                for bone in [platform, effect, child] {
                    affine.translation_boundary(bone);
                }
                for entity in [platform, effect] {
                    affine.translate(entity, Vec3::new(0., 0., 600.));
                }
            },
        );
        app.update();
        for (entity, z) in [(platform, 600.), (effect, 620.), (child, 50.), (mesh, 620.)] {
            assert_eq!(
                app.world()
                    .get::<GlobalTransform>(entity)
                    .unwrap()
                    .translation()
                    .z,
                z
            );
        }
        app.world_mut()
            .run_system_once(move |helper: Helper| {
                assert_eq!(
                    helper
                        .compute_global_transform(effect)
                        .unwrap()
                        .translation()
                        .z,
                    620.
                );
            })
            .unwrap();
        let driven = GlobalTransform::from_translation(Vec3::new(10., 20., 900.));
        app.world_mut()
            .resource_mut::<Locals>()
            .set_world(effect, driven);
        app.world_mut()
            .run_system_once(move |helper: Helper| {
                assert_eq!(helper.compute_global_transform(effect).unwrap(), driven);
                assert_eq!(helper.compute_global_transform(mesh).unwrap(), driven);
            })
            .unwrap();
        // Controllers do not modify local poses, and releasing them restores
        // ordinary propagation even when the local transforms stay unchanged.
        app.update();
        assert_eq!(
            app.world()
                .get::<GlobalTransform>(effect)
                .unwrap()
                .translation()
                .z,
            20.
        );
    }

    #[test]
    #[ignore = "requires locally cooked Thoda scenery; no devices"]
    fn thoda_platform_and_teleporter_bones_rise_by_the_same_amount() -> anyhow::Result<()> {
        use resonance_content::{animation::Skeleton, field::FieldAssets};
        let root = std::path::PathBuf::from(std::env::var_os("RESONANCE_WORLD_ASSETS").unwrap());
        let field: FieldAssets =
            serde_json::from_slice(&std::fs::read(root.join("fields/map-9.json"))?)?;
        let skeleton = Skeleton::from_glb(&std::fs::read(root.join(&field.parts[0].mesh))?)?;
        // The platform and both teleporters must rise together.
        let targets = ["aqu_d03_base00", "aqu_d03_po00", "aqu_d03_po01"]
            .map(|name| skeleton.bone(name).unwrap());
        assert_eq!(skeleton.bones[targets[1] as usize].parent, Some(targets[0]));
        assert_eq!(skeleton.bones[targets[2] as usize].parent, Some(targets[0]));
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        install(&mut app);
        let actor = app
            .world_mut()
            .spawn(
                Transform::from_rotation(Quat::from_rotation_x(0.5))
                    .with_scale(Vec3::new(2., 3., 4.)),
            )
            .id();
        let mut entities = Vec::new();
        for bone in &skeleton.bones {
            let bind = bone.bind;
            let parent = bone.parent.map_or(actor, |i| entities[i as usize]);
            entities.push(
                app.world_mut()
                    .spawn((
                        Transform {
                            translation: Vec3::from_array(bind.translation),
                            rotation: Quat::from_array(bind.rotation),
                            scale: Vec3::from_array(bind.scale),
                        },
                        ChildOf(parent),
                    ))
                    .id(),
            );
        }
        app.update();
        let before: Vec<_> = entities
            .iter()
            .map(|e| *app.world().get::<GlobalTransform>(*e).unwrap())
            .collect();
        let nodes = entities.clone();
        app.insert_resource(Playback(None)).add_systems(
            Update,
            move |frame: Res<Playback>, mut affine: ResMut<Locals>| {
                for &node in &nodes {
                    affine.translation_boundary(node);
                }
                for target in targets {
                    affine.translate(
                        nodes[target as usize],
                        Vec3::Z * frame.0.unwrap_or_default(),
                    );
                }
            },
        );
        for height in [600. / 180., 300., 600.] {
            app.world_mut().resource_mut::<Playback>().0 = Some(height);
            app.update();
            for (i, &entity) in entities.iter().enumerate() {
                let expected = before[i].translation()
                    + if targets.contains(&(i as u16)) {
                        Vec3::Z * height
                    } else {
                        Vec3::ZERO
                    };
                let actual = app
                    .world()
                    .get::<GlobalTransform>(entity)
                    .unwrap()
                    .translation();
                assert!(
                    actual.distance(expected) < 0.001,
                    "{}: {actual:?} != {expected:?}",
                    skeleton.bones[i].name
                );
            }
        }
        Ok(())
    }

    #[test]
    fn world_pose_survives_collapsed_parent_and_restores_after_release() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        install(&mut app);
        let collapsed = Transform::from_scale(Vec3::new(0., 0., 3.));
        let root = app.world_mut().spawn(collapsed).id();
        let rest = Transform::from_xyz(1., 2., 3.);
        let bone = app.world_mut().spawn((rest, ChildOf(root))).id();
        let offset = Transform::from_xyz(4., 5., 6.);
        let child = app.world_mut().spawn((offset, ChildOf(bone))).id();
        // Dynamics can place the bone outside the collapsed parent's Z axis;
        // no local transform under that parent could reproduce this matrix.
        let driven = GlobalTransform::from_translation(Vec3::new(20., 30., 40.));
        app.add_systems(
            Update,
            move |mut locals: ResMut<Locals>, mut once: Local<bool>| {
                if !*once {
                    locals.set_world(bone, driven);
                    *once = true;
                }
            },
        );
        app.update();
        let expected = driven.mul_transform(offset);
        assert_eq!(
            *app.world().get::<GlobalTransform>(child).unwrap(),
            expected
        );
        app.world_mut()
            .run_system_once(move |helper: Helper| {
                assert_eq!(helper.compute_global_transform(child).unwrap(), expected);
                assert_eq!(
                    helper.global_with(child, &BTreeMap::new()).unwrap(),
                    expected
                );
            })
            .unwrap();
        assert_eq!(*app.world().get::<Transform>(bone).unwrap(), rest);
        app.update();
        assert_eq!(
            *app.world().get::<GlobalTransform>(child).unwrap(),
            GlobalTransform::from(collapsed)
                .mul_transform(rest)
                .mul_transform(offset)
        );
        app.world_mut().entity_mut(root).insert(Transform::IDENTITY);
        app.update();
        assert_eq!(
            *app.world().get::<GlobalTransform>(child).unwrap(),
            GlobalTransform::from(rest).mul_transform(offset)
        );
    }

    #[test]
    fn affine_adjustments_preserve_position_and_other_components() {
        let entity = Entity::PLACEHOLDER;
        let mut transform = Transform::IDENTITY;
        let mut poses = Locals::default();
        let matrix = Affine3A::from_cols(
            Vec3::X.into(),
            Vec3::new(0.5, 1., 0.).into(),
            Vec3::Z.into(),
            Vec3::splat(10.).into(),
        );
        poses.set(entity, &mut transform, Pose::Affine(matrix));
        poses.rotate(entity, &mut transform, Quat::from_rotation_z(1.));
        let rotated = poses.get(entity, transform).global().affine();
        assert_eq!(rotated.translation, matrix.translation);
        assert!((rotated.matrix3.y_axis.length() - matrix.matrix3.y_axis.length()).abs() < 0.0001);
        poses.scale(entity, &mut transform, Vec3::splat(2.));
        let scaled = poses.get(entity, transform).global().affine();
        assert_eq!(scaled.translation, matrix.translation);
        assert!(
            scaled
                .matrix3
                .y_axis
                .normalize()
                .abs_diff_eq(rotated.matrix3.y_axis.normalize(), 0.0001)
        );
        assert!((scaled.matrix3.y_axis.length() - 2.).abs() < 0.0001);
    }

    #[test]
    #[allow(clippy::type_complexity)] // Exercise the same separated read/write queries as attachments.
    fn matrix_keys_preserve_shear_through_hierarchy_and_attachment() {
        let motion: Motion = serde_json::from_value(serde_json::json!({
            "duration_frames":2., "tracks":[
                {"bone":0,"bind_channels":0,"period_frames":2.,"times":[0.,1.],
                 "translation":null,"scale":null,"rotation":null,
                 "matrices":[[1.,0.5,0.,3.,0.,1.,0.,4.,0.,0.,1.,5.],
                             [1.,-0.5,0.,6.,0.,1.,0.,7.,0.,0.,1.,8.]]},
                {"bone":1,"bind_channels":0,"period_frames":2.,"times":[0.],
                 "translation":null,"scale":null,"rotation":null,
                 "matrices":[[1.,0.,0.,1.,0.2,1.,0.,2.,0.,0.,1.,3.]]}
            ]
        }))
        .unwrap();
        let motion = Motion::decode(&motion.encode().unwrap()).unwrap();
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        install(&mut app);
        let parent_pose = Transform::from_xyz(10., 20., 30.)
            .with_scale(Vec3::new(2., 3., 4.))
            .with_rotation(Quat::from_rotation_z(0.4));
        let root = app.world_mut().spawn(parent_pose).id();
        let rest = Transform::from_xyz(2., 4., 6.);
        let bone = app.world_mut().spawn((rest, ChildOf(root))).id();
        let nested = app
            .world_mut()
            .spawn((Transform::IDENTITY, ChildOf(bone)))
            .id();
        let child_pose = Transform::from_xyz(2., 3., 4.);
        let child = app.world_mut().spawn((child_pose, ChildOf(nested))).id();
        let attachment = app.world_mut().spawn(Transform::IDENTITY).id();
        let attachment_child = app
            .world_mut()
            .spawn((child_pose, ChildOf(attachment)))
            .id();
        let offset = Transform::from_xyz(4., 5., 6.);
        let binding = Binding(vec![(bone, rest), (nested, Transform::IDENTITY)]);
        let sampled = motion.clone();
        app.insert_resource(Playback(Some(0.125))).add_systems(
            Update,
            (
                move |frame: Res<Playback>,
                      mut transforms: Query<&mut Transform>,
                      mut affine: ResMut<Locals>| {
                    if let Some(frame) = frame.0 {
                        binding
                            .sample(&sampled, frame / FRAME_HZ, &mut transforms, &mut affine)
                            .unwrap();
                    }
                },
                move |mut poses: ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>| {
                    let global = poses
                        .p0()
                        .compute_global_transform(child)
                        .unwrap()
                        .mul_transform(offset);
                    let (mut transforms, mut affine) = poses.p1();
                    affine.set(
                        attachment,
                        &mut transforms.get_mut(attachment).unwrap(),
                        Pose::Affine(global.affine()),
                    );
                },
            )
                .chain(),
        );
        for frame in [0.125, 1., 1.75] {
            app.world_mut().resource_mut::<Playback>().0 = Some(frame);
            app.update();
            let matrix = |index: usize| {
                Affine3A::from_mat4(Mat4::from_cols_array_2d(
                    &motion.tracks[index]
                        .sample_matrix(frame, Bind::default())
                        .unwrap(),
                ))
            };
            let expected =
                parent_pose.compute_affine() * matrix(0) * matrix(1) * child_pose.compute_affine();
            assert!(
                app.world()
                    .get::<GlobalTransform>(child)
                    .unwrap()
                    .affine()
                    .abs_diff_eq(expected, 0.0001)
            );
            assert!(
                app.world()
                    .get::<GlobalTransform>(attachment_child)
                    .unwrap()
                    .affine()
                    .abs_diff_eq(
                        expected * offset.compute_affine() * child_pose.compute_affine(),
                        0.0001
                    )
            );
        }
        app.world_mut().resource_mut::<Playback>().0 = None;
        app.update();
        let ordinary =
            parent_pose.compute_affine() * rest.compute_affine() * child_pose.compute_affine();
        assert!(
            app.world()
                .get::<GlobalTransform>(child)
                .unwrap()
                .affine()
                .abs_diff_eq(ordinary, 0.0001)
        );
        assert_eq!(*app.world().get::<Transform>(bone).unwrap(), rest);
    }
}
