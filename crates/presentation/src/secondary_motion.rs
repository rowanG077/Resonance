//! Fixed-tick secondary bone motion, applied after skeletal animation.
//! The inverted outline hull consumes the primary skeleton's result.
use super::field_view::{ActorPart, Art, State};
use bevy::{math::Affine3A, prelude::*, transform::helper::TransformHelper};
use resonance_content::secondary_motion::Chain;
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Rig {
    bones: BTreeMap<u16, Bone>,
    chains: Vec<(Chain, Simulation)>,
    tick: Option<u32>,
    creation: Option<resonance_events::ActorCreation>,
    pose: BTreeMap<u16, GlobalTransform>,
}
struct Bone {
    entity: Entity,
    parent: Entity,
    authored: Transform,
}

impl Rig {
    pub(super) fn new(
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
        if spec.secondary_motion.iter().any(|chain| {
            chain.joints.iter().any(|j| !bones.contains_key(&j.node))
                || chain
                    .collision_plane
                    .as_ref()
                    .is_some_and(|p| !bones.contains_key(&p.anchor))
        }) {
            return None;
        }
        Some(Self {
            bones,
            chains: spec
                .secondary_motion
                .iter()
                .cloned()
                .map(|chain| (chain, Simulation::default()))
                .collect(),
            tick: None,
            creation: None,
            pose: BTreeMap::new(),
        })
    }

    pub(super) fn advance(
        &mut self,
        helper: &TransformHelper,
        yaw: f32,
        tick: u32,
        settle: bool,
        animated_roots: &[u16],
        initial: Option<Affine3A>,
    ) -> Option<BTreeMap<u16, GlobalTransform>> {
        let authored: BTreeMap<_, _> = self
            .bones
            .iter()
            .filter_map(|(&node, bone)| {
                helper
                    .compute_global_transform(bone.entity)
                    .ok()
                    .map(|pose| (node, pose))
            })
            .collect();
        if authored.len() != self.bones.len() {
            return None;
        }

        let actor_rotation = Quat::from_rotation_z(yaw.to_radians());
        let steps = if self.tick.is_none() && settle {
            300
        } else {
            self.tick
                .or(self.creation.map(|p| p.tick))
                .map_or(1, |previous| tick.saturating_sub(previous).min(16))
        };
        let mut output = BTreeMap::new();
        for (chain, simulation) in &mut self.chains {
            let targets: Vec<_> = chain
                .joints
                .iter()
                .map(|joint| authored[&joint.node].translation())
                .collect();
            let plane = chain.collision_plane.as_ref().map(|p| {
                (
                    authored[&p.anchor].rotation() * Vec3::from_array(p.normal),
                    p.offset,
                    p.strength,
                )
            });
            let attraction = if animated_roots.contains(&chain.joints[0].node) {
                0.2
            } else {
                chain.attraction
            };
            let initial = initial.filter(|_| self.tick.is_none() && !settle);
            if let Some(transform) = initial {
                simulation.reset(
                    &targets
                        .iter()
                        .map(|&p| transform.transform_point3(p))
                        .collect::<Vec<_>>(),
                );
            }
            simulation.advance(chain, &targets, plane, attraction, steps);
            for (index, joint) in chain.joints.iter().enumerate().take(chain.joints.len() - 1) {
                let mut pose = authored[&joint.node].compute_transform();
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

        self.tick = Some(tick);
        self.pose.clone_from(&output);
        Some(output)
    }

    pub(super) fn locals(
        &self,
        helper: &TransformHelper,
        pose: &BTreeMap<u16, GlobalTransform>,
    ) -> Vec<(Entity, Transform)> {
        let mut locals = Vec::new();
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
                locals.push((bone.entity, world.reparented_to(&parent)));
            }
        }

        locals
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
    mut actors: Query<(Entity, &mut ActorPart), Without<Rig>>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform, &ChildOf)>,
    meshes: Query<(), With<Mesh3d>>,
) {
    for (root, mut actor) in &mut actors {
        let spec = &art.models[&actor.resource][actor.part].spec;
        if !actor.prepared || spec.secondary_motion.is_empty() {
            continue;
        }
        let names = super::field_pose::named_bones(root, &children, &nodes, &meshes);
        if let Some(mut rig) = Rig::new(spec, &names) {
            rig.creation = actor.creation.take();
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
    mut transforms: ParamSet<(TransformHelper, Query<&mut Transform>)>,
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
            // Offscreen actors keep their last model pose and dynamics. Move
            // the clock forward so returning onscreen does not catch up the
            // skipped simulation ticks; a new rig still evaluates once.
            if actor.animation_culled && rig.tick.is_some() {
                rig.tick = Some(tick);
                poses.insert(part.actor, rig.pose.clone());
                continue;
            }
            let animated_roots = part
                .active_clip
                .and_then(|index| art.models[&part.resource][part.part].spec.clips.get(index))
                .map(|clip| clip.secondary_pose_nodes.as_slice())
                .unwrap_or_default();
            let yaw = actor.appearance.fixed_heading.unwrap_or(actor.heading);
            // Constructor evaluation precedes same-tick script movement. Keep
            // its pose for a new rig, while restored and running actors retain
            // their normal initialization/history.
            let initial = rig.creation.filter(|_| rig.tick.is_none()).map(|p| {
                if p.position == actor.position && p.heading == yaw {
                    Affine3A::IDENTITY
                } else {
                    let transform = |position, heading: f32| {
                        Affine3A::from_rotation_translation(
                            Quat::from_rotation_z(heading.to_radians()),
                            Vec3::from_array(position),
                        )
                    };
                    transform(p.position, p.heading) * transform(actor.position, yaw).inverse()
                }
            });
            if let Some(output) = rig.advance(
                &helper,
                yaw,
                tick,
                state.checkpoint.is_some(),
                animated_roots,
                initial,
            ) {
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
    for (entity, local) in locals {
        if let Ok(mut transform) = transforms.p1().get_mut(entity) {
            *transform = local;
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
        steps: u32,
    ) {
        if self.positions.len() != targets.len() {
            self.reset(targets);
        }
        let previous_targets = std::mem::take(&mut self.targets);
        for step in 0..steps {
            // Catch-up ticks consume interpolated authored targets; rendering
            // additional frames without a field tick does not advance physics.
            let t = (step + 1) as f32 / steps as f32;
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
                for (position, velocity) in self.positions.iter_mut().zip(&self.velocity) {
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
    use resonance_content::secondary_motion::Joint;
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
            300,
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
                0,
            );
        }
        assert_eq!(simulation.positions, saved);
        let pulled: Vec<_> = targets.iter().map(|p| *p + Vec3::X * 150.).collect();
        simulation.reset(&targets);
        simulation.advance(&chain, &pulled, None, 0.5, 1);
        assert!(simulation.positions[3].distance(pulled[3]) > 1.);
        let moved: Vec<_> = targets.iter().map(|p| *p + Vec3::X * 1000.).collect();
        simulation.advance(&chain, &moved, None, chain.attraction, 1);
        assert_eq!(simulation.positions, moved);
        assert!(simulation.velocity.iter().all(|v| *v == Vec3::ZERO));
    }
}
