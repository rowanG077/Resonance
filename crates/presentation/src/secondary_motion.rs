//! Fixed-tick secondary bone motion, applied after skeletal animation.
//! The inverted outline hull consumes the primary skeleton's result.
use super::field_view::{ActorPart, Art, State};
use super::sparse_animation::affine::{Helper as TransformHelper, Locals, Pose, rotation};
use bevy::prelude::*;
use resonance_content::secondary_motion::Chain;
use std::collections::BTreeMap;

const SETTLE_UPDATES: u32 = 300;

#[derive(Component)]
pub(super) struct Rig {
    root: Entity,
    bones: BTreeMap<u16, Bone>,
    chains: Vec<(Chain, Simulation)>,
    tick: Option<u32>,
    pose: BTreeMap<u16, GlobalTransform>,
    impulses: BTreeMap<u32, Vec3>,
    disabled: bool,
}
struct Bone {
    entity: Entity,
    parent: Entity,
    authored: Transform,
}
struct Authored {
    world: GlobalTransform,
    affine: bool,
}
pub(super) enum Deformation {
    Local(Pose),
    World(GlobalTransform),
}
impl Deformation {
    pub(super) fn apply(self, entity: Entity, transform: &mut Transform, poses: &mut Locals) {
        match self {
            Self::Local(pose) => poses.set(entity, transform, pose),
            Self::World(pose) => poses.set_world(entity, pose),
        }
    }
}

