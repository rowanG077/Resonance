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
            // Matrix keys carry no matching TRS flags for native cross-fades.
            _ => to,
        }
    }

    pub fn adjusted(self, base: Self, current: Self) -> Self {
        if base == current {
            return self;
        }
        match (self, base, current) {
            (Self::Trs(binding), Self::Trs(base), Self::Trs(current)) => Self::Trs(Transform {
                translation: binding.translation + (current.translation - base.translation),
                rotation: binding.rotation * (base.rotation.inverse() * current.rotation),
                scale: binding.scale * (current.scale / base.scale),
            }),
            _ => {
                assert_eq!(
                    base, current,
                    "late-binding attachment cannot reapply affine secondary deformation to a held pose"
                );
                self
            }
        }
    }
}

/// Native setters replace matrix mode; they do not decompose its translation,
/// scale or shear. Quaternion extraction uses the complete 3x3 basis, then the
/// quaternion-to-matrix writer normalizes it.
pub(crate) fn rotation(matrix: Affine3A) -> Quat {
    let columns = matrix.matrix3.to_cols_array_2d();
    let m = |row: usize, col: usize| columns[col][row];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let mut q = [0.; 4];
    if trace > 0. {
        let scale = (1. + trace).sqrt();
        q[3] = 0.5 * scale;
        let scale = 0.5 / scale;
        q[0] = (m(2, 1) - m(1, 2)) * scale;
        q[1] = (m(0, 2) - m(2, 0)) * scale;
        q[2] = (m(1, 0) - m(0, 1)) * scale;
    } else {
        let mut i = usize::from(m(1, 1) > m(0, 0));
        if m(2, 2) > m(i, i) {
            i = 2;
        }
        let j = (i + 1) % 3;
        let k = (j + 1) % 3;
        let mut scale = ((m(i, i) - (m(j, j) + m(k, k))) + 1.).sqrt();
        q[i] = 0.5 * scale;
        if scale != 0. {
            scale = 0.5 / scale;
        }
        q[3] = (m(k, j) - m(j, k)) * scale;
        q[j] = (m(i, j) + m(j, i)) * scale;
        q[k] = (m(i, k) + m(k, i)) * scale;
    }
    let q = Quat::from_array(q);
    assert!(
        q.is_finite() && q.length_squared().is_finite() && q.length_squared() > 0.,
        "invalid native matrix rotation"
    );
    q.normalize()
}

