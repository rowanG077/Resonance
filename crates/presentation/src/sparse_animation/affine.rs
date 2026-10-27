//! Affine local poses for authored matrix keys and their attached scene instances.
use bevy::{
    ecs::{entity::EntityHashMap, system::SystemParam},
    math::Affine3A,
    prelude::*,
};
use resonance_content::animation::Matrix;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) struct MatrixEndpoint {
    pub poses: PosePair,
}

impl MatrixEndpoint {
    pub fn new(from: &Matrix, to: &Matrix) -> Self {
        let pose = |m| Pose::Affine(Affine3A::from_mat4(Mat4::from_cols_array_2d(m)));
        Self {
            poses: PosePair::new(pose(from), pose(to)),
        }
    }

    pub fn sample(self, alpha: f32) -> Matrix {
        self.poses
            .sample(alpha)
            .global()
            .to_matrix()
            .to_cols_array_2d()
    }
}

pub(crate) struct Matrices {
    pairs: Vec<PosePair>,
    parents: Vec<Option<usize>>,
    order: Vec<usize>,
    from: Vec<Matrix>,
    to: Vec<Matrix>,
}

impl Matrices {
    pub fn new(
        from: &resonance_content::animation::pose::Palette,
        to: &resonance_content::animation::pose::Palette,
    ) -> Self {
        assert_eq!(
            from.len(),
            to.len(),
            "matrix endpoints require matching skeletons"
        );
        let mut parents = Vec::with_capacity(from.len());
        let pairs = from
            .joints
            .iter()
            .zip(&to.joints)
            .enumerate()
            .map(|(i, (a, b))| {
                if a.parent == b.parent {
                    parents.push(a.parent);
                    PosePair::new(a.local.into(), b.local.into())
                } else {
                    // A secondary world override can start/end under a collapsed
                    // parent. Both endpoints remain expressible in model space.
                    parents.push(None);
                    MatrixEndpoint::new(&from[i], &to[i]).poses
                }
            })
            .collect();
        let mut order = Vec::with_capacity(from.len());
        let mut visited = vec![false; from.len()];
        fn visit(
            i: usize,
            parents: &[Option<usize>],
            visited: &mut [bool],
            order: &mut Vec<usize>,
        ) {
            if visited[i] {
                return;
            }
            visited[i] = true;
            if let Some(parent) = parents[i] {
                visit(parent, parents, visited, order);
            }
            order.push(i);
        }
        for i in 0..from.len() {
            visit(i, &parents, &mut visited, &mut order);
        }
        Self {
            pairs,
            parents,
            order,
            from: from.global.clone(),
            to: to.global.clone(),
        }
    }

    pub fn endpoint(&self, bone: usize, future: bool) -> Matrix {
        if future {
            self.to[bone]
        } else {
            self.from[bone]
        }
    }