impl Rig {
    pub(super) fn new(
        root: Entity,
        spec: &resonance_content::ScenePart,
        names: &BTreeMap<String, (Entity, Transform, Entity)>,
    ) -> Option<Self> {
        let bones: BTreeMap<_, _> = spec
            .bone_names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                let &(entity, authored, parent) = names.get(name)?;
                Some((
                    index as u16,
                    Bone {
                        entity,
                        authored,
                        parent,
                    },
                ))
            })
            .collect();
        let chains = spec
            .secondary_motion
            .prepare(&spec.bone_names)
            .unwrap_or_else(|error| panic!("secondary-motion preparation failed: {error:#}"));
        if chains.iter().any(|chain| {
            chain.joints.iter().any(|j| !bones.contains_key(&j.node))
                || chain
                    .collision_plane
                    .as_ref()
                    .is_some_and(|p| !bones.contains_key(&p.anchor))
        }) {
            return None;
        }
        Some(Self {
            root,
            bones,
            chains: chains
                .into_iter()
                .map(|chain| (chain, Simulation::default()))
                .collect(),
            tick: None,
            pose: BTreeMap::new(),
            impulses: BTreeMap::new(),
            disabled: false,
        })
    }

    pub(super) fn advance(
        &mut self,
        helper: &TransformHelper,
        yaw: f32,
        tick: u32,
        settle: bool,
        animated_roots: &[u16],
    ) -> Option<BTreeMap<u16, GlobalTransform>> {
        let Pose::Trs(actor) = helper.local(self.root).ok()? else {
            panic!("secondary-motion actor root must expose its scale")
        };
        let authored = self.authored(helper)?;
        let steps = if self.tick.is_none() && settle {
            SETTLE_UPDATES
        } else {
            self.tick.map_or(1, |previous| {
                tick.saturating_sub(previous)
                    .min(resonance_events::projectile::CHAIN_HISTORY_TICKS)
            })
        };
        let forces: Vec<_> = (0..steps)
            .map(|step| {
                tick.checked_sub(steps - step - 1)
                    .and_then(|tick| self.impulses.get(&tick))
                    .copied()
                    .unwrap_or_default()
            })
            .collect();
        let output = self.evaluate(&authored, actor.scale, yaw, animated_roots, &forces);
        self.tick = Some(tick);
        self.pose.clone_from(&output);
        Some(output)
    }

    fn authored(&self, helper: &TransformHelper) -> Option<BTreeMap<u16, Authored>> {
        self.bones
            .iter()
            .map(|(&node, bone)| {
                Some((
                    node,
                    Authored {
                        world: helper.compute_global_transform(bone.entity).ok()?,
                        affine: helper.has_affine(bone.entity),
                    },
                ))
            })
            .collect()
    }

    fn evaluate(
        &mut self,
        authored: &BTreeMap<u16, Authored>,
        actor_scale: Vec3,
        yaw: f32,
        animated_roots: &[u16],
        forces: &[Vec3],
    ) -> BTreeMap<u16, GlobalTransform> {
        let actor_rotation = Quat::from_rotation_z(yaw.to_radians());
        let mut output = BTreeMap::new();
        for (chain, simulation) in &mut self.chains {
            let targets: Vec<_> = chain
                .joints
                .iter()
                .map(|joint| authored[&joint.node].world.translation())
                .collect();
            if self.disabled {
                // Pin disabled joints to the animated pose.
                if simulation.positions.len() != targets.len() {
                    simulation.reset(&targets);
                }
                simulation.positions[1..].copy_from_slice(&targets[1..]);
                simulation.previous[1..].copy_from_slice(&targets[1..]);
                simulation.targets = targets;
                continue;
            }
            let plane = chain.collision_plane.as_ref().map(|p| {
                (
                    plane_normal(
                        authored[&p.anchor].world,
                        Vec3::from_array(p.normal),
                        authored[&p.anchor].affine || nonuniform_scale(actor_scale),
                    ),
                    p.offset,
                    p.strength,
                )
            });
            let attraction = if animated_roots.contains(&chain.joints[0].node) {
                0.2
            } else {
                chain.attraction
            };
            simulation.advance(chain, &targets, plane, attraction, forces);
            // Keep the terminal guide at its animated world pose.
            let tip = chain.joints.last().unwrap().node;
            output.entry(tip).or_insert(authored[&tip].world);
            for (index, joint) in chain.joints.iter().enumerate().take(chain.joints.len() - 1) {
                let mut pose = driven_pose(
                    authored[&joint.node].world,
                    actor_scale,
                    authored[&joint.node].affine,
                );
                pose.translation = simulation.positions[index];
                if !chain.preserve_rotation {
                    let angles = |direction: Vec3| {
                        let d = actor_rotation.inverse() * direction.normalize_or_zero();
                        Vec2::new((-d.y).clamp(-1., 1.).asin(), d.x.atan2(d.z))
                    };
                    let mut delta =
                        angles(simulation.positions[index] - simulation.positions[index + 1])
                            - angles(targets[index] - targets[index + 1]);
                    if chain.rotation_locks[0] {
                        delta.x = 0.;
                    }
                    if chain.rotation_locks[1] {
                        delta.y = 0.;
                    }
                    pose.rotation *= Quat::from_euler(EulerRot::ZYX, 0., delta.y, delta.x);
                }
                output.insert(joint.node, GlobalTransform::from(pose));
            }
        }

        output
    }

    pub(super) fn locals(
        &self,
        helper: &TransformHelper,
        pose: &BTreeMap<u16, GlobalTransform>,
    ) -> Vec<(Entity, Deformation)> {
        let mut locals = Vec::new();
        let Ok(Pose::Trs(actor)) = helper.local(self.root) else {
            panic!("secondary-motion actor root must expose its scale")
        };
        let entities: BTreeMap<_, _> = self
            .bones
            .iter()
            .map(|(&node, bone)| (bone.entity, node))
            .collect();
        for (&node, world) in pose {
            let Some(bone) = self.bones.get(&node) else {
                continue;
            };
            let parent = entities
                .get(&bone.parent)
                .and_then(|node| pose.get(node))
                .copied()
                .or_else(|| helper.compute_global_transform(bone.parent).ok());
            if let Some(parent) = parent {
                let terminal = self
                    .chains
                    .iter()
                    .any(|(chain, _)| chain.joints.last().is_some_and(|tip| tip.node == node));
                let pose = if terminal
                    || helper.has_world_translation(bone.entity)
                    || parent.affine().matrix3.determinant() == 0.
                    || world.affine().matrix3.determinant() == 0.
                {
                    Deformation::World(*world)
                } else {
                    Deformation::Local(local_pose(
                        *world,
                        parent,
                        helper.has_affine(bone.entity),
                        actor.scale,
                    ))
                };
                locals.push((bone.entity, pose));
            }
        }

        locals
    }
}

