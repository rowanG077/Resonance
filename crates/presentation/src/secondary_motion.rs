//! Fixed-tick secondary bone motion, applied after skeletal animation.
//! The inverted outline hull consumes the primary skeleton's result.
use super::field_view::{ActorPart, Art, Failures, State};
use super::sparse_animation::affine::{Helper as TransformHelper, Locals, Pose};
use bevy::{math::Affine3A, prelude::*};
use resonance_content::secondary_motion::{Chain, Environment, Simulation};
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Rig {
    disabled: bool,
    bones: BTreeMap<u16, Bone>,
    chains: Vec<(Chain, Simulation)>,
    tick: Option<u32>,
    impulses: BTreeMap<u32, Vec3>,
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
    ) -> anyhow::Result<Option<Self>> {
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
        let chains = spec.secondary_motion.chains.clone();
        for chain in &chains {
            chain.validate(spec.bone_names.len())?;
        }
        if chains.iter().any(|chain| {
            chain.joints.iter().any(|j| !bones.contains_key(&j.node))
                || chain
                    .collision_plane
                    .as_ref()
                    .is_some_and(|p| !bones.contains_key(&p.anchor))
        }) {
            return Ok(None);
        }
        Ok(Some(Self {
            disabled: false,
            bones,
            chains: chains
                .into_iter()
                .map(|chain| (chain, Simulation::default()))
                .collect(),
            tick: None,
            impulses: BTreeMap::new(),
            creation: None,
            pose: BTreeMap::new(),
        }))
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
        // A collapsed parent has no local inverse. Leave the authored skeleton
        // intact until it expands, and restart dynamics from that current pose.
        for (chain, _) in &self.chains {
            for joint in chain.joints.iter().take(chain.joints.len() - 1) {
                let parent = helper
                    .compute_global_transform(self.bones[&joint.node].parent)
                    .ok()?;
                if inverse(parent).is_none() {
                    for (_, simulation) in &mut self.chains {
                        *simulation = Simulation::default();
                    }
                    self.tick = Some(tick);
                    self.pose.clear();
                    return Some(BTreeMap::new());
                }
            }
        }
        if self.tick == Some(tick) {
            return Some(self.pose.clone());
        }
        let authored = self
            .bones
            .iter()
            .map(|(&node, bone)| Some((node, helper.compute_global_transform(bone.entity).ok()?)))
            .collect::<Option<BTreeMap<_, _>>>()?;
        let steps = if self.tick.is_none() && settle {
            300
        } else {
            self.tick
                .or(self.creation.map(|p| p.tick))
                .map_or(1, |previous| tick.saturating_sub(previous).min(16))
        };
        let initial = initial.filter(|_| self.tick.is_none() && !settle);
        self.tick = Some(tick);
        let output = self.evaluate(&authored, yaw, animated_roots, steps, initial);
        self.pose.clone_from(&output);
        Some(output)
    }

    fn evaluate(
        &mut self,
        authored: &BTreeMap<u16, GlobalTransform>,
        yaw: f32,
        animated_roots: &[u16],
        steps: u32,
        initial: Option<Affine3A>,
    ) -> BTreeMap<u16, GlobalTransform> {
        let actor_rotation = Quat::from_rotation_z(yaw.to_radians());
        let mut output = BTreeMap::new();
        for (chain, simulation) in &mut self.chains {
            let targets: Vec<_> = chain
                .joints
                .iter()
                .map(|joint| authored[&joint.node].translation())
                .collect();
            let plane = chain.collision_plane.as_ref().map(|p| {
                (
                    plane_normal(authored[&p.anchor], Vec3::from_array(p.normal)),
                    p.offset,
                    p.strength,
                )
            });
            let attraction = if animated_roots.contains(&chain.joints[0].node) {
                0.2
            } else {
                chain.attraction
            };
            if let Some(transform) = initial {
                simulation.reset(
                    &targets
                        .iter()
                        .map(|&p| transform.transform_point3(p))
                        .collect::<Vec<_>>(),
                );
            }
            if self.impulses.is_empty() || steps == 0 {
                simulation.advance(
                    chain,
                    &targets,
                    plane,
                    attraction,
                    steps,
                    Environment::default(),
                );
            } else {
                let previous = if simulation.targets().len() == targets.len() {
                    simulation.targets().to_vec()
                } else {
                    targets.clone()
                };
                for step in 0..steps {
                    let targets: Vec<_> = previous
                        .iter()
                        .zip(&targets)
                        .map(|(from, to)| from.lerp(*to, (step + 1) as f32 / steps as f32))
                        .collect();
                    let acceleration = self
                        .tick
                        .and_then(|tick| tick.checked_sub(steps - step - 1))
                        .and_then(|tick| self.impulses.get(&tick))
                        .copied()
                        .unwrap_or_default();
                    simulation.advance(
                        chain,
                        &targets,
                        plane,
                        attraction,
                        1,
                        Environment {
                            acceleration,
                            ..Default::default()
                        },
                    );
                }
            }
            for (index, joint) in chain.joints.iter().enumerate().take(chain.joints.len() - 1) {
                let mut rotation = Quat::IDENTITY;
                if !chain.preserve_rotation {
                    let angles = |direction: Vec3| {
                        let d = actor_rotation.inverse() * direction.normalize_or_zero();
                        Vec2::new((-d.y).clamp(-1., 1.).asin(), d.x.atan2(d.z))
                    };
                    let mut delta =
                        angles(simulation.positions()[index] - simulation.positions()[index + 1])
                            - angles(targets[index] - targets[index + 1]);
                    if chain.rotation_locks[0] {
                        delta.x = 0.;
                    }
                    if chain.rotation_locks[1] {
                        delta.y = 0.;
                    }
                    rotation = actor_rotation
                        * Quat::from_euler(EulerRot::ZYX, 0., delta.y, delta.x)
                        * actor_rotation.inverse();
                }
                output.insert(
                    joint.node,
                    driven_pose(
                        authored[&joint.node],
                        simulation.positions()[index],
                        rotation,
                    ),
                );
            }
        }

        output
    }

    pub(super) fn locals(
        &self,
        helper: &TransformHelper,
        pose: &BTreeMap<u16, GlobalTransform>,
    ) -> Vec<(Entity, Pose)> {
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
            if let Some(parent) = parent
                && let Some(local) = local_pose(*world, parent)
            {
                locals.push((bone.entity, local));
            }
        }

        locals
    }
}