    pub fn sample(&self, alpha: f32, output: &mut [Matrix]) {
        assert_eq!(self.pairs.len(), output.len());
        if alpha <= 0. {
            output.copy_from_slice(&self.from);
        } else if alpha >= 1. {
            output.copy_from_slice(&self.to);
        } else {
            for &i in &self.order {
                let local = self.pairs[i]
                    .sample(alpha)
                    .global()
                    .to_matrix()
                    .to_cols_array_2d();
                output[i] = self.parents[i].map_or(local, |parent| {
                    resonance_content::animation::multiply(output[parent], local)
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pose {
    Trs(Transform),
    Affine(Affine3A),
}

#[derive(Clone, Copy)]
pub(crate) struct PoseEndpoint {
    pub entity: Entity,
    pub poses: PosePair,
    pub global: bool,
}

impl PoseEndpoint {
    pub fn apply(self, alpha: f32, nodes: &mut Query<&mut Transform>, affine: &mut Locals) {
        if let Ok(mut transform) = nodes.get_mut(self.entity) {
            let pose = self.poses.sample(alpha);
            affine.replace_for_render(self.entity, &mut transform, pose);
            if self.global {
                affine.set_world(self.entity, pose.global());
            }
        }
    }
}

impl From<Transform> for Pose {
    fn from(value: Transform) -> Self {
        Self::Trs(value)
    }
}

/// Rotation and the remaining affine basis are factored once per endpoint.
/// Authored TRS poses retain their signed scales without decomposition.
#[derive(Clone, Copy)]
pub(crate) struct PosePair {
    pub from: Pose,
    pub to: Pose,
    basis: Option<[(Quat, bevy::math::Mat3A); 2]>,
}

impl PosePair {
    pub fn new(from: Pose, to: Pose) -> Self {
        let basis = if matches!((from, to), (Pose::Trs(_), Pose::Trs(_)))
            || from.global().affine().matrix3 == to.global().affine().matrix3
        {
            None
        } else {
            let a = from.global().affine().matrix3;
            let b = to.global().affine().matrix3;
            let ra = orthogonal_basis(a);
            let rb = orthogonal_basis(b);
            let ra = ra.unwrap_or_else(|| collapsed_rotation(a, rb.unwrap_or(Quat::IDENTITY)));
            let rb = rb.unwrap_or_else(|| collapsed_rotation(b, ra));
            Some([
                (ra, bevy::math::Mat3A::from_quat(ra.inverse()) * a),
                (rb, bevy::math::Mat3A::from_quat(rb.inverse()) * b),
            ])
        };
        Self { from, to, basis }
    }

    pub fn sample(self, weight: f32) -> Pose {
        if weight <= 0. || self.from == self.to {
            return self.from;
        }
        if weight >= 1. {
            return self.to;
        }
        if matches!((self.from, self.to), (Pose::Trs(_), Pose::Trs(_))) {
            return self.from.mix(self.to, weight);
        }
        let a = self.from.global().affine();
        let b = self.to.global().affine();
        Pose::Affine(Affine3A {
            matrix3: self.basis.map_or(a.matrix3, |[(ra, a), (rb, b)]| {
                bevy::math::Mat3A::from_quat(ra.slerp(rb, weight))
                    * (a * (1. - weight) + b * weight)
            }),
            translation: a.translation.lerp(b.translation, weight),
        })
    }
}

/// QR decomposition gives a proper orthonormal rotation while keeping shear and
/// reflection in the residual. Normalize first so small model scales are valid.
fn orthogonal_basis(matrix: bevy::math::Mat3A) -> Option<Quat> {
    let axes = [matrix.x_axis, matrix.y_axis, matrix.z_axis].map(|v| Vec3::from(v).try_normalize());
    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
        let (Some(x), Some(y)) = (axes[a], axes[b]) else {
            continue;
        };
        let perpendicular = y - x * x.dot(y);
        if perpendicular.length_squared() <= 1e-12 {
            continue;
        }
        let y = perpendicular.normalize();
        let mut basis = [Vec3::ZERO; 3];
        basis[a] = x;
        basis[b] = y;
        basis[(b + 1) % 3] = x.cross(y);
        return Some(Quat::from_mat3(&Mat3::from_cols(basis[0], basis[1], basis[2])).normalize());
    }
    None
}

/// A rank-zero/one matrix does not define a complete orientation. Retain the
/// other endpoint's unobservable axes instead of introducing an arbitrary spin.
fn collapsed_rotation(matrix: bevy::math::Mat3A, reference: Quat) -> Quat {
    for (axis, value) in
        [Vec3::X, Vec3::Y, Vec3::Z]
            .into_iter()
            .zip([matrix.x_axis, matrix.y_axis, matrix.z_axis])
    {
        if let Some(direction) = Vec3::from(value).try_normalize() {
            return (Quat::from_rotation_arc(reference * axis, direction) * reference).normalize();
        }
    }
    reference
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

impl From<resonance_content::animation::pose::LocalPose> for Pose {
    fn from(value: resonance_content::animation::pose::LocalPose) -> Self {
        use resonance_content::animation::pose::LocalPose;
        match value {
            LocalPose::Trs(value) => Self::Trs(Transform {
                translation: Vec3::from_array(value.translation),
                // Native pose matrices normalize blended and quantized rotations.
                // Bevy's TRS conversion assumes a unit quaternion instead.
                rotation: Quat::from_array(value.rotation).normalize(),
                scale: Vec3::from_array(value.scale),
            }),
            LocalPose::Affine(value) => {
                Self::Affine(Affine3A::from_mat4(Mat4::from_cols_array_2d(&value)))
            }
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
    poses: EntityHashMap<(Affine3A, Transform)>,
    worlds: EntityHashMap<GlobalTransform>,
    translations: EntityHashMap<Vec3>,
}
impl Locals {
    pub fn has_translation(&self, entity: Entity) -> bool {
        self.translations.contains_key(&entity)
    }
    pub fn replace_for_render(&mut self, entity: Entity, transform: &mut Transform, pose: Pose) {
        self.worlds.remove(&entity);
        self.translations.remove(&entity);
        self.set(entity, transform, pose);
    }
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

    /// Joint adjustments follow the joint's own axes, including its bind rotation.
    pub fn rotate_local(&mut self, entity: Entity, transform: &mut Transform, delta: Quat) {
        let pose = match self.get(entity, *transform) {
            Pose::Trs(mut value) => {
                value.rotation *= delta;
                value.into()
            }
            Pose::Affine(mut value) => {
                value.matrix3 *= bevy::math::Mat3A::from_quat(delta);
                Pose::Affine(value)
            }
        };
        self.set(entity, transform, pose);
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
    /// Resolve interpolation endpoints once, retaining local-space attachment
    /// relationships even when dynamics or authored offsets supply world poses.
    pub fn pose_endpoints<'a>(
        &'a self,
        entities: impl Iterator<Item = Entity> + 'a,
        locals: &'a BTreeMap<Entity, Pose>,
        adjustments: &'a Locals,
        dynamics: &'a BTreeMap<Entity, GlobalTransform>,
    ) -> impl Iterator<Item = Result<PoseEndpoint, bevy::ecs::query::QueryEntityError>> + 'a {
        let mut current_cache = EntityHashMap::default();
        let mut future_cache = EntityHashMap::default();
        entities.map(move |entity| {
            if !self.requires_world_interpolation(entity)
                && !dynamics.contains_key(&entity)
                && !adjustments.has_translation(entity)
            {
                return Ok(PoseEndpoint {
                    entity,
                    poses: PosePair::new(
                        self.local(entity)?,
                        locals
                            .get(&entity)
                            .copied()
                            .map_or_else(|| self.local(entity), Ok)?,
                    ),
                    global: false,
                });
            }
            let from = self.compute_global_cached(entity, &mut current_cache)?;
            let to = self.predicted_global_cached(
                entity,
                locals,
                adjustments,
                dynamics,
                &mut future_cache,
            )?;
            let parents = self.parent(entity).and_then(|parent| {
                Some((
                    self.compute_global_cached(parent, &mut current_cache)
                        .ok()?,
                    self.predicted_global_cached(
                        parent,
                        locals,
                        adjustments,
                        dynamics,
                        &mut future_cache,
                    )
                    .ok()?,
                ))
            });
            let (from, to, global) = if let Some((a, b)) = parents.filter(|(a, b)| {
                a.affine().matrix3.determinant().abs() > 1e-12
                    && b.affine().matrix3.determinant().abs() > 1e-12
            }) {
                (
                    Pose::Affine(a.affine().inverse() * from.affine()),
                    Pose::Affine(b.affine().inverse() * to.affine()),
                    false,
                )
            } else {
                (Pose::Affine(from.affine()), Pose::Affine(to.affine()), true)
            };
            Ok(PoseEndpoint {
                entity,
                poses: PosePair::new(from, to),
                global,
            })
        })
    }

    /// A disposable authored pose supplies its own translation boundaries.
    /// Current secondary world overrides must not feed into the next solver step.
    pub fn predicted_global(
        &self,
        entity: Entity,
        locals: &BTreeMap<Entity, Pose>,
        adjustments: &Locals,
        overrides: &BTreeMap<Entity, GlobalTransform>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let mut world = GlobalTransform::IDENTITY;
        let mut offset = None;
        for node in std::iter::once(entity).chain(self.parents.iter_ancestors(entity)) {
            offset = offset.or_else(|| adjustments.translations.get(&node).copied());
            if let Some(pose) = overrides.get(&node) {
                // Match Locals::set_world: descendants may establish a new
                // translation boundary instead of inheriting this bone's offset.
                let mut matrix = pose.affine();
                matrix.translation -= bevy::math::Vec3A::from(
                    adjustments
                        .translations
                        .get(&node)
                        .copied()
                        .unwrap_or_default(),
                );
                world = GlobalTransform::from(matrix) * world;
                break;
            }
            if let Some(local) = locals.get(&node) {
                world = local.global() * world;
            } else {
                world = self.local(node)?.global() * world;
            }
        }
        let mut matrix = world.affine();
        matrix.translation += bevy::math::Vec3A::from(offset.unwrap_or_default());
        Ok(matrix.into())
    }

    pub fn predicted_has_affine(&self, entity: Entity, locals: &BTreeMap<Entity, Pose>) -> bool {
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .any(|e| {
                locals
                    .get(&e)
                    .copied()
                    .or_else(|| self.local(e).ok())
                    .is_some_and(|p| matches!(p, Pose::Affine(_)))
            })
    }

    pub fn parent(&self, entity: Entity) -> Option<Entity> {
        self.parents.get(entity).ok().map(ChildOf::parent)
    }

    pub fn predicted_global_cached(
        &self,
        entity: Entity,
        locals: &BTreeMap<Entity, Pose>,
        adjustments: &Locals,
        overrides: &BTreeMap<Entity, GlobalTransform>,
        cache: &mut EntityHashMap<GlobalTransform>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let pose = self.predicted_cached(entity, locals, adjustments, overrides, cache)?;
        let offset = std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find_map(|e| adjustments.translations.get(&e))
            .copied()
            .unwrap_or_default();
        let mut matrix = pose.affine();
        matrix.translation += bevy::math::Vec3A::from(offset);
        Ok(matrix.into())
    }

    fn predicted_cached(
        &self,
        entity: Entity,
        locals: &BTreeMap<Entity, Pose>,
        adjustments: &Locals,
        overrides: &BTreeMap<Entity, GlobalTransform>,
        cache: &mut EntityHashMap<GlobalTransform>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        if let Some(&pose) = cache.get(&entity) {
            return Ok(pose);
        }
        let pose = if let Some(pose) = overrides.get(&entity) {
            let mut matrix = pose.affine();
            matrix.translation -= bevy::math::Vec3A::from(
                adjustments
                    .translations
                    .get(&entity)
                    .copied()
                    .unwrap_or_default(),
            );
            matrix.into()
        } else {
            let local = locals
                .get(&entity)
                .copied()
                .map_or_else(|| self.local(entity), Ok)?
                .global();
            if let Some(parent) = self.parent(entity) {
                self.predicted_cached(parent, locals, adjustments, overrides, cache)? * local
            } else {
                local
            }
        };
        cache.insert(entity, pose);
        Ok(pose)
    }

    pub fn compute_global_cached(
        &self,
        entity: Entity,
        cache: &mut EntityHashMap<GlobalTransform>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        self.cached_global(entity, cache)
            .map(|p| self.translated(entity, p))
    }
    pub fn local(&self, entity: Entity) -> Result<Pose, bevy::ecs::query::QueryEntityError> {
        let transform = *self.transforms.get(entity)?;
        Ok(self
            .affine
            .as_ref()
            .map_or(Pose::Trs(transform), |a| a.get(entity, transform)))
    }

    pub fn has_affine(&self, entity: Entity) -> bool {
        self.affine.as_ref().is_some_and(|affine| {
            !affine.poses.is_empty()
                && std::iter::once(entity)
                    .chain(self.parents.iter_ancestors(entity))
                    .any(|e| affine.poses.contains_key(&e))
        })
    }

    fn world_override(&self, entity: Entity) -> Option<GlobalTransform> {
        self.affine.as_ref()?.worlds.get(&entity).copied()
    }

    /// Ordinary bones can stay in local space, retaining their authored TRS.
    pub fn requires_world_interpolation(&self, entity: Entity) -> bool {
        self.affine
            .as_ref()
            .is_some_and(|a| a.worlds.contains_key(&entity) || a.translations.contains_key(&entity))
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

    /// Resolve a batch against this unchanged pose, sharing ancestor products.
    /// The cache is local to the batch: bone adjustments between systems must
    /// always be visible to subsequent attachment and outline queries.
    pub fn compute_global_transforms(
        &self,
        entities: impl IntoIterator<Item = Entity>,
    ) -> Result<Vec<GlobalTransform>, bevy::ecs::query::QueryEntityError> {
        let mut cache = EntityHashMap::default();
        entities
            .into_iter()
            .map(|entity| {
                self.cached_global(entity, &mut cache)
                    .map(|pose| self.translated(entity, pose))
            })
            .collect()
    }

    fn cached_global(
        &self,
        entity: Entity,
        cache: &mut EntityHashMap<GlobalTransform>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        if let Some(&pose) = cache.get(&entity) {
            return Ok(pose);
        }
        let pose = if let Some(pose) = self.world_override(entity) {
            pose
        } else {
            let local = self.local(entity)?.global();
            if let Ok(parent) = self.parents.get(entity) {
                self.cached_global(parent.parent(), cache)? * local
            } else {
                local
            }
        };
        // Keep translation boundaries outside the cached product: a child may
        // override its parent's world-space offset with a different (or zero) one.
        cache.insert(entity, pose);
        Ok(pose)
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
    for &entity in affine
        .poses
        .keys()
        .chain(affine.worlds.keys())
        .chain(affine.translations.keys())
    {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.set_changed();
        }
    }
    affine.poses.clear();
    affine.worlds.clear();
    affine.translations.clear();
}

#[allow(clippy::too_many_arguments)] // Hierarchy queries and retained traversal scratch buffers.
pub(crate) fn propagate(
    affine: Res<Locals>,
    helper: Helper,
    children: Query<&Children>,
    mut globals: Query<&mut GlobalTransform>,
    mut seen: Local<bevy::ecs::entity::EntityHashSet>,
    mut entities: Local<Vec<Entity>>,
    mut stack: Local<Vec<Entity>>,
    mut cache: Local<EntityHashMap<GlobalTransform>>,
) {
    let _timer = crate::performance::smoothing_profile::Timer::start(3);
    seen.clear();
    entities.clear();
    stack.clear();
    cache.clear();
    for &root in affine
        .poses
        .keys()
        .chain(affine.worlds.keys())
        .chain(affine.translations.keys())
    {
        stack.push(root);
        while let Some(entity) = stack.pop() {
            if !seen.insert(entity) {
                continue;
            }
            entities.push(entity);
            if let Ok(children) = children.get(entity) {
                stack.extend(children.iter());
            }
        }
    }
    for &entity in entities.iter() {
        let pose = helper
            .cached_global(entity, &mut cache)
            .map(|p| helper.translated(entity, p))
            .expect("affine hierarchy must have local transforms");
        *globals
            .get_mut(entity)
            .expect("affine hierarchy must have global transforms") = pose;
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
    fn pose_endpoints_keep_dynamic_children_attached_and_handle_collapsed_parents() {
        use bevy::ecs::system::RunSystemOnce;
        for collapsed in [false, true] {
            let mut world = World::new();
            world.init_resource::<Locals>();
            let scale = if collapsed { Vec3::ZERO } else { Vec3::ONE };
            let root = world.spawn(Transform::from_scale(scale)).id();
            let child = world
                .spawn((Transform::from_xyz(50., 0., 0.), ChildOf(root)))
                .id();
            world
                .resource_mut::<Locals>()
                .set_world(child, GlobalTransform::from_translation(Vec3::X * 50.));
            let endpoints = world
                .run_system_once(move |helper: Helper| {
                    let locals = BTreeMap::from([
                        (
                            root,
                            Transform::from_rotation(Quat::from_rotation_z(
                                std::f32::consts::FRAC_PI_2,
                            ))
                            .with_scale(scale)
                            .into(),
                        ),
                        (child, Transform::from_xyz(50., 0., 0.).into()),
                    ]);
                    let dynamics =
                        BTreeMap::from([(child, GlobalTransform::from_translation(Vec3::Y * 50.))]);
                    helper
                        .pose_endpoints(
                            [root, child].into_iter(),
                            &locals,
                            &Locals::default(),
                            &dynamics,
                        )
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap()
                })
                .unwrap();
            assert_eq!(endpoints[1].global, collapsed);
            // Reusing endpoints must not feed a previous rendered pose back into prediction.
            for alpha in [0., 0.25, 0.5, 0.75, 0.999] {
                let endpoints = endpoints.clone();
                world
                    .run_system_once(
                        move |mut nodes: Query<&mut Transform>, mut affine: ResMut<Locals>| {
                            for &endpoint in &endpoints {
                                endpoint.apply(alpha, &mut nodes, &mut affine);
                            }
                        },
                    )
                    .unwrap();
                let position = world
                    .run_system_once(move |helper: Helper| {
                        helper
                            .compute_global_transform(child)
                            .unwrap()
                            .translation()
                    })
                    .unwrap();
                let expected = if collapsed {
                    (Vec3::X * 50.).lerp(Vec3::Y * 50., alpha)
                } else {
                    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2 * alpha) * Vec3::X * 50.
                };
                assert!(
                    position.distance(expected) < 0.001,
                    "{position:?} != {expected:?}"
                );
            }
        }
    }

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
                let entities = [mesh, child, platform, effect, child];
                let batch = helper.compute_global_transforms(entities).unwrap();
                for (entity, pose) in entities.into_iter().zip(batch) {
                    assert_eq!(pose, helper.compute_global_transform(entity).unwrap());
                }
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
                assert_eq!(
                    helper
                        .compute_global_transforms([mesh, effect, mesh])
                        .unwrap(),
                    [driven; 3]
                );
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
                        let mut endpoints = Vec::new();
                        binding
                            .sample_endpoints(
                                (&sampled, frame / FRAME_HZ),
                                (&sampled, frame / FRAME_HZ),
                                &mut endpoints,
                            )
                            .unwrap();
                        for endpoint in endpoints {
                            endpoint.apply(0., &mut transforms, &mut affine);
                        }
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

#[cfg(test)]
mod interpolation_tests {
    use super::*;
    #[test]
    fn skeletons_preserve_local_offsets_through_turns_and_collapsed_parent_scales() {
        use resonance_content::animation::{
            Bone, Skeleton, Transform as Trs,
            pose::{LocalPose, Palette},
        };
        // Children need not precede their parents in the source palette.
        let skeleton = Skeleton {
            bones: vec![
                Bone {
                    name: "child".into(),
                    parent: Some(1),
                    bind_channels: Default::default(),
                    bind: Trs::default(),
                },
                Bone {
                    name: "parent".into(),
                    parent: None,
                    bind_channels: Default::default(),
                    bind: Trs::default(),
                },
            ],
        };
        let child = Trs {
            translation: [50., 0., 0.],
            ..Default::default()
        };
        for scale in [Vec3::ONE, Vec3::ZERO, Vec3::new(-2., 3., 0.5)] {
            let from = Trs {
                scale: scale.to_array(),
                ..Default::default()
            };
            let to = Trs {
                rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2).to_array(),
                ..Default::default()
            };
            let palette = |parent| {
                Palette::new(
                    &skeleton,
                    vec![LocalPose::Trs(child), LocalPose::Trs(parent)],
                )
                .unwrap()
            };
            let a = palette(from);
            let b = palette(to);
            let matrices = Matrices::new(&a, &b);
            let mut output = vec![Mat4::IDENTITY.to_cols_array_2d(); 2];
            for alpha in [0., 0.25, 0.5, 0.75, 1.] {
                matrices.sample(alpha, &mut output);
                let parent = Mat4::from_cols_array_2d(&from.blend(to, alpha).matrix());
                let expected = parent * Mat4::from_cols_array_2d(&child.matrix());
                assert!(Mat4::from_cols_array_2d(&output[0]).abs_diff_eq(expected, 0.0001));
            }
        }
    }
    #[test]
    fn rotating_affine_poses_preserve_scale_shear_and_reflection() {
        for scale in [
            Vec3::splat(0.1),
            Vec3::new(0.001, 3., 12.),
            Vec3::new(-2., 3., 0.5),
            Vec3::new(0., 3., 0.5),
            Vec3::new(2., 0., 0.),
            Vec3::ZERO,
        ] {
            for shear in [0., 0.7] {
                let mut basis = Mat3::from_diagonal(scale);
                basis.y_axis.x += shear;
                let start = Quat::from_euler(EulerRot::XYZ, 0.1, -0.2, 0.3);
                let end = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * start;
                let pose = |rotation| {
                    Pose::Affine(Affine3A {
                        matrix3: (Mat3::from_quat(rotation) * basis).into(),
                        translation: Vec3::new(12., 3., -4.).into(),
                    })
                };
                let pair = PosePair::new(pose(start), pose(end));
                let gram = basis.transpose() * basis;
                for alpha in [0., 0.1, 0.5, 0.9, 1.] {
                    let actual = Mat3::from(pair.sample(alpha).global().affine().matrix3);
                    assert!(
                        (actual.transpose() * actual).abs_diff_eq(gram, 0.0001),
                        "scale={scale:?}, shear={shear}, alpha={alpha}"
                    );
                    assert!((actual.determinant() - basis.determinant()).abs() < 0.0001);
                }
            }
        }
    }

    #[test]
    fn collapsed_endpoint_borrows_orientation_without_spinning() {
        let rotation = Quat::from_euler(EulerRot::XYZ, 0.3, -0.6, 1.8);
        let pose = |scale| {
            Pose::Affine(
                Transform::from_rotation(rotation)
                    .with_scale(scale)
                    .compute_affine(),
            )
        };
        let pair = PosePair::new(pose(Vec3::ZERO), pose(Vec3::new(0.1, 3., 2.)));
        for alpha in [0.1, 0.5, 0.9] {
            let expected = pose(Vec3::new(0.1, 3., 2.) * alpha).global().affine();
            assert!(
                pair.sample(alpha)
                    .global()
                    .affine()
                    .abs_diff_eq(expected, 0.00001)
            );
        }
    }
    #[test]
    fn stationary_affine_poses_do_not_drift_between_ticks() {
        let mut matrix = Transform::from_xyz(127., -23., 81.)
            .with_rotation(Quat::from_euler(EulerRot::ZYX, 0.23, 0.91, -0.1))
            .with_scale(Vec3::new(-1.3, 0.9, 2.4))
            .compute_affine();
        matrix.matrix3.y_axis.x += 0.13;
        let pose = Pose::Affine(matrix);
        let translated = Pose::Affine(Affine3A {
            translation: matrix.translation + bevy::math::Vec3A::Z,
            ..matrix
        });
        for alpha in [0.01, 0.25, 0.5, 0.9, 0.99] {
            assert_eq!(PosePair::new(pose, pose).sample(alpha), pose);
            assert_eq!(
                PosePair::new(pose, translated)
                    .sample(alpha)
                    .global()
                    .affine()
                    .matrix3,
                matrix.matrix3
            );
        }
    }
    #[test]
    fn predicted_solver_poses_respect_independent_translation_boundaries() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let parent = world.spawn(Transform::IDENTITY).id();
        let child = world
            .spawn((Transform::from_xyz(0., 0., 10.), ChildOf(parent)))
            .id();
        world
            .run_system_once(move |helper: Helper| {
                let mut adjustments = Locals::default();
                adjustments.translate(parent, Vec3::Z * 600.);
                adjustments.translation_boundary(child);
                let overrides =
                    [(parent, GlobalTransform::from_translation(Vec3::Z * 900.))].into();
                let locals = BTreeMap::new();
                for order in [[parent, child], [child, parent]] {
                    let mut cache = EntityHashMap::default();
                    for entity in order {
                        assert_eq!(
                            helper
                                .predicted_global_cached(
                                    entity,
                                    &locals,
                                    &adjustments,
                                    &overrides,
                                    &mut cache
                                )
                                .unwrap(),
                            helper
                                .predicted_global(entity, &locals, &adjustments, &overrides)
                                .unwrap(),
                        );
                    }
                }
                assert_eq!(
                    helper
                        .predicted_global(parent, &locals, &adjustments, &overrides)
                        .unwrap()
                        .translation()
                        .z,
                    900.
                );
                assert_eq!(
                    helper
                        .predicted_global(child, &locals, &adjustments, &overrides)
                        .unwrap()
                        .translation()
                        .z,
                    310.
                );
            })
            .unwrap();
    }

    #[test]
    fn matrices_preserve_endpoints_and_stay_finite_with_shear_reflection_and_zero_scale() {
        for scale in [Vec3::ONE, Vec3::new(-2., 3., 0.5), Vec3::ZERO] {
            let from = Pose::Affine(
                Transform::from_rotation(Quat::from_rotation_z(359_f32.to_radians()))
                    .with_scale(scale)
                    .compute_affine(),
            );
            let mut to = Transform::from_xyz(8., 2., -4.)
                .with_rotation(Quat::from_rotation_z(1_f32.to_radians()))
                .with_scale(scale)
                .compute_affine();
            to.matrix3.y_axis.x += 0.4;
            let to = Pose::Affine(to);
            assert_eq!(PosePair::new(from, to).sample(0.), from);
            assert_eq!(PosePair::new(from, to).sample(1.), to);
            for alpha in [0.25, 0.5, 0.75] {
                let pose = PosePair::new(from, to).sample(alpha).global();
                assert!(pose.affine().is_finite());
                assert!(
                    pose.translation()
                        .abs_diff_eq(Vec3::new(8., 2., -4.) * alpha, 0.0001)
                );
            }
        }
    }
}