fn nonuniform_scale(scale: Vec3) -> bool {
    scale.x != scale.y || scale.y != scale.z
}

fn driven_pose(world: GlobalTransform, actor_scale: Vec3, affine: bool) -> Transform {
    if affine || nonuniform_scale(actor_scale) || world.affine().matrix3.determinant() == 0. {
        // Dynamics build a fresh world TRS from actor scale and the quaternion
        // extracted from the complete authored world matrix.
        Transform::from_translation(world.translation())
            .with_rotation(rotation(world.affine()))
            .with_scale(actor_scale)
    } else {
        world.compute_transform()
    }
}

fn local_pose(
    world: GlobalTransform,
    parent: GlobalTransform,
    affine: bool,
    actor_scale: Vec3,
) -> Pose {
    if affine || nonuniform_scale(actor_scale) {
        // Unequal actor scaling and a rotated bone produce local shear even
        // when every authored animation key uses TRS.
        let local = parent.affine().inverse() * world.affine();
        assert!(
            local.is_finite(),
            "secondary-motion parent must be invertible"
        );
        Pose::Affine(local)
    } else {
        world.reparented_to(&parent).into()
    }
}

fn plane_normal(world: GlobalTransform, normal: Vec3, affine: bool) -> Vec3 {
    if affine || world.affine().matrix3.determinant() == 0. {
        // Equivalent to transforming the plane's three points before their
        // cross product, including reflection and nonuniform scale.
        let tangent = normal.any_orthonormal_vector();
        let bitangent = normal.cross(tangent);
        world
            .affine()
            .transform_vector3(tangent)
            .cross(world.affine().transform_vector3(bitangent))
            .normalize_or_zero()
    } else {
        world.rotation() * normal
    }
}

/// Read-only solver evidence for silent replay checkpoints.
pub(super) fn diagnostic(world: &mut World) -> serde_json::Value {
    let mut query = world.query::<(&ActorPart, &Rig)>();
    serde_json::json!(query.iter(world).filter(|(part, _)| part.part == 0).map(|(part, rig)| {
        serde_json::json!({"actor":part.actor,"tick":rig.tick,
            "nodes":rig.bones.iter().filter_map(|(node,bone)|world.get::<GlobalTransform>(bone.entity).map(|pose|serde_json::json!({"node":node,"world":pose.to_matrix().to_cols_array()}))).collect::<Vec<_>>(),
            "chains":rig.chains.iter().map(|(chain, simulation)| {
            serde_json::json!({"root":chain.joints[0].node,"positions":simulation.positions.iter().map(|p|p.to_array()).collect::<Vec<_>>(),
                "targets":simulation.targets.iter().map(|p|p.to_array()).collect::<Vec<_>>(),
                "velocity":simulation.velocity.iter().map(|p|p.to_array()).collect::<Vec<_>>()})
        }).collect::<Vec<_>>()})
    }).collect::<Vec<_>>())
}

pub(super) fn bind(
    mut commands: Commands,
    art: Res<Art>,
    actors: Query<(Entity, &ActorPart), Without<Rig>>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform, &ChildOf)>,
    meshes: Query<(), With<Mesh3d>>,
) {
    for (root, actor) in &actors {
        let spec = &art.models[&actor.resource][actor.part].spec;
        if !actor.prepared || spec.secondary_motion.is_empty() {
            continue;
        }
        let names = super::field_pose::named_bones(root, &children, &nodes, &meshes);
        if let Some(rig) = Rig::new(root, spec, &names) {
            commands.entity(root).insert(rig);
        }
    }
}

