//! Fixed-tick secondary bone motion, applied after skeletal animation.
//! The inverted outline hull consumes the primary skeleton's result.
use super::field_view::{ActorPart, Art, Failures, State};
use super::sparse_animation::affine::{Helper as TransformHelper, Locals, Pose, rotation};
use bevy::{math::Affine3A, prelude::*};
use resonance_content::secondary_motion::{Chain, Environment, Simulation};
use std::collections::BTreeMap;

#[derive(Component)]
pub(super) struct Rig {
    disabled: bool,
    root: Entity,
    bones: BTreeMap<u16, Bone>,
    chains: Vec<(Chain, Simulation)>,
    tick: Option<u32>,
    creation: Option<resonance_events::ActorCreation>,
    pose: BTreeMap<u16, GlobalTransform>,
    binding_tick: Option<u32>,
    binding_pose: BTreeMap<Entity, GlobalTransform>,
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

impl Rig {
    pub(super) fn new(
        root: Entity,
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
        let chains = spec.secondary_motion.prepare(&spec.bone_names)?;
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
            root,
            bones,
            chains: chains
                .into_iter()
                .map(|chain| (chain, Simulation::default()))
                .collect(),
            tick: None,
            creation: None,
            pose: BTreeMap::new(),
            binding_tick: None,
            binding_pose: BTreeMap::new(),
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
        // The hidden binding has advanced the retained solver beyond this draw.
        // Render-only updates must reuse the draw, not those newer positions.
        if self.binding_tick == Some(tick) {
            return Some(self.pose.clone());
        }
        self.binding_tick = None;
        self.binding_pose.clear();
        let Pose::Trs(actor) = helper.local(self.root).ok()? else {
            return None;
        };
        let authored = self.authored(helper, &BTreeMap::new())?;
        let steps = if self.tick.is_none() && settle {
            300
        } else {
            self.tick
                .or(self.creation.map(|p| p.tick))
                .map_or(1, |previous| tick.saturating_sub(previous).min(16))
        };
        let initial = initial.filter(|_| self.tick.is_none() && !settle);
        let output = self.evaluate(&authored, actor.scale, yaw, animated_roots, steps, initial);
        self.tick = Some(tick);
        self.pose.clone_from(&output);
        Some(output)
    }

    fn authored(
        &self,
        helper: &TransformHelper,
        locals: &BTreeMap<Entity, Pose>,
    ) -> Option<BTreeMap<u16, Authored>> {
        self.bones
            .iter()
            .map(|(&node, bone)| {
                Some((
                    node,
                    Authored {
                        world: helper.global_with(bone.entity, locals).ok()?,
                        affine: helper.has_affine_with(bone.entity, locals),
                    },
                ))
            })
            .collect()
    }

    /// Binding runs the same model update again without drawing. Keep its
    /// solver history, but retain separate world matrices for the held draw.
    fn bind_pose(
        &mut self,
        helper: &TransformHelper,
        locals: &BTreeMap<Entity, Pose>,
        yaw: f32,
        tick: u32,
        animated_roots: &[u16],
    ) -> Option<()> {
        if self.binding_tick == Some(tick) {
            return Some(());
        }
        let Pose::Trs(actor) = helper.local(self.root).ok()? else {
            return None;
        };
        let authored = self.authored(helper, locals)?;
        let driven = self.evaluate(&authored, actor.scale, yaw, animated_roots, 1, None);
        // The model caches all world matrices before dynamics. Driven nodes
        // replace their entries; untracked children retain their authored world.
        self.binding_pose = authored
            .into_iter()
            .map(|(node, pose)| {
                (
                    self.bones[&node].entity,
                    driven.get(&node).copied().unwrap_or(pose.world),
                )
            })
            .collect();
        self.binding_tick = Some(tick);
        Some(())
    }

    pub(super) fn binding_attachment(&self, entity: Entity, tick: u32) -> Option<Vec3> {
        (self.binding_tick == Some(tick))
            .then(|| self.binding_pose.get(&entity))?
            .map(|pose| pose.translation())
    }

