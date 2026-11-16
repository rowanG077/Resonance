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

    pub fn sample(&self, alpha: f32) -> Matrix {
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
        let pairs: Vec<_> = from
            .joints
            .iter()
            .zip(&to.joints)
            .enumerate()
            .map(|(i, (a, b))| {
                if a.parent == b.parent {
                    parents.push(a.parent);
                    PosePair::new(a.local.into(), b.local.into())
                } else {
                    // Different parent frames can only be compared in model space.
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
    pub space: PoseSpace,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum PoseSpace {
    Local,
    World,
    /// Apply scripted world-axis displacements after composing the pose.
    LocalOffset([Vec3; 2]),
    WorldOffset([Vec3; 2]),
    /// Recover a local pose after sampling the complete authored hierarchy.
    Guide(Entity),
    Dynamic(Entity),
    Pinned([Vec3; 2]),
}

impl PoseSpace {
    fn is_overlay(self) -> bool {
        matches!(self, Self::Guide(_) | Self::Dynamic(_) | Self::Pinned(_))
    }
}

impl PoseEndpoint {
    pub fn apply(self, alpha: f32, nodes: &mut Query<&mut Transform>, affine: &mut Locals) {
        assert!(!self.space.is_overlay());
        if let Ok(mut transform) = nodes.get_mut(self.entity) {
            let pose = self.poses.sample(alpha);
            affine.replace_for_render(self.entity, &mut transform, pose);
            if let PoseSpace::LocalOffset([a, b]) | PoseSpace::WorldOffset([a, b]) = self.space {
                affine.translate(self.entity, a.lerp(b, alpha.clamp(0., 1.)));
            }
            if matches!(self.space, PoseSpace::World | PoseSpace::WorldOffset(_)) {
                affine.set_raw_world(self.entity, pose.global());
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct PoseWorkspace {
    cache: EntityHashMap<GlobalTransform>,
    pins: EntityHashMap<Vec3>,
    worlds: Vec<(Entity, GlobalTransform)>,
    locals: Vec<(Entity, Pose, Option<GlobalTransform>)>,
}

/// Compose all authored locals before overlaying simulated worlds. A guide
/// keeps that composed pose, and free joints follow only the pin's displacement.
#[allow(clippy::type_complexity)]
pub(crate) fn apply_poses(
    endpoints: &[PoseEndpoint],
    alpha: f32,
    transforms: &mut ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
    work: &mut PoseWorkspace,
) {
    let (mut nodes, mut affine) = transforms.p1();
    let mut has_overlays = false;
    for &endpoint in endpoints {
        if endpoint.space.is_overlay() {
            has_overlays = true;
        } else {
            endpoint.apply(alpha, &mut nodes, &mut affine);
        }
    }
    if !has_overlays {
        return;
    }
    let helper = transforms.p0();
    let empty = BTreeMap::new();
    work.cache.clear();
    work.pins.clear();
    work.worlds.clear();
    work.locals.clear();
    for endpoint in endpoints {
        if let PoseSpace::Pinned([from, to]) = endpoint.space
            && let Ok(world) = helper.global_with(endpoint.entity, &empty, Some(&mut work.cache))
        {
            work.pins.insert(
                endpoint.entity,
                world.translation() - from.lerp(to, alpha.clamp(0., 1.)),
            );
        }
    }
    for endpoint in endpoints {
        let entity = endpoint.entity;
        let pin = match endpoint.space {
            PoseSpace::Guide(guide) => {
                if let Ok(world) = helper.global_with(guide, &empty, Some(&mut work.cache)) {
                    work.worlds.push((entity, world));
                }
                continue;
            }
            PoseSpace::Dynamic(pin) => pin,
            PoseSpace::Pinned(_) => entity,
            _ => continue,
        };
        let mut world = endpoint.poses.sample(alpha).global().affine();
        world.translation +=
            bevy::math::Vec3A::from(work.pins.get(&pin).copied().unwrap_or_default());
        work.worlds.push((entity, world.into()));
    }
    work.cache.clear();
    for &(entity, world) in &work.worlds {
        helper.cache_world(&mut work.cache, entity, world);
    }
    for &(entity, _) in &work.worlds {
        // The cache excludes each joint's world-axis offset. Recover the local
        // pose in that same space so propagation adds the sampled offset once.
        let world = work.cache[&entity];
        let parent = helper
            .parent(entity)
            .and_then(|p| helper.raw_global(p, Some(&mut work.cache), |_| None).ok())
            .and_then(inverse);
        work.locals.push((
            entity,
            Pose::Affine(parent.map_or(world.affine(), |p| p * world.affine())),
            parent.is_none().then_some(world),
        ));
    }
    let (mut nodes, mut affine) = transforms.p1();
    for &(entity, pose, world) in &work.locals {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            affine.set(entity, &mut transform, pose);
            if let Some(world) = world {
                affine.set_raw_world(entity, world);
            } else {
                affine.worlds.remove(&entity);
            }
        }
    }
}

fn inverse(pose: GlobalTransform) -> Option<Affine3A> {
    let matrix = pose.affine();
    (matrix.matrix3.determinant() != 0.)
        .then(|| matrix.inverse())
        .filter(|matrix| matrix.is_finite())
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
            let mut rb = rb.unwrap_or_else(|| collapsed_rotation(b, ra));
            if (a.determinant() < 0.) != (b.determinant() < 0.) {
                // QR can assign a changed scale sign to a different axis at each
                // end. Flipping two basis axes leaves a proper rotation. Pick the
                // closest equivalent basis and let the residual carry the scale.
                let basis = rb;
                for half_turn in [Vec4::X, Vec4::Y, Vec4::Z] {
                    let candidate = basis * Quat::from_vec4(half_turn);
                    if ra.dot(candidate).abs() > ra.dot(rb).abs() {
                        rb = candidate;
                    }
                }
            }
            Some([
                (ra, bevy::math::Mat3A::from_quat(ra.inverse()) * a),
                (rb, bevy::math::Mat3A::from_quat(rb.inverse()) * b),
            ])
        };
        Self { from, to, basis }
    }

    pub fn sample(&self, weight: f32) -> Pose {
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

/// Accessories inherit only rotation. As in TRS decomposition, a reflection
/// belongs to the X scale. At a zero scale use the remaining observable axes.
pub(crate) fn unscaled_rotation(mut matrix: bevy::math::Mat3A) -> Quat {
    if matrix.determinant() < 0. {
        matrix.x_axis = -matrix.x_axis;
    }
    orthogonal_basis(matrix).unwrap_or_else(|| collapsed_rotation(matrix, Quat::IDENTITY))
}

impl Pose {
    pub fn with_scale(self, scale: Vec3) -> Self {
        match self {
            Self::Trs(value) => value.with_scale(scale).into(),
            Self::Affine(mut value) => {
                let matrix = &mut value.matrix3;
                for (axis, length) in [&mut matrix.x_axis, &mut matrix.y_axis, &mut matrix.z_axis]
                    .into_iter()
                    .zip(scale.to_array())
                {
                    *axis = axis.normalize_or_zero() * length;
                }
                Self::Affine(value)
            }
        }
    }

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
    authored: EntityHashMap<Pose>,
    poses: EntityHashMap<(Affine3A, Transform)>,
    worlds: EntityHashMap<GlobalTransform>,
    translations: EntityHashMap<Vec3>,
}

impl Locals {
    pub fn remember_authored(&mut self, entity: Entity, transform: Transform) {
        let pose = self.get(entity, transform);
        self.authored.insert(entity, pose);
    }

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
        let mut matrix = pose.affine();
        matrix.translation -=
            bevy::math::Vec3A::from(self.translations.get(&entity).copied().unwrap_or_default());
        self.set_raw_world(entity, matrix.into());
    }

    fn set_raw_world(&mut self, entity: Entity, pose: GlobalTransform) {
        assert!(pose.affine().is_finite(), "nonfinite world pose");
        self.worlds.insert(entity, pose);
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
        let pose = self.get(entity, *transform).with_scale(scale);
        self.set(entity, transform, pose);
    }
    pub fn translation_boundary(&mut self, entity: Entity) {
        self.translations.entry(entity).or_default();
    }

    pub fn translate(&mut self, entity: Entity, delta: Vec3) {
        *self.translations.entry(entity).or_default() += delta;
    }

    fn clear(&mut self, mut changed: impl FnMut(Entity)) {
        for &entity in self
            .poses
            .keys()
            .chain(self.worlds.keys())
            .chain(self.translations.keys())
        {
            changed(entity);
        }
        self.authored.clear();
        self.poses.clear();
        self.worlds.clear();
        self.translations.clear();
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
    /// Resolve endpoints once. Driven joints interpolate in world space, with
    /// only their root's translation constrained by the animated attachment.
    pub fn pose_endpoints<'a>(
        &'a self,
        entities: impl Iterator<Item = Entity> + 'a,
        locals: &'a BTreeMap<Entity, Pose>,
        adjustments: &'a Locals,
        dynamics: &'a BTreeMap<Entity, GlobalTransform>,
        driven: &'a BTreeMap<Entity, Entity>,
    ) -> impl Iterator<Item = Result<PoseEndpoint, bevy::ecs::query::QueryEntityError>> + 'a {
        let authored = self.affine.as_ref().map(|a| &a.authored);
        let mut current_cache = EntityHashMap::default();
        let mut future_cache = EntityHashMap::default();
        let mut solved_cache = EntityHashMap::default();
        entities
            .map(move |entity| {
                let current = authored.and_then(|a| a.get(&entity)).copied();
                let from = current.map_or_else(|| self.local(entity), Ok)?;
                let to = locals.get(&entity).copied().unwrap_or(from);
                let mut base = PoseEndpoint {
                    entity,
                    poses: PosePair::new(from, to),
                    space: PoseSpace::Local,
                };
                let current_world = |node, cache: &mut EntityHashMap<_>| {
                    self.global_from(node, Some(cache), |e| {
                        authored.and_then(|map| map.get(&e)).copied()
                    })
                };
                let future_world = |node, cache: &mut EntityHashMap<_>| {
                    self.predicted_global(node, locals, adjustments, &BTreeMap::new(), Some(cache))
                };
                if current.is_none()
                    && let Some(a) = self.world_override(entity)
                {
                    let b = if let Some(world) = dynamics.get(&entity) {
                        let mut raw = world.affine();
                        raw.translation -= bevy::math::Vec3A::from(
                            self.offset(entity, Some(&adjustments.translations)),
                        );
                        raw
                    } else {
                        future_world(entity, &mut future_cache)?;
                        future_cache[&entity].affine()
                    };
                    let parents = self.parent(entity).and_then(|p| {
                        current_world(p, &mut current_cache).ok()?;
                        future_world(p, &mut future_cache).ok()?;
                        Some((inverse(current_cache[&p])?, inverse(future_cache[&p])?))
                    });
                    let (a, b) = if let Some((pa, pb)) = parents {
                        (pa * a.affine(), pb * b)
                    } else {
                        base.space = PoseSpace::World;
                        (a.affine(), b)
                    };
                    base.poses = PosePair::new(Pose::Affine(a), Pose::Affine(b));
                }
                if self.has_world_translation(entity) || adjustments.has_translation(entity) {
                    // An absent boundary inherits the nearest ancestor's offset.
                    let offsets = [
                        self.offset(entity, self.affine.as_ref().map(|a| &a.translations)),
                        self.offset(entity, Some(&adjustments.translations)),
                    ];
                    base.space = if base.space == PoseSpace::World {
                        PoseSpace::WorldOffset(offsets)
                    } else {
                        PoseSpace::LocalOffset(offsets)
                    };
                }
                let overlay = if let Some(&to) = dynamics.get(&entity) {
                    let space = match driven.get(&entity).copied() {
                        Some(pin) if pin == entity => PoseSpace::Pinned([
                            current_world(entity, &mut current_cache)?.translation(),
                            future_world(entity, &mut future_cache)?.translation(),
                        ]),
                        Some(pin) => PoseSpace::Dynamic(pin),
                        None => PoseSpace::Guide(entity),
                    };
                    let from =
                        self.global_with(entity, &BTreeMap::new(), Some(&mut solved_cache))?;
                    Some(PoseEndpoint {
                        entity,
                        space,
                        poses: PosePair::new(
                            Pose::Affine(from.affine()),
                            Pose::Affine(to.affine()),
                        ),
                    })
                } else {
                    None
                };
                Ok([Some(base), overlay])
            })
            .flat_map(|result| {
                match result {
                    Ok([base, overlay]) => [base.map(Ok), overlay.map(Ok)],
                    Err(error) => [Some(Err(error)), None],
                }
                .into_iter()
                .flatten()
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
        cache: Option<&mut EntityHashMap<GlobalTransform>>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let pose = self.resolve(entity, cache, &|node| {
            if let Some(pose) = overrides.get(&node) {
                // World overrides already include their own authored offset.
                let mut matrix = pose.affine();
                matrix.translation -=
                    bevy::math::Vec3A::from(self.offset(node, Some(&adjustments.translations)));
                Ok((matrix.into(), true))
            } else {
                let local = locals
                    .get(&node)
                    .copied()
                    .map_or_else(|| self.local(node), Ok)?;
                Ok((local.global(), false))
            }
        })?;
        Ok(self.translated(entity, pose, Some(&adjustments.translations)))
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

    pub fn has_world_translation(&self, entity: Entity) -> bool {
        self.affine
            .as_ref()
            .is_some_and(|affine| affine.translations.contains_key(&entity))
    }

    fn offset(&self, entity: Entity, translations: Option<&EntityHashMap<Vec3>>) -> Vec3 {
        let Some(translations) = translations.filter(|values| !values.is_empty()) else {
            return Vec3::ZERO;
        };
        std::iter::once(entity)
            .chain(self.parents.iter_ancestors(entity))
            .find_map(|entity| translations.get(&entity).copied())
            .unwrap_or_default()
    }

    fn translated(
        &self,
        entity: Entity,
        pose: GlobalTransform,
        translations: Option<&EntityHashMap<Vec3>>,
    ) -> GlobalTransform {
        let mut matrix = pose.affine();
        matrix.translation += bevy::math::Vec3A::from(self.offset(entity, translations));
        matrix.into()
    }

    /// The shared hierarchy cache stores matrices before inherited world offsets.
    pub fn cache_world(
        &self,
        cache: &mut EntityHashMap<GlobalTransform>,
        entity: Entity,
        world: GlobalTransform,
    ) {
        let offset = self.offset(entity, self.affine.as_ref().map(|a| &a.translations));
        let mut matrix = world.affine();
        matrix.translation -= bevy::math::Vec3A::from(offset);
        cache.insert(entity, matrix.into());
    }

    pub fn compute_global_transform(
        &self,
        entity: Entity,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        self.global_with(entity, &BTreeMap::new(), None)
    }

    /// Resolve a batch against this unchanged pose, sharing ancestor products.
    /// The cache is local to the batch: bone adjustments between systems must
    /// always be visible to subsequent attachment and outline queries.
    pub fn compute_global_transforms(
        &self,
        entities: impl IntoIterator<Item = Entity>,
    ) -> Result<Vec<GlobalTransform>, bevy::ecs::query::QueryEntityError> {
        let mut cache = EntityHashMap::default();
        let locals = BTreeMap::new();
        entities
            .into_iter()
            .map(|entity| self.global_with(entity, &locals, Some(&mut cache)))
            .collect()
    }

    /// Resolve either pose source with the same hierarchy walk. Cached values
    /// exclude translation boundaries: children can replace their parent's offset.
    fn resolve(
        &self,
        entity: Entity,
        mut cache: Option<&mut EntityHashMap<GlobalTransform>>,
        source: &impl Fn(Entity) -> Result<(GlobalTransform, bool), bevy::ecs::query::QueryEntityError>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        if let Some(&pose) = cache.as_deref().and_then(|cache| cache.get(&entity)) {
            return Ok(pose);
        }
        let (mut pose, world) = source(entity)?;
        if !world && let Some(parent) = self.parent(entity) {
            pose = self.resolve(parent, cache.as_deref_mut(), source)? * pose;
        }
        if let Some(cache) = cache {
            cache.insert(entity, pose);
        }
        Ok(pose)
    }

    /// Parent overrides let attached actors resolve in dependency order.
    pub fn global_with(
        &self,
        entity: Entity,
        locals: &BTreeMap<Entity, Pose>,
        cache: Option<&mut EntityHashMap<GlobalTransform>>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        self.global_from(entity, cache, |node| locals.get(&node).copied())
    }

    fn global_from(
        &self,
        entity: Entity,
        cache: Option<&mut EntityHashMap<GlobalTransform>>,
        local: impl Fn(Entity) -> Option<Pose>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        let pose = self.raw_global(entity, cache, local)?;
        Ok(self.translated(entity, pose, self.affine.as_ref().map(|a| &a.translations)))
    }

    fn raw_global(
        &self,
        entity: Entity,
        cache: Option<&mut EntityHashMap<GlobalTransform>>,
        local: impl Fn(Entity) -> Option<Pose>,
    ) -> Result<GlobalTransform, bevy::ecs::query::QueryEntityError> {
        self.resolve(entity, cache, &|node| {
            if let Some(local) = local(node) {
                Ok((local.global(), false))
            } else if let Some(pose) = self.world_override(node) {
                Ok((pose, true))
            } else {
                Ok((self.local(node)?.global(), false))
            }
        })
    }
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<Locals>()
        .add_systems(
            PreUpdate,
            // Keep the outgoing pose until the battle transition hides the field.
            clear.run_if(crate::battle::field_presenting.or_else(field_camera_hidden)),
        )
        .add_systems(
            PostUpdate,
            propagate
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CalculateBounds)
                .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
}

fn field_camera_hidden(cameras: Query<&Camera, With<crate::FieldCamera>>) -> bool {
    !cameras.iter().any(|camera| camera.is_active)
}

fn clear(mut affine: ResMut<Locals>, mut transforms: Query<&mut Transform>) {
    affine.clear(|entity| {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.set_changed();
        }
    });
}

/// Battle disposal can resume field publication after this frame's PreUpdate.
pub(crate) fn reset(world: &mut World) {
    if !world.contains_resource::<Locals>() {
        return;
    }
    world.resource_scope(|world, mut affine: Mut<Locals>| {
        affine.clear(|entity| {
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.set_changed();
            }
        });
    });
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
            .global_with(entity, &BTreeMap::new(), Some(&mut cache))
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
    fn battle_holds_the_outgoing_pose_until_the_field_camera_is_hidden() {
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin)
            .init_resource::<crate::loading::Resident>();
        install(&mut app);
        let camera = app
            .world_mut()
            .spawn((crate::FieldCamera, Camera::default()))
            .id();
        let bone = app.world_mut().spawn(Transform::IDENTITY).id();
        app.update();

        let last_pose = GlobalTransform::from_translation(Vec3::Y);
        app.world_mut()
            .resource_mut::<Locals>()
            .set_world(bone, last_pose);
        app.world_mut().run_schedule(PostUpdate);
        app.world()
            .resource::<crate::loading::Resident>()
            .battle
            .store(true, std::sync::atomic::Ordering::Release);
        app.update();
        assert_eq!(
            *app.world().get::<GlobalTransform>(bone).unwrap(),
            last_pose
        );

        app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
        app.update();
        assert_eq!(
            *app.world().get::<GlobalTransform>(bone).unwrap(),
            GlobalTransform::IDENTITY
        );
    }

    #[test]
    fn reset_restores_all_overrides_before_same_frame_actor_retirement() {
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        install(&mut app);
        let matrix_bone = app.world_mut().spawn(Transform::IDENTITY).id();
        let world_bone = app
            .world_mut()
            .spawn(Transform::from_scale(Vec3::ZERO))
            .id();
        let translated_bone = app.world_mut().spawn(Transform::IDENTITY).id();
        app.update();
        let matrix = bevy::math::Affine3A::from_mat4(Mat4::from_cols(
            Vec4::X,
            Vec4::new(0.5, 1., 0., 0.),
            Vec4::Z,
            Vec4::new(1., 2., 3., 1.),
        ));
        let held_world = GlobalTransform::from_translation(Vec3::new(10., 20., 30.));
        let translation = Vec3::new(40., 50., 60.);
        app.world_mut()
            .resource_scope(|world, mut locals: Mut<Locals>| {
                locals.set(
                    matrix_bone,
                    &mut world.get_mut::<Transform>(matrix_bone).unwrap(),
                    Pose::Affine(matrix),
                );
                locals.set_world(world_bone, held_world);
                locals.translate(translated_bone, translation);
            });
        app.world_mut().run_schedule(PostUpdate);
        for (bone, expected) in [
            (matrix_bone, matrix),
            (world_bone, held_world.affine()),
            (
                translated_bone,
                Transform::from_translation(translation).compute_affine(),
            ),
        ] {
            assert!(
                app.world()
                    .get::<GlobalTransform>(bone)
                    .unwrap()
                    .affine()
                    .abs_diff_eq(expected, 0.0001)
            );
        }
        reset(app.world_mut());
        // Field return can retire actors before the next pose propagation.
        app.world_mut().despawn(world_bone);
        // Disposal resumes this frame without another PreUpdate clear.
        app.world_mut().run_schedule(PostUpdate);
        for bone in [matrix_bone, translated_bone] {
            assert_eq!(
                *app.world().get::<GlobalTransform>(bone).unwrap(),
                GlobalTransform::IDENTITY
            );
        }
        reset(&mut World::new());
    }

    #[test]
    fn turning_actor_moves_cloth_only_by_its_pin_and_keeps_guides_authored() {
        use bevy::ecs::system::SystemState;
        let actor = |t: f32| {
            Transform::from_xyz(t * 20., 5., 0.).with_rotation(Quat::from_rotation_y(2.5 * t))
        };
        let cloth = |t: f32, position| {
            GlobalTransform::from(
                Transform::from_translation(position).with_rotation(Quat::from_rotation_z(0.7 * t)),
            )
        };
        let mut world = World::new();
        world.init_resource::<Locals>();
        let root = world.spawn(actor(0.)).id();
        let mut child = |parent, offset| {
            world
                .spawn((Transform::from_translation(offset), ChildOf(parent)))
                .id()
        };
        let pin = child(root, Vec3::X * 5.);
        let spacer = child(pin, Vec3::Z * 3.);
        let free = child(spacer, Vec3::Z * 7.);
        let nested_pin = child(free, Vec3::Z * 10.);
        let guide = child(nested_pin, Vec3::Z * 10.);
        let joints = [pin, free, nested_pin, guide];
        let pin_at = |t| actor(t).transform_point(Vec3::X * 5.);
        let free_at = Vec3::new(5., 5., 10.);
        let bend = |t: f32| Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 * t);
        let expected = |t| {
            [
                cloth(t, pin_at(t)),
                cloth(t, crate::test_support::inertial_joint(free_at, pin_at, t)),
                cloth(
                    t,
                    actor(t).transform_point(Vec3::X * 5. + bend(t) * Vec3::Z * 20.),
                ),
                GlobalTransform::from(actor(t))
                    * Transform::from_translation(Vec3::X * 5.).with_rotation(bend(t))
                    * Transform::from_xyz(0., 0., 30.),
            ]
        };
        for (joint, pose) in joints.into_iter().zip(expected(0.)) {
            let local = *world.get::<Transform>(joint).unwrap();
            world
                .resource_mut::<Locals>()
                .remember_authored(joint, local);
            world.resource_mut::<Locals>().set_world(joint, pose);
        }
        let mut helper = SystemState::<Helper>::new(&mut world);
        let locals = [
            (root, actor(1.).into()),
            (
                pin,
                Transform::from_translation(Vec3::X * 5.)
                    .with_rotation(bend(1.))
                    .into(),
            ),
        ]
        .into();
        let dynamics = joints.into_iter().zip(expected(1.)).collect();
        let driven = [(pin, pin), (free, pin), (nested_pin, nested_pin)].into();
        let endpoints = helper
            .get(&world)
            .unwrap()
            .pose_endpoints(
                [root, pin, free, nested_pin, guide].into_iter(),
                &locals,
                &Locals::default(),
                &dynamics,
                &driven,
            )
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut params =
            SystemState::<ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>>::new(
                &mut world,
            );
        let attachment =
            Transform::from_xyz(100., -30., 50.).with_rotation(Quat::from_rotation_z(0.7));
        for t in [0., 0.5, 1., 0.25] {
            apply_poses(
                &endpoints,
                t,
                &mut params.get_mut(&mut world).unwrap(),
                &mut PoseWorkspace::default(),
            );
            // Attach the sampled actor just as the field does, then reuse the same endpoints.
            *world.get_mut::<Transform>(root).unwrap() =
                attachment * *world.get::<Transform>(root).unwrap();
            let actual = helper
                .get(&world)
                .unwrap()
                .compute_global_transforms(joints)
                .unwrap();
            for ((joint, actual), expected) in joints.into_iter().zip(actual).zip(expected(t)) {
                let expected = attachment.compute_affine() * expected.affine();
                assert!(
                    actual.affine().abs_diff_eq(expected, 0.001),
                    "joint {joint:?} at {t}"
                );
            }
        }
    }

    #[test]
    fn pose_endpoints_keep_dynamic_children_attached_and_handle_collapsed_parents() {
        use bevy::ecs::system::SystemState;
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
            let next = Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                .with_scale(scale);
            let locals = [(root, next.into())].into();
            let dynamics = [(child, GlobalTransform::from_translation(Vec3::Y * 50.))].into();
            let mut helper = SystemState::<Helper>::new(&mut world);
            let endpoints = helper
                .get(&world)
                .unwrap()
                .pose_endpoints(
                    [root, child].into_iter(),
                    &locals,
                    &Locals::default(),
                    &dynamics,
                    &BTreeMap::new(),
                )
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let mut transforms = SystemState::<
                ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
            >::new(&mut world);
            assert_eq!(endpoints[1].space == PoseSpace::World, collapsed);
            // Reusing endpoints must not feed a previous rendered pose back into prediction.
            for alpha in [0., 0.25, 0.5, 0.75, 0.999] {
                apply_poses(
                    &endpoints,
                    alpha,
                    &mut transforms.get_mut(&mut world).unwrap(),
                    &mut PoseWorkspace::default(),
                );
                let position = helper
                    .get(&world)
                    .unwrap()
                    .compute_global_transform(child)
                    .unwrap()
                    .translation();
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
        use bevy::ecs::system::SystemState;
        for scale in [Vec3::ONE, Vec3::new(0., 1., 1.)] {
            let mut app = App::new();
            app.add_plugins(bevy::transform::TransformPlugin);
            install(&mut app);
            let world = app.world_mut();
            let root = world.spawn(Transform::from_scale(scale)).id();
            let mut spawn = |parent, z| {
                world
                    .spawn((Transform::from_xyz(0., 0., z), ChildOf(parent)))
                    .id()
            };
            let platform = spawn(root, 0.);
            let effect = spawn(platform, 20.);
            let child = spawn(effect, 30.);
            let mesh = spawn(effect, 0.);
            // The platform stays 100 units along world X while the parent turns.
            // The effect releases its offset and its child gains a zero boundary.
            let platform_offset = Vec3::X * 100.;
            let effect_offset = Vec3::Y * 40.;
            let mut current = world.resource_mut::<Locals>();
            current.translate(platform, platform_offset);
            current.translate(effect, effect_offset);
            let mut next_offsets = Locals::default();
            next_offsets.translate(platform, platform_offset);
            next_offsets.translation_boundary(child);
            let turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            let next = [
                (
                    root,
                    Transform::from_rotation(turn).with_scale(scale).into(),
                ),
                (platform, Transform::from_xyz(10., 0., 0.).into()),
            ]
            .into();
            let mut helper = SystemState::<Helper>::new(world);
            let authored = helper.get(world).unwrap();
            let guide = authored
                .predicted_global(effect, &next, &next_offsets, &BTreeMap::new(), None)
                .unwrap();
            let endpoints = authored
                .pose_endpoints(
                    [root, platform, effect, child].into_iter(),
                    &next,
                    &next_offsets,
                    &[(effect, guide)].into(),
                    &BTreeMap::new(),
                )
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let mut transforms = SystemState::<
                ParamSet<(Helper, (Query<&mut Transform>, ResMut<Locals>))>,
            >::new(world);
            let mut workspace = PoseWorkspace::default();
            for t in [0., 0.5, 1., 0.] {
                apply_poses(
                    &endpoints,
                    t,
                    &mut transforms.get_mut(world).unwrap(),
                    &mut workspace,
                );
                world.run_schedule(PostUpdate);
                let movement = Quat::IDENTITY.slerp(turn, t) * (scale * Vec3::X * (10. * t));
                let inherited = effect_offset.lerp(platform_offset, t) + Vec3::Z * 20.;
                let entities = [platform, effect, mesh, child];
                let offsets = [
                    platform_offset,
                    inherited,
                    inherited,
                    effect_offset * (1. - t) + Vec3::Z * 50.,
                ];
                let computed = helper
                    .get(world)
                    .unwrap()
                    .compute_global_transforms(entities)
                    .unwrap();
                for ((entity, pose), offset) in entities.into_iter().zip(computed).zip(offsets) {
                    assert_eq!(*world.get::<GlobalTransform>(entity).unwrap(), pose);
                    assert!(
                        pose.translation().abs_diff_eq(movement + offset, 0.001),
                        "{} at {t}",
                        pose.translation()
                    );
                }
            }
            let driven = GlobalTransform::from_translation(Vec3::new(10., 20., 900.));
            world.resource_mut::<Locals>().set_world(effect, driven);
            assert_eq!(
                helper
                    .get(world)
                    .unwrap()
                    .compute_global_transforms([mesh, effect, mesh])
                    .unwrap(),
                [driven; 3]
            );
            // Releasing the controllers restores ordinary propagation.
            app.update();
            assert_eq!(
                app.world()
                    .get::<GlobalTransform>(effect)
                    .unwrap()
                    .translation(),
                Vec3::Z * 20.
            );
        }
    }

    #[test]
    #[ignore = "requires locally cooked Thoda scenery; no devices"]
    fn thoda_platform_and_teleporter_bones_rise_by_the_same_amount() -> anyhow::Result<()> {
        use resonance_content::{animation::Skeleton, field::document, library::Library};
        let root = std::path::PathBuf::from(std::env::var_os("RESONANCE_TEST_ASSETS").unwrap());
        let library = Library::open(&root, &[])?;
        let (_, field) = document::capture_scene(&mut library.snapshot(), 9, || false)?;
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
                    helper.global_with(child, &BTreeMap::new(), None).unwrap(),
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
        use resonance_content::animation::{Channel, Interpolation, NodeTrack, Track};
        let motion = Motion {
            duration_frames: 2.0,
            tracks: vec![
                Track {
                    bone: 0,
                    nodes: vec![
                        std::sync::Arc::new(NodeTrack {
                            bind: Default::default(),
                            translation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0, 0.03333333333333333],
                                values: vec![[3.0, 4.0, 5.0], [6.0, 7.0, 8.0]],
                            }),
                            rotation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0, 0.03333333333333333],
                                values: vec![
                                    [
                                        0.6686189942740389,
                                        0.23010571591330087,
                                        0.23010571591330087,
                                        0.6686189942740389,
                                    ],
                                    [
                                        0.6686189942740389,
                                        -0.23010571591330087,
                                        -0.23010571591330087,
                                        0.6686189942740389,
                                    ],
                                ],
                            }),
                            scale: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0, 0.03333333333333333],
                                values: vec![
                                    [1.2807764064044151, 1.0, 0.7807764064044151],
                                    [1.2807764064044151, 1.0, 0.7807764064044151],
                                ],
                            }),
                        }),
                        std::sync::Arc::new(NodeTrack {
                            bind: Default::default(),
                            translation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0, 0.03333333333333333],
                                values: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
                            }),
                            rotation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0, 0.03333333333333333],
                                values: vec![
                                    [
                                        -0.6354943369933826,
                                        -0.3100757127692221,
                                        -0.3100757127692221,
                                        0.6354943369933828,
                                    ],
                                    [
                                        -0.6354943369933826,
                                        0.3100757127692221,
                                        0.3100757127692221,
                                        0.6354943369933828,
                                    ],
                                ],
                            }),
                            scale: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0, 0.03333333333333333],
                                values: vec![[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
                            }),
                        }),
                    ],
                },
                Track {
                    bone: 1,
                    nodes: vec![
                        std::sync::Arc::new(NodeTrack {
                            bind: Default::default(),
                            translation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0],
                                values: vec![[1.0, 2.0, 3.0]],
                            }),
                            rotation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0],
                                values: vec![[
                                    -0.2867902889042155,
                                    0.6463368550455998,
                                    0.6463368550455998,
                                    -0.28679028890421554,
                                ]],
                            }),
                            scale: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0],
                                values: vec![[1.104987562112089, 1.0, 0.9049875621120891]],
                            }),
                        }),
                        std::sync::Arc::new(NodeTrack {
                            bind: Default::default(),
                            translation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0],
                                values: vec![[0.0, 0.0, 0.0]],
                            }),
                            rotation: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0],
                                values: vec![[
                                    -0.2542378150709102,
                                    0.6598205311961501,
                                    0.65982053119615,
                                    0.2542378150709102,
                                ]],
                            }),
                            scale: Some(Channel {
                                interpolation: Interpolation::Step,
                                times: vec![0.0],
                                values: vec![[1.0, 1.0, 1.0]],
                            }),
                        }),
                    ],
                },
            ],
        };
        let mut skeleton = resonance_content::animation::Skeleton::single("parent");
        skeleton.bones.push(resonance_content::animation::Bone {
            name: "child".into(),
            parent: Some(0),
            parent_transform: None,
            shadow_bounds: true,
            bind: resonance_content::animation::Transform::default(),
        });
        let motion = Motion::decode(&motion.encode(&skeleton).unwrap()).unwrap();
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
                    shadow_bounds: true,
                    parent_transform: None,
                    bind: Trs::default(),
                },
                Bone {
                    name: "parent".into(),
                    parent: None,
                    shadow_bounds: true,
                    parent_transform: None,
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
                let turn = 120_f32.to_radians();
                let end = Quat::from_rotation_z(turn) * start;
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
                    // Gram matrices and determinants alone accept any rotation,
                    // including a pose frozen at its first endpoint. Two
                    // independent columns distinguish the turn from scale changes.
                    if scale.y != 0. && scale.z != 0. {
                        let expected =
                            Mat3::from_quat(Quat::from_rotation_z(turn * alpha) * start) * basis;
                        assert!(
                            actual.abs_diff_eq(expected, 0.0001),
                            "rotation: scale={scale:?}, shear={shear}, alpha={alpha}"
                        );
                    }
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
                                .predicted_global(
                                    entity,
                                    &locals,
                                    &adjustments,
                                    &overrides,
                                    Some(&mut cache)
                                )
                                .unwrap(),
                            helper
                                .predicted_global(entity, &locals, &adjustments, &overrides, None)
                                .unwrap(),
                        );
                    }
                }
                assert_eq!(
                    helper
                        .predicted_global(parent, &locals, &adjustments, &overrides, None)
                        .unwrap()
                        .translation()
                        .z,
                    900.
                );
                assert_eq!(
                    helper
                        .predicted_global(child, &locals, &adjustments, &overrides, None)
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