fn driven_pose(world: GlobalTransform, position: Vec3, rotation: Quat) -> GlobalTransform {
    let mut pose = Affine3A::from_quat(rotation) * world.affine();
    pose.translation = position.into();
    pose.into()
}

fn inverse(parent: GlobalTransform) -> Option<Affine3A> {
    let parent = parent.affine();
    if !parent.is_finite() || parent.matrix3.determinant() == 0. {
        return None;
    }
    let inverse = parent.inverse();
    inverse.is_finite().then_some(inverse)
}

fn local_pose(world: GlobalTransform, parent: GlobalTransform) -> Option<Pose> {
    let local = inverse(parent)? * world.affine();
    local.is_finite().then_some(Pose::Affine(local))
}

fn plane_normal(world: GlobalTransform, normal: Vec3) -> Vec3 {
    let tangent = normal.any_orthonormal_vector();
    let bitangent = normal.cross(tangent);
    world
        .affine()
        .transform_vector3(tangent)
        .cross(world.affine().transform_vector3(bitangent))
        .normalize_or_zero()
}

/// Read-only solver evidence for silent replay checkpoints.
pub(super) fn diagnostic(world: &mut World) -> serde_json::Value {
    let mut query = world.query::<(&ActorPart, &Rig)>();
    serde_json::json!(query.iter(world).filter(|(part, _)| part.part == 0).map(|(part, rig)| {
        serde_json::json!({"actor":part.actor,"tick":rig.tick,
            "nodes":rig.bones.iter().filter_map(|(node,bone)|world.get::<GlobalTransform>(bone.entity).map(|pose|serde_json::json!({"node":node,"world":pose.to_matrix().to_cols_array()}))).collect::<Vec<_>>(),
            "chains":rig.chains.iter().map(|(chain, simulation)| {
            serde_json::json!({"root":chain.joints[0].node,"positions":simulation.positions().iter().map(|p|p.to_array()).collect::<Vec<_>>(),
                "targets":simulation.targets().iter().map(|p|p.to_array()).collect::<Vec<_>>(),
                "velocity":simulation.velocity().iter().map(|p|p.to_array()).collect::<Vec<_>>()})
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
    mut failures: Failures,
) {
    for (root, mut actor) in &mut actors {
        let spec = &art.models[&actor.resource][actor.part].spec;
        if !actor.prepared || actor.disabled || spec.secondary_motion.is_empty() {
            continue;
        }
        let names = super::field_pose::named_bones(root, &children, &nodes, &meshes);
        let result = Rig::new(spec, &names).and_then(|rig| {
            rig.ok_or_else(|| anyhow::anyhow!("secondary-motion skeleton is incomplete"))
        });
        match result {
            Ok(mut rig) => {
                rig.creation = actor.creation.take();
                commands.entity(root).insert(rig);
            }
            Err(error) => {
                if !failures.skip(
                    "field secondary-motion binding",
                    error.context(format!("actor {}", actor.actor)),
                ) {
                    return;
                }
            }
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
    mut failures: Failures,
) {
    let tick = state.get().events.tick();
    let mut poses: BTreeMap<i32, BTreeMap<u16, GlobalTransform>> = BTreeMap::new();
    {
        let helper = transforms.p0();
        for (part, mut rig) in &mut rigs {
            if part.part != 0 || part.disabled || rig.disabled {
                continue;
            }
            let Some(actor) = state.get().events.world.actors.get(&part.actor) else {
                continue;
            };
            if actor.appearance.secondary_motion_disabled {
                rig.tick = Some(tick);
                rig.pose.clear();
                poses.insert(part.actor, BTreeMap::new());
                continue;
            }
            rig.impulses = actor
                .chain_impulses
                .iter()
                .map(|(&tick, impulse)| (tick, Vec3::from_array(impulse.acceleration)))
                .collect();
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
            let yaw = actor.appearance.fixed_heading.unwrap_or(actor.heading);
            // Constructor evaluation precedes same-tick script movement. Keep
            // its pose for a new rig, while restored and running actors retain
            // their normal initialization/history.
            let initial = rig.creation.filter(|_| rig.tick.is_none()).map(|p| {
                if p.position == actor.presented_position() && p.heading == yaw {
                    Affine3A::IDENTITY
                } else {
                    let transform = |position, heading: f32| {
                        Affine3A::from_rotation_translation(
                            Quat::from_rotation_z(heading.to_radians()),
                            Vec3::from_array(position),
                        )
                    };
                    transform(p.position, p.heading)
                        * transform(actor.presented_position(), yaw).inverse()
                }
            });
            let output = if culled {
                Some(rig.pose.clone())
            } else {
                rig.advance(&helper, yaw, tick, false, animated_roots, initial)
            };
            let Some(output) = output else {
                rig.disabled = true;
                if !failures.skip(
                    "field secondary motion",
                    anyhow::anyhow!("actor {} has an unavailable skeleton pose", part.actor),
                ) {
                    return;
                }
                continue;
            };
            poses.insert(part.actor, output);
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
            affine.set(entity, &mut transform, local);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::secondary_motion::Joint;

    #[test]
    fn dynamics_preserve_authored_basis_and_attachment_placement() {
        let parent = GlobalTransform::from(Affine3A::from_cols(
            Vec3::new(2., 0., 1.).into(),
            Vec3::new(0.5, -3., 0.).into(),
            Vec3::new(0., 0., 4.).into(),
            Vec3::splat(10.).into(),
        ));
        let authored = parent.mul_transform(Transform::from_xyz(4., 5., 6.));
        let position = authored.translation() + Vec3::new(1., -2., 3.);
        let delta = Quat::from_rotation_x(0.4);
        let driven = driven_pose(authored, position, delta);
        assert_eq!(driven.translation(), position);
        let basis = authored.affine().matrix3;
        let deformed = driven.affine().matrix3;
        assert!((deformed.transpose() * deformed).abs_diff_eq(basis.transpose() * basis, 0.00001));
        assert!(deformed.abs_diff_eq(bevy::math::Mat3A::from_quat(delta) * basis, 0.00001));
        let local = local_pose(driven, parent).unwrap();
        assert!(matches!(local, Pose::Affine(_)));
        let restored = parent * local.global();
        assert!(restored.affine().abs_diff_eq(driven.affine(), 0.00001));
        assert!(
            plane_normal(parent, Vec3::Z).abs_diff_eq(Vec3::new(3., 0.5, -6.).normalize(), 0.00001)
        );
        let attachment = Vec3::new(2., 3., 4.);
        assert!(
            restored
                .transform_point(attachment)
                .abs_diff_eq(driven.transform_point(attachment), 0.00001)
        );
    }

    #[test]
    fn collapsed_parent_suspends_dynamics_and_recovers_on_expansion() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let parent = world.spawn(Transform::IDENTITY).id();
        let authored = Transform::from_xyz(0., 0., 40.).with_scale(Vec3::new(2., 3., 4.));
        let bone = world.spawn((authored, ChildOf(parent))).id();
        let tip_pose = Transform::from_xyz(8., 0., 0.);
        let tip = world.spawn((tip_pose, ChildOf(bone))).id();
        let rig = world
            .spawn(Rig {
                disabled: false,
                bones: BTreeMap::from([
                    (
                        0,
                        Bone {
                            entity: bone,
                            parent,
                            authored,
                        },
                    ),
                    (
                        1,
                        Bone {
                            entity: tip,
                            parent: bone,
                            authored: tip_pose,
                        },
                    ),
                ]),
                chains: vec![(
                    Chain {
                        joints: vec![
                            Joint {
                                node: 0,
                                gravity: 0.,
                                damping: 0.7,
                            },
                            Joint {
                                node: 1,
                                gravity: 0.3,
                                damping: 0.7,
                            },
                        ],
                        attraction: 0.03,
                        preserve_rotation: false,
                        rotation_locks: [false; 2],
                        collision_plane: None,
                    },
                    Simulation::default(),
                )],
                tick: None,
                impulses: BTreeMap::new(),
                creation: None,
                pose: BTreeMap::new(),
            })
            .id();
        for (tick, scale) in [(1, Vec3::ONE), (2, Vec3::ZERO), (3, Vec3::splat(2.))] {
            world.get_mut::<Transform>(parent).unwrap().scale = scale;
            let (pose, locals) = world
                .run_system_once(move |helper: TransformHelper, mut rigs: Query<&mut Rig>| {
                    let mut rig = rigs.get_mut(rig).unwrap();
                    let pose = rig.advance(&helper, 0., tick, false, &[], None).unwrap();
                    let locals = rig.locals(&helper, &pose);
                    (pose, locals)
                })
                .unwrap();
            let rig = world.get::<Rig>(rig).unwrap();
            assert!(!rig.disabled);
            assert_eq!(*world.get::<Transform>(bone).unwrap(), authored);
            if tick == 2 {
                assert!(pose.is_empty() && locals.is_empty());
                assert!(rig.chains[0].1.positions().is_empty());
                assert!(
                    local_pose(
                        GlobalTransform::IDENTITY,
                        Transform::from_scale(scale).into()
                    )
                    .is_none()
                );
            } else {
                assert_eq!(locals.len(), 1);
                assert!(pose[&0].affine().is_finite());
                assert!(locals[0].1.global().affine().is_finite());
                let simulated = rig.chains[0].1.positions();
                assert!(simulated.iter().all(|position| position.is_finite()));
                assert!(simulated[1].z < rig.chains[0].1.targets()[1].z);
                assert!(
                    pose[&0]
                        .translation()
                        .abs_diff_eq(Vec3::new(0., 0., 40.) * scale, 0.0001)
                );
            }
        }
    }
}