    fn evaluate(
        &mut self,
        authored: &BTreeMap<u16, Authored>,
        actor_scale: Vec3,
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
                .map(|joint| authored[&joint.node].world.translation())
                .collect();
            let plane = chain.collision_plane.as_ref().map(|p| {
                (
                    plane_normal(
                        authored[&p.anchor].world,
                        Vec3::from_array(p.normal),
                        authored[&p.anchor].affine,
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
            if let Some(transform) = initial {
                simulation.reset(
                    &targets
                        .iter()
                        .map(|&p| transform.transform_point3(p))
                        .collect::<Vec<_>>(),
                );
            }
            simulation.advance(
                chain,
                &targets,
                plane,
                attraction,
                steps,
                Environment::default(),
            );
            for (index, joint) in chain.joints.iter().enumerate().take(chain.joints.len() - 1) {
                let mut pose = driven_pose(
                    authored[&joint.node].world,
                    actor_scale,
                    authored[&joint.node].affine,
                );
                pose.translation = simulation.positions()[index];
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
            if let Some(parent) = parent {
                locals.push((
                    bone.entity,
                    local_pose(*world, parent, helper.has_affine(bone.entity)),
                ));
            }
        }

        locals
    }
}

fn driven_pose(world: GlobalTransform, actor_scale: Vec3, affine: bool) -> Transform {
    if affine {
        // Dynamics build a fresh world TRS from actor scale and the quaternion
        // extracted from the complete authored world matrix.
        Transform::from_translation(world.translation())
            .with_rotation(rotation(world.affine()))
            .with_scale(actor_scale)
    } else {
        world.compute_transform()
    }
}

fn local_pose(world: GlobalTransform, parent: GlobalTransform, affine: bool) -> Pose {
    if affine {
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
    if affine {
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
        let result = Rig::new(root, spec, &names).and_then(|rig| {
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
    mut rigs: Query<(&ActorPart, &mut Rig, Option<&super::field_animation::Rig>)>,
    parts: Query<&ActorPart>,
    mut transforms: ParamSet<(TransformHelper, (Query<&mut Transform>, ResMut<Locals>))>,
    mut applied: ResMut<super::field_audit::Applied>,
    mut failures: Failures,
) {
    let tick = state.get().events.tick();
    let mut poses: BTreeMap<i32, BTreeMap<u16, GlobalTransform>> = BTreeMap::new();
    {
        let helper = transforms.p0();
        for (part, mut rig, animation) in &mut rigs {
            if part.part != 0 || part.disabled || rig.disabled {
                continue;
            }
            let Some(actor) = state.get().events.world.actors.get(&part.actor) else {
                continue;
            };
            if actor.animation_bindings.0 == tick && actor.animation_bindings.1 > 1 {
                rig.disabled = true;
                if !failures.skip("field secondary motion", anyhow::anyhow!("actor {} has multiple animation bindings; intermediate poses are unavailable", part.actor)) { return; }
                continue;
            }
            if rig.binding_tick != Some(tick) {
                rig.binding_tick = None;
                rig.binding_pose.clear();
            }
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
            let output = if culled {
                Some(rig.pose.clone())
            } else {
                rig.advance(
                    &helper,
                    yaw,
                    tick,
                    state.checkpoint.is_some(),
                    animated_roots,
                    initial,
                )
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
            if rig.binding_tick != Some(tick)
                && let Some(locals) =
                    animation.and_then(|animation| animation.binding_locals(&helper))
                && rig
                    .bind_pose(&helper, &locals, yaw, tick, animated_roots)
                    .is_none()
            {
                rig.disabled = true;
                poses.remove(&part.actor);
                if !failures.skip(
                    "field secondary-motion binding",
                    anyhow::anyhow!("actor {} has unavailable binding transforms", part.actor),
                ) {
                    return;
                }
            }
        }
    }
    // Parent transforms must also use the deformed pose. Copying the same
    // world joints to the outline prevents a second, independently moving hull.
    let helper = transforms.p0();
    let mut locals = Vec::new();
    for (part, rig, _) in &rigs {
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
    fn held_affine_binding_advances_existing_solver_once_without_replacing_the_draw() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Locals>();
        let root = world.spawn(Transform::from_scale(Vec3::splat(2.))).id();
        let neck = world.spawn((Transform::IDENTITY, ChildOf(root))).id();
        let offset = Transform::from_xyz(8., 0., 0.);
        let head = world.spawn((offset, ChildOf(neck))).id();
        let tip = world.spawn((offset, ChildOf(head))).id();
        let matrix = |shear, height| {
            Affine3A::from_cols(
                Vec3::X.into(),
                Vec3::new(shear, 1., 0.).into(),
                Vec3::Z.into(),
                Vec3::new(0., 0., height).into(),
            )
        };
        world.resource_scope(|world, mut locals: Mut<Locals>| {
            locals.set(
                neck,
                &mut world.get_mut::<Transform>(neck).unwrap(),
                Pose::Affine(matrix(0.5, 20.)),
            );
        });
        let chain = Chain {
            joints: (0..3)
                .map(|node| Joint {
                    node,
                    gravity: 0.333,
                    damping: 0.766,
                })
                .collect(),
            attraction: 0.03,
            preserve_rotation: true,
            rotation_locks: [false; 2],
            collision_plane: None,
        };
        world.entity_mut(root).insert(Rig {
            disabled: false,
            root,
            bones: [
                (0, neck, root, Transform::IDENTITY),
                (1, head, neck, offset),
                (2, tip, head, offset),
            ]
            .into_iter()
            .map(|(node, entity, parent, authored)| {
                (
                    node,
                    Bone {
                        entity,
                        parent,
                        authored,
                    },
                )
            })
            .collect(),
            chains: vec![(chain, Simulation::default())],
            tick: None,
            creation: None,
            pose: BTreeMap::new(),
            binding_tick: None,
            binding_pose: BTreeMap::new(),
        });
        let hidden = BTreeMap::from([(neck, Pose::Affine(matrix(1.5, 24.)))]);
        world
            .run_system_once(move |helper: TransformHelper, mut rigs: Query<&mut Rig>| {
                let mut rig = rigs.get_mut(root).unwrap();
                rig.advance(&helper, 0., 0, false, &[], None).unwrap();
                let drawn = rig.advance(&helper, 0., 1, false, &[], None).unwrap();
                let binding = rig.authored(&helper, &hidden).unwrap();
                let targets: Vec<_> = binding
                    .values()
                    .map(|pose| pose.world.translation())
                    .collect();
                let (chain, before) = &rig.chains[0];
                let chain = chain.clone();
                let mut expected = before.clone();
                expected.advance(
                    &chain,
                    &targets,
                    None,
                    chain.attraction,
                    1,
                    Environment::default(),
                );
                rig.bind_pose(&helper, &hidden, 0., 1, &[]).unwrap();
                assert_eq!(rig.chains[0].1.positions(), expected.positions());
                assert_eq!(rig.chains[0].1.velocity(), expected.velocity());
                assert_eq!(rig.pose, drawn);
                assert_eq!(
                    rig.binding_attachment(head, 1),
                    Some(expected.positions()[1])
                );
                assert!(expected.positions()[1].distance(drawn[&1].translation()) > 1.);
                // Native world-matrix reads do not propagate a driven parent's
                // deformation into the terminal bone's already evaluated matrix.
                assert_eq!(rig.binding_attachment(tip, 1), Some(targets[2]));
                assert!(expected.positions()[2].distance(targets[2]) > 0.1);
                assert!(
                    rig.binding_pose[&head].affine().abs_diff_eq(
                        Transform::from_translation(expected.positions()[1])
                            .with_rotation(rotation(binding[&1].world.affine()))
                            .with_scale(Vec3::splat(2.))
                            .compute_affine(),
                        0.00001
                    )
                );
                for _ in 0..5 {
                    assert_eq!(
                        rig.advance(&helper, 0., 1, false, &[], None).unwrap(),
                        drawn
                    );
                    rig.bind_pose(&helper, &hidden, 0., 1, &[]).unwrap();
                    assert_eq!(rig.chains[0].1.positions(), expected.positions());
                    assert_eq!(rig.chains[0].1.velocity(), expected.velocity());
                }
                // The next ordinary update starts from the hidden binding's state.
                let next = rig.authored(&helper, &BTreeMap::new()).unwrap();
                let targets: Vec<_> = next.values().map(|pose| pose.world.translation()).collect();
                expected.advance(
                    &chain,
                    &targets,
                    None,
                    chain.attraction,
                    1,
                    Environment::default(),
                );
                rig.advance(&helper, 0., 2, false, &[], None).unwrap();
                assert_eq!(rig.chains[0].1.positions(), expected.positions());
                assert_eq!(rig.chains[0].1.velocity(), expected.velocity());
                assert_eq!(rig.binding_attachment(head, 2), None);
            })
            .unwrap();
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
        let local = local_pose(driven.into(), parent, true);
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
            Environment::default(),
        );
        assert!(simulation.positions()[3].z < targets[3].z - 5.);
        assert!(
            simulation
                .positions()
                .iter()
                .all(|p| p.is_finite() && p.y <= 0.0001)
        );
        let saved = simulation.positions().to_vec();
        for _ in 0..100 {
            simulation.advance(
                &chain,
                &targets,
                Some((Vec3::NEG_Y, 0., 1.)),
                chain.attraction,
                0,
                Environment::default(),
            );
        }
        assert_eq!(simulation.positions(), saved);
        let pulled: Vec<_> = targets.iter().map(|p| *p + Vec3::X * 150.).collect();
        simulation.reset(&targets);
        simulation.advance(&chain, &pulled, None, 0.5, 1, Environment::default());
        assert!(simulation.positions()[3].distance(pulled[3]) > 1.);
        let moved: Vec<_> = targets.iter().map(|p| *p + Vec3::X * 1000.).collect();
        simulation.advance(
            &chain,
            &moved,
            None,
            chain.attraction,
            1,
            Environment::default(),
        );
        assert_eq!(simulation.positions(), moved);
        assert!(simulation.velocity().iter().all(|v| *v == Vec3::ZERO));
    }
}