/// Restore only the bones modified by dynamics. A clip may omit these tracks;
/// feeding yesterday's deformation back into the authored pose causes drift.
pub(super) fn restore(rigs: Query<&Rig>, mut transforms: Query<&mut Transform>) {
    for rig in &rigs {
        for (chain, _) in &rig.chains {
            for joint in chain.joints.iter().take(chain.joints.len() - 1) {
                let bone = &rig.bones[&joint.node];
                if let Ok(mut transform) = transforms.get_mut(bone.entity) {
                    *transform = bone.authored;
                }
            }
        }
    }
}

#[allow(clippy::type_complexity)] // Snapshot authored world poses before writing local bones.
pub(super) fn apply(
    state: State,
    art: Res<Art>,
    mut rigs: Query<(&ActorPart, &mut Rig)>,
    parts: Query<&ActorPart>,
    mut transforms: ParamSet<(TransformHelper, (Query<&mut Transform>, ResMut<Locals>))>,
    mut applied: ResMut<super::field_audit::Applied>,
) {
    let tick = state.get().events.tick();
    let mut poses: BTreeMap<i32, BTreeMap<u16, GlobalTransform>> = BTreeMap::new();
    {
        let helper = transforms.p0();
        for (part, mut rig) in &mut rigs {
            if part.part != 0 {
                continue;
            }
            let Some(actor) = state.get().events.world.actors.get(&part.actor) else {
                continue;
            };
            rig.disabled = actor.appearance.secondary_motion_disabled;
            // Offscreen actors keep their last model pose and dynamics. Move
            // the clock forward so returning onscreen does not catch up the
            // skipped simulation ticks; a new rig still evaluates once.
            let culled = actor.animation_culled && rig.tick.is_some();
            if culled {
                rig.tick = Some(tick);
            }
            let animated_roots = part
                .active_clip
                .and_then(|index| art.models[&part.resource][part.part].spec.clips.get(index))
                .map(|clip| clip.secondary_pose_nodes.as_slice())
                .unwrap_or_default();
            rig.impulses = actor
                .chain_impulses
                .iter()
                .map(|(&tick, impulse)| (tick, Vec3::from_array(impulse.acceleration)))
                .collect();
            let yaw = actor.appearance.fixed_heading.unwrap_or(actor.heading);
            let output = if culled {
                Some(rig.pose.clone())
            } else {
                rig.advance(
                    &helper,
                    yaw,
                    tick,
                    state.checkpoint.is_some(),
                    animated_roots,
                )
            };
            if let Some(output) = output {
                poses.insert(part.actor, output);
            }
        }
    }
    // Parent transforms must also use the deformed pose. Copying the same
    // world joints to the outline prevents a second, independently moving hull.
    let helper = transforms.p0();
    let mut locals = Vec::new();
    for (part, rig) in &rigs {
        let Some(pose) = poses.get(&part.actor) else {
            // Outline scenes can finish loading before their primary scene.
            // They consume that primary skeleton's dynamics, so propagate
            // its bounded loading dependency instead of treating it as lost.
            if parts.iter().any(|primary| {
                primary.actor == part.actor && primary.part == 0 && !primary.prepared
            }) {
                applied.loading(super::field_audit::Request::SecondaryMotion(
                    part.actor, part.part,
                ));
            }
            continue;
        };
        locals.extend(rig.locals(&helper, pose));
        applied.ack(super::field_audit::Request::SecondaryMotion(
            part.actor, part.part,
        ));
    }
    let (mut transforms, mut affine) = transforms.p1();
    for (entity, local) in locals {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            local.apply(entity, &mut transform, &mut affine);
        }
    }
}