#[derive(Clone, Copy)]
enum Adjustment {
    Rotate(Quat),
    Scale(Vec3),
}
impl Adjustment {
    fn apply(self, pose: Pose) -> Pose {
        match (self, pose) {
            (Self::Rotate(delta), Pose::Trs(mut value)) => {
                value.rotation *= delta;
                value.into()
            }
            (Self::Rotate(delta), Pose::Affine(matrix)) => {
                Transform::from_rotation(rotation(matrix * Affine3A::from_quat(delta))).into()
            }
            (Self::Scale(scale), Pose::Trs(value)) => value.with_scale(scale).into(),
            (Self::Scale(scale), Pose::Affine(_)) => Transform::from_scale(scale).into(),
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct Locals {
    poses: BTreeMap<Entity, (Affine3A, Transform)>,
    adjustments: BTreeMap<Entity, Vec<Adjustment>>,
}
impl Locals {
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

    fn adjust(&mut self, entity: Entity, transform: &mut Transform, adjustment: Adjustment) {
        let pose = adjustment.apply(self.get(entity, *transform));
        self.set(entity, transform, pose);
        self.adjustments.entry(entity).or_default().push(adjustment);
    }

    pub fn rotate(&mut self, entity: Entity, transform: &mut Transform, delta: Quat) {
        self.adjust(entity, transform, Adjustment::Rotate(delta));
    }

    pub fn scale(&mut self, entity: Entity, transform: &mut Transform, scale: Vec3) {
        self.adjust(entity, transform, Adjustment::Scale(scale));
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

    pub fn adjusted(
        &self,
        entity: Entity,
        mut binding: Pose,
        mut base: Pose,
        current: Pose,
    ) -> Pose {
        if let Some(affine) = &self.affine
            && let Some(adjustments) = affine.adjustments.get(&entity)
        {
            for adjustment in adjustments {
                binding = adjustment.apply(binding);
                base = adjustment.apply(base);
            }
        }
        binding.adjusted(base, current)
    }

    pub fn compute_global_transform(
        &self,
        entity: Entity,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let mut pose = self.local(entity)?.global();
        for parent in self.parents.iter_ancestors(entity) {
            pose = self.local(parent)?.global() * pose;
        }
        Ok(pose)
    }

    pub fn global_with(
        &self,
        entity: Entity,
        locals: &BTreeMap<Entity, Pose>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let local = |entity| {
            locals
                .get(&entity)
                .copied()
                .map_or_else(|| self.local(entity), Ok)
        };
        let mut world = local(entity)?.global();
        for parent in self.parents.iter_ancestors(entity) {
            world = local(parent)?.global() * world;
        }
        Ok(world)
    }

    pub fn has_affine_with(&self, entity: Entity, locals: &BTreeMap<Entity, Pose>) -> bool {
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .any(|entity| {
                matches!(
                    locals
                        .get(&entity)
                        .copied()
                        .map_or_else(|| self.local(entity), Ok),
                    Ok(Pose::Affine(_))
                )
            })
    }
}

pub(super) fn install(app: &mut App) {
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
    affine.adjustments.clear();
    for entity in std::mem::take(&mut affine.poses).into_keys() {
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
    for &root in affine.poses.keys() {
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
    fn native_setters_replace_matrix_mode_and_replay_for_held_attachments() {
        use bevy::ecs::system::RunSystemOnce;
        let matrix = |shear| {
            Affine3A::from_cols(
                Vec3::X.into(),
                Vec3::new(shear, 1., 0.).into(),
                Vec3::Z.into(),
                Vec3::splat(10.).into(),
            )
        };
        let held = Pose::Affine(matrix(0.5));
        let binding = Pose::Affine(matrix(1.5));
        let delta = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mut world = World::new();
        world.init_resource::<Locals>();
        let entity = world.spawn(Transform::IDENTITY).id();
        world
            .run_system_once(
                move |mut nodes: Query<&mut Transform>, mut locals: ResMut<Locals>| {
                    let mut transform = nodes.get_mut(entity).unwrap();
                    locals.set(entity, &mut transform, held);
                    locals.rotate(entity, &mut transform, delta);
                    assert_eq!(transform.translation, Vec3::ZERO);
                    assert_eq!(transform.scale, Vec3::ONE);
                    assert!(
                        transform
                            .rotation
                            .abs_diff_eq(Quat::from_xyzw(0., 0., 0.8, 1.).normalize(), 0.00001)
                    );
                },
            )
            .unwrap();
        let adjusted = world
            .run_system_once(move |helper: Helper| {
                helper.adjusted(entity, binding, held, helper.local(entity).unwrap())
            })
            .unwrap();
        let Pose::Trs(adjusted) = adjusted else {
            panic!("rotation clears matrix mode")
        };
        assert!(
            adjusted
                .rotation
                .abs_diff_eq(Quat::from_xyzw(0., 0., 4. / 7., 1.).normalize(), 0.00001)
        );
        assert_eq!(adjusted.translation, Vec3::ZERO);
        assert_eq!(adjusted.scale, Vec3::ONE);

        let hidden = Adjustment::Scale(Vec3::ZERO).apply(binding);
        assert_eq!(hidden, Transform::from_scale(Vec3::ZERO).into());
        assert_eq!(hidden.adjusted(hidden, hidden), hidden);
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            let expected = Quat::from_axis_angle(axis, std::f32::consts::PI);
            assert!(rotation(Affine3A::from_quat(expected)).dot(expected).abs() > 0.99999);
        }
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