#[derive(Default)]
struct Simulation {
    positions: Vec<Vec3>,
    previous: Vec<Vec3>,
    velocity: Vec<Vec3>,
    targets: Vec<Vec3>,
}
impl Simulation {
    fn reset(&mut self, targets: &[Vec3]) {
        self.positions = targets.to_vec();
        self.previous = targets.to_vec();
        self.velocity = vec![Vec3::ZERO; targets.len()];
        self.targets = targets.to_vec();
    }
    fn advance(
        &mut self,
        chain: &Chain,
        targets: &[Vec3],
        plane: Option<(Vec3, f32, f32)>,
        attraction: f32,
        forces: &[Vec3],
    ) {
        if self.positions.len() != targets.len() {
            self.reset(targets);
        }
        let previous_targets = std::mem::take(&mut self.targets);
        for (step, force) in forces.iter().enumerate() {
            // Catch-up ticks consume interpolated authored targets; rendering
            // additional frames without a field tick does not advance physics.
            let t = (step + 1) as f32 / forces.len() as f32;
            let targets: Vec<_> = previous_targets
                .iter()
                .zip(targets)
                .map(|(a, b)| a.lerp(*b, t))
                .collect();
            for (index, joint) in chain.joints.iter().enumerate() {
                let position = self.positions[index];
                self.positions[index] += (targets[index] - position) * attraction
                    + Vec3::new(0., 0., -0.98 * joint.gravity);
            }
            // Attraction can pull a large displacement within the chain's
            // recovery radius. Test afterwards, before applying momentum.
            if self.positions[0].distance(targets[0]) > 100. {
                self.reset(&targets);
            } else {
                for (position, velocity) in self.positions.iter_mut().zip(&mut self.velocity) {
                    *velocity += *force;
                    *position += *velocity;
                }
            }
            let lengths: Vec<_> = targets.windows(2).map(|p| p[0].distance(p[1])).collect();
            for _ in 0..10 {
                self.positions[0] = targets[0];
                self.previous[0] = targets[0];
                for (index, length) in lengths.iter().enumerate() {
                    let delta = self.positions[index + 1] - self.positions[index];
                    let distance = delta.length();
                    if distance > 1e-6 {
                        let correction = delta * (0.45 * (length - distance) / distance);
                        self.positions[index] -= correction;
                        self.positions[index + 1] += correction;
                    }
                }
            }
            if let Some((normal, offset, strength)) = plane {
                let root = self.positions[0];
                for position in self.positions.iter_mut().skip(1) {
                    let distance = (*position - root).dot(normal) - offset;
                    if distance < 0. {
                        *position -= normal * distance * strength;
                    }
                }
            }
            for (index, joint) in chain.joints.iter().enumerate() {
                self.velocity[index] =
                    (self.positions[index] - self.previous[index]) * joint.damping;
                self.previous[index] = self.positions[index];
            }
        }
        self.targets = targets.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Affine3A;
    use resonance_content::secondary_motion::Joint;

    #[test]
    fn stretched_actor_keeps_secondary_bone_scale_and_world_pose() {
        let actor_scale = Vec3::new(0.4, 0.4, 2.2);
        let parent = GlobalTransform::from(
            Transform::from_xyz(-2120., -150., 305.)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::PI))
                .with_scale(actor_scale),
        );
        let authored = parent.mul_transform(
            Transform::from_xyz(4., 5., 120.).with_rotation(Quat::from_rotation_x(0.6)),
        );
        let mut driven = driven_pose(authored, actor_scale, false);
        assert_eq!(driven.scale, actor_scale);
        driven.translation += Vec3::new(1., -2., 3.);
        driven.rotation *= Quat::from_rotation_y(0.2);
        let local = local_pose(driven.into(), parent, false, actor_scale);
        assert!(
            (parent * local.global())
                .affine()
                .abs_diff_eq(driven.compute_affine(), 0.0001)
        );
    }

    #[test]
    fn affine_dynamics_rebuild_world_trs_and_keep_exact_parent_inverse() {
        let parent = GlobalTransform::from(Affine3A::from_cols(
            Vec3::new(2., 0., 1.).into(),
            Vec3::new(0.5, -3., 0.).into(),
            Vec3::new(0., 0., 4.).into(),
            Vec3::splat(10.).into(),
        ));
        let authored = parent.mul_transform(Transform::from_xyz(4., 5., 6.));
        let actor_scale = Vec3::splat(2.);
        let mut driven = driven_pose(authored, actor_scale, true);
        assert_eq!(driven.scale, actor_scale);
        assert_eq!(driven.translation, authored.translation());
        driven.translation += Vec3::new(1., -2., 3.);
        driven.rotation *= Quat::from_rotation_x(0.4);
        let local = local_pose(driven.into(), parent, true, actor_scale);
        assert!(matches!(local, Pose::Affine(_)));
        let restored = parent * local.global();
        assert!(
            restored
                .affine()
                .abs_diff_eq(driven.compute_affine(), 0.00001)
        );
        assert!(
            plane_normal(parent, Vec3::Z, true)
                .abs_diff_eq(Vec3::new(3., 0.5, -6.).normalize(), 0.00001)
        );
        let attachment = Vec3::new(2., 3., 4.);
        assert!(
            restored
                .transform_point(attachment)
                .abs_diff_eq(driven.transform_point(attachment), 0.00001)
        );
    }

    #[test]
    fn chain_impulses_preserve_their_order_during_render_catchup() {
        let chain = Chain {
            joints: (0..3)
                .map(|node| Joint {
                    node,
                    gravity: 0.,
                    damping: 0.8,
                })
                .collect(),
            attraction: 0.,
            preserve_rotation: false,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        let targets = [Vec3::ZERO, Vec3::Z * 10., Vec3::Z * 20.];
        let forces = [Vec3::X, Vec3::Y * 2., Vec3::ZERO];
        let mut per_tick = Simulation::default();
        let mut caught_up = Simulation::default();
        for force in forces {
            per_tick.advance(&chain, &targets, None, 0., &[force]);
        }
        caught_up.advance(&chain, &targets, None, 0., &forces);
        assert_eq!(caught_up.positions, per_tick.positions);
        assert!(caught_up.positions[0].distance(targets[0]) < 0.1);
        assert!(caught_up.positions[2].x > 1. && caught_up.positions[2].y > 2.);
    }

    #[test]
    fn chain_settles_without_render_rate_drift_and_resets_after_teleport() {
        let chain = Chain {
            joints: (0..4)
                .map(|node| Joint {
                    node,
                    gravity: 0.333,
                    damping: 0.766,
                })
                .collect(),
            attraction: 0.03,
            preserve_rotation: false,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        let targets: Vec<_> = (0..4).map(|n| Vec3::new(n as f32 * 8., 0., 40.)).collect();
        let mut simulation = Simulation::default();
        simulation.advance(
            &chain,
            &targets,
            Some((Vec3::NEG_Y, 0., 1.)),
            chain.attraction,
            &[Vec3::ZERO; 300],
        );
        assert!(simulation.positions[3].z < targets[3].z - 5.);
        assert!(
            simulation
                .positions
                .iter()
                .all(|p| p.is_finite() && p.y <= 0.0001)
        );
        let saved = simulation.positions.clone();
        for _ in 0..100 {
            simulation.advance(
                &chain,
                &targets,
                Some((Vec3::NEG_Y, 0., 1.)),
                chain.attraction,
                &[],
            );
        }
        assert_eq!(simulation.positions, saved);
        let pulled: Vec<_> = targets.iter().map(|p| *p + Vec3::X * 150.).collect();
        simulation.reset(&targets);
        simulation.advance(&chain, &pulled, None, 0.5, &[Vec3::ZERO]);
        assert!(simulation.positions[3].distance(pulled[3]) > 1.);
        let moved: Vec<_> = targets.iter().map(|p| *p + Vec3::X * 1000.).collect();
        simulation.advance(&chain, &moved, None, chain.attraction, &[Vec3::ZERO]);
        assert_eq!(simulation.positions, moved);
        assert!(simulation.velocity.iter().all(|v| *v == Vec3::ZERO));
    }
}
