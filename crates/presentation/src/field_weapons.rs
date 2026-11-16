//! Replace opted-in equipment placeholders on playable characters' KK joints.
//! The original prop still owns choreography, visibility, flight and lifetime.
mod choreography;
mod equipment;
use equipment::Equipment;
#[cfg(test)]
use equipment::capture;
pub(super) use equipment::{EquipmentCache, prepare};
mod publication;
mod ropes;
use super::{
    draw_order::DrawOrder,
    field_view::{ActorPart, Art, Failures, State},
    materials::TitleSurface,
    sparse_animation::{Binding, affine::Locals},
};
use crate::sparse_animation::affine::{PoseEndpoint, PoseWorkspace};
use anyhow::{Context as ContextExt, Result};
use bevy::{
    ecs::{schedule::ScheduleConfigs, system::ScheduleSystem},
    prelude::*,
};
use choreography::apply as choreograph;
pub(super) use publication::publish;
use resonance_content::battle_model::AttachmentTextureAnimation;
use resonance_events::effect::Blend;
pub(super) use ropes::render as strings;
use std::collections::{BTreeMap, BTreeSet};

fn carried(
    actor: &resonance_events::Actor,
    world: &resonance_events::GameWorld,
) -> Option<resonance_events::FieldWeapon> {
    symphonia_constants::FieldEquipmentModelId::try_from(actor.resource).ok()?;
    actor.field_weapon.or_else(|| {
        let attachment = actor.attachment.as_ref()?;
        let owner = world.field_model_actor(attachment.actor)?;
        resonance_events::FieldWeapon::from_attachment(
            actor.resource,
            owner.resource,
            &attachment.bone,
        )
    })
}

/// Both live simulation and captures wait for the actual replacement scenes.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Readiness<'w, 's> {
    cache: Option<Res<'w, EquipmentCache>>,
    actors: Query<'w, 's, (Entity, &'static ActorPart)>,
    roots: Query<'w, 's, &'static Instance>,
}
impl Readiness<'_, '_> {
    pub(super) fn ready(&self, world: &resonance_events::GameWorld) -> bool {
        let Some(cache) = &self.cache else {
            return false;
        };
        if !cache.assets_ready(world) {
            return false;
        }
        let layers: Vec<_> = self.roots.iter().map(Instance::state).collect();
        self.actors.iter().all(|(owner, part)| {
            if part.part != 0 || part.pass != 0 {
                return true;
            }
            let Some(actor) = world.field_model_actor(part.actor) else {
                return true;
            };
            let Some((binding, item)) = selection(actor, world) else {
                return true;
            };
            if !part.current(actor) {
                return false;
            }
            part.disabled
                || item == 0
                || cache.failed.contains(&item)
                || cache
                    .equipment
                    .get(&item)
                    .is_some_and(|model| model.complete(owner, item, binding, &layers))
        })
    }
}

pub(super) fn ready(world: &mut World) -> bool {
    let mut state = bevy::ecs::system::SystemState::<(State, Readiness)>::new(world);
    let (field, readiness) = state.get(world).expect("field readiness resources");
    readiness.ready(&field.get().events.world)
}

/// For tests that inject renderer readiness: the live field's equipment has
/// no models to wait for.
#[cfg(test)]
pub(crate) fn without_models_for_test(world: &mut World) {
    let mut cache = EquipmentCache::default();
    cache.failed.extend(equipment::equipped(
        &world
            .resource::<crate::new_game::Session>()
            .field()
            .events
            .world,
    ));
    world.insert_resource(cache);
}

pub(super) fn retire(world: &mut World) {
    let roots: Vec<_> = world
        .query_filtered::<Entity, With<Instance>>()
        .iter(world)
        .collect();
    for root in roots {
        world.despawn(root);
    }
}

fn selection(
    actor: &resonance_events::Actor,
    world: &resonance_events::GameWorld,
) -> Option<(resonance_events::FieldWeapon, u16)> {
    let binding = carried(actor, world)?;
    let member = world
        .party
        .as_ref()?
        .members
        .get(usize::from(binding.character - 1))?;
    let item = member.equipment[if matches!(binding.character, 6 | 9) && binding.slot == 1 {
        5
    } else {
        0
    }];
    Some((binding, item))
}

#[derive(Clone, Copy)]
struct LayerState {
    key: (Entity, u16, u8, usize),
    ready: bool,
    active: bool,
}

impl Equipment {
    fn has_dynamics(&self, layer: usize) -> bool {
        let layer = &self.layers[layer];
        !layer.art.spec.secondary_motion.is_empty()
            && !self.layers[layer.rig_layer]
                .art
                .spec
                .secondary_motion
                .is_empty()
    }

    fn part(&self, binding: resonance_events::FieldWeapon) -> Option<u8> {
        let part = if self.shield {
            binding.slot.checked_sub(1)?
        } else if binding.character == 5 {
            if binding.slot >= 4 {
                return None;
            }
            binding.slot / 2
        } else {
            binding.slot
        };
        self.layers
            .iter()
            .any(|layer| layer.part == part)
            .then_some(part)
    }

    fn complete(
        &self,
        owner: Entity,
        item: u16,
        binding: resonance_events::FieldWeapon,
        states: &[LayerState],
    ) -> bool {
        self.ready
            && self.part(binding).is_none_or(|part| {
                self.layers
                    .iter()
                    .enumerate()
                    .filter(|(_, layer)| layer.part == part)
                    .all(|(index, _)| {
                        states.iter().any(|state| {
                            state.key == (owner, item, binding.slot, index) && state.ready
                        })
                    })
            })
    }
}

#[derive(Component)]
pub(super) struct Instance {
    owner: Entity,
    actor: i32,
    item: u16,
    slot: u8,
    layer: usize,
    birth: u32,
    binding: Option<Binding>,
    clip_ages: Option<[u32; 2]>,
    clip_poses: Vec<PoseEndpoint>,
    camera_facing: Vec<Entity>,
    materials: Vec<Handle<TitleSurface>>,
    winding: super::field_pose::Winding,
    geometry: Vec<(usize, Entity)>,
    dynamics: Option<super::secondary_motion::Rig>,
    future: Option<(u64, Vec<PoseEndpoint>)>,
    choreography: Option<choreography::Mapping>,
    prepared: bool,
    sampled: bool,
    active: bool,
    rope: Option<(Entity, Handle<Mesh>, Handle<TitleSurface>)>,
}
impl Instance {
    fn rig_key(&self, cache: &EquipmentCache) -> (Entity, u16, u8, usize) {
        let rig = cache.equipment[&self.item].layers[self.layer].rig_layer;
        (self.owner, self.item, self.slot, rig)
    }

    fn state(&self) -> LayerState {
        LayerState {
            key: (self.owner, self.item, self.slot, self.layer),
            ready: self.prepared,
            active: self.active,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn instances(
    mut commands: Commands,
    state: State,
    cache: Res<EquipmentCache>,
    actors: Query<(Entity, &ActorPart)>,
    roots: Query<(Entity, &Instance)>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    let field = state.get();
    let world = &field.events.world;
    let mut wanted = BTreeSet::new();
    let mut holding = BTreeSet::new();
    for (owner, part) in &actors {
        if part.part != 0 || part.pass != 0 || part.disabled {
            continue;
        }
        let Some((binding, item)) = world
            .field_model_actor(part.actor)
            .and_then(|actor| selection(actor, world))
        else {
            continue;
        };
        if item == 0 || cache.failed.contains(&item) {
            continue;
        }
        let model = cache.equipment.get(&item).filter(|model| model.ready);
        // Keep the published item through a cold swap, including scene instantiation.
        if model.is_some_and(|model| model.part(binding).is_none()) {
            continue;
        }
        holding.insert(owner);
        let Some(model) = model else { continue };
        let Some(model_part) = model.part(binding) else {
            continue;
        };
        let slot = binding.slot;
        for (index, layer) in model
            .layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.part == model_part)
        {
            let key = (owner, item, slot, index);
            wanted.insert(key);
            if roots
                .iter()
                .any(|(_, root)| (root.owner, root.item, root.slot, root.layer) == key)
            {
                continue;
            }
            let materials = layer
                .art
                .surfaces()
                .into_iter()
                .map(|surface| surfaces.add(surface))
                .collect();
            let root = commands
                .spawn((
                    Transform::from_translation(Vec3::from_array(layer.preview.scene.translation)),
                    Visibility::Hidden,
                    Instance {
                        owner,
                        actor: part.actor,
                        item,
                        slot,
                        layer: index,
                        birth: field.effect_clock.tick(),
                        binding: None,
                        clip_ages: None,
                        clip_poses: vec![],
                        camera_facing: Vec::new(),
                        materials,
                        winding: Default::default(),
                        geometry: Vec::new(),
                        dynamics: None,
                        future: None,
                        choreography: None,
                        prepared: false,
                        sampled: false,
                        active: false,
                        rope: None,
                    },
                ))
                .id();
            crate::model_asset::instantiate(&mut commands, root, layer.art.scene.clone());
        }
    }
    for (entity, root) in &roots {
        if !wanted.contains(&(root.owner, root.item, root.slot, root.layer))
            && !(root.active && holding.contains(&root.owner))
        {
            commands.entity(entity).despawn();
        }
    }
}

fn texture_offset(
    animation: &AttachmentTextureAnimation,
    age: u32,
    fraction: f32,
) -> (usize, [f32; 2]) {
    match animation {
        AttachmentTextureAnimation::Scroll {
            texture,
            step_milli_uv,
            offset_milli_uv,
        } => (
            usize::from(*texture),
            std::array::from_fn(|axis| {
                ((i64::from(offset_milli_uv[axis])
                    + i64::from(step_milli_uv[axis]) * i64::from(age))
                    % 1000) as f32
                    / 1000.
                    + f32::from(step_milli_uv[axis]) * fraction / 1000.
            }),
        ),
        AttachmentTextureAnimation::Atlas {
            texture,
            rows,
            ticks_per_frame,
            frames,
        } => {
            let frame = age / u32::from(ticks_per_frame.get());
            let row = if frames.is_empty() {
                (frame % u32::from(*rows)) as u8
            } else {
                frames[frame as usize % frames.len()]
            };
            (
                usize::from(*texture),
                [0., f32::from(row) / f32::from(*rows)],
            )
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Read bind poses before writing sampled transforms.
pub(super) fn render(
    prediction: Res<super::field_view::Prediction>,
    mut commands: Commands,
    state: State,
    cache: Res<EquipmentCache>,
    actors: Query<(Entity, &ActorPart)>,
    mut roots: Query<
        (Entity, &mut Instance, &crate::model_asset::Instance),
        With<crate::model_asset::Instance>,
    >,
    mut orders: Query<&mut DrawOrder>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut failures: Failures,
) {
    let field = state.get();
    let world = &field.events.world;
    for (_entity, mut root, model_instance) in &mut roots {
        root.sampled = false;
        root.prepared = false;
        let Some(actor) = world.field_model_actor(root.actor) else {
            continue;
        };
        let pose = actor.field_model_pose();
        let Ok((_, owner)) = actors.get(root.owner) else {
            continue;
        };
        let Some(model) = cache.equipment.get(&root.item) else {
            continue;
        };
        let layer = &model.layers[root.layer];
        let result = (|| -> Result<()> {
            if root.binding.is_none() {
                root.binding = Some(model_instance.binding(layer.art.spec.bone_names.len())?);
                root.camera_facing = model_instance.facing();
                for (child, draw) in model_instance.draws() {
                    let index = draw.slot.index(root.materials.len())?;
                    root.geometry.push((index, child));
                    commands.entity(child).insert((
                        MeshMaterial3d(root.materials[index].clone()),
                        super::field_view::ActorGeometry(root.actor),
                    ));
                }
            }
            let age = field.effect_clock.tick().wrapping_sub(root.birth);
            let fraction = effect_fraction(field, &prediction, root.actor);
            let body_material = owner
                .materials
                .first()
                .and_then(|handle| surfaces.get(handle))
                .cloned();
            let tint = body_material.as_ref().map_or_else(
                || Vec3::ONE.extend(f32::from(pose.opacity) / 255.),
                |body| body.tint,
            );
            let order = owner
                .geometry
                .first()
                .and_then(|(_, entity)| orders.get(*entity).ok())
                .copied();
            for &(index, child) in &root.geometry {
                let spec = &layer.art.spec.materials[index];
                if let Some(order) = order {
                    let next = DrawOrder(
                        order.0,
                        order.1,
                        2 * resonance_content::field::MODEL_DRAW_SPAN
                            + u32::from(root.slot) * 4 * resonance_content::field::MODEL_DRAW_SPAN
                            + spec.draw_order,
                    );
                    if let Ok(mut order) = orders.get_mut(child) {
                        *order = next;
                    } else {
                        commands.entity(child).insert(next);
                    }
                }
                let mut surface = surfaces
                    .get_mut(&root.materials[index])
                    .context("missing field weapon material")?;
                surface.tint = tint;
                if let Some(body) = &body_material {
                    surface.field_light = body.field_light;
                    surface.shade_colors = body.shade_colors;
                    surface.ambient_color = body.ambient_color;
                    surface.ambient_modulation = body.ambient_modulation;
                    surface.scenery_lighting = body.scenery_lighting;
                    surface.toon_ramp = if layer.preview.additive {
                        None
                    } else {
                        body.toon_ramp.clone()
                    };
                }
                if let Some(color) = layer.art.spec.outline_color_for(spec) {
                    surface.tint *= Vec4::from_array(color.map(|value| f32::from(value) / 255.));
                    surface.toon_ramp = None;
                }
                surface.blend = if layer.preview.additive {
                    Some(Blend::Additive)
                } else {
                    pose.blend
                        .or((spec.blend || surface.tint.w < 1.).then_some(Blend::Alpha))
                };
                surface.depth_write =
                    spec.depth_write && pose.depth_write && !layer.preview.additive;
                surface.uv_offsets = layer
                    .preview
                    .uv_offsets
                    .get(index)
                    .copied()
                    .map(Vec4::from_array)
                    .unwrap_or_default();
                if let Some(animation) = &layer.animation {
                    let (texture, uv) = texture_offset(animation, age, fraction);
                    for (stage, binding) in [&spec.color, &spec.multiply].into_iter().enumerate() {
                        if binding
                            .as_ref()
                            .is_some_and(|binding| binding.texture == texture)
                        {
                            surface.uv_offsets[stage * 2] += uv[0];
                            surface.uv_offsets[stage * 2 + 1] += uv[1];
                        }
                    }
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => root.sampled = true,
            Err(error) => {
                failures.skip("field weapon rendering", error);
            }
        }
    }
}

const COMMITTED: u8 = 0;
const FUTURE: u8 = 1;
const DRAW: u8 = 2;

fn next_actor<'a>(
    field: &resonance_game::field::FieldSession,
    prediction: &'a super::field_view::Prediction,
    id: i32,
) -> Option<&'a resonance_game::field::FieldSession> {
    let next = prediction.field()?;
    let current = field.events.world.field_model_actor(id)?;
    let future = next.events.world.field_model_actor(id)?;
    (super::field_view::prediction::continuous_pose(current, future)
        && selection(current, &field.events.world) == selection(future, &next.events.world))
    .then_some(next)
}

fn effect_fraction(
    field: &resonance_game::field::FieldSession,
    prediction: &super::field_view::Prediction,
    id: i32,
) -> f32 {
    next_actor(field, prediction, id)
        .filter(|next| next.effect_clock.tick() == field.effect_clock.tick().wrapping_add(1))
        .map_or(0., |_| prediction.alpha)
}

/// Only equipment with a secondary chain needs exact physics visits. Cache the
/// solved poses for all draws that share this pair of actor endpoints.
pub(super) fn physics_endpoints() -> ScheduleConfigs<ScheduleSystem> {
    (
        visit::<COMMITTED>(),
        super::field_animation::present::<true>,
        visit::<FUTURE>(),
    )
        .chain()
        .run_if(needs_physics_endpoints)
}

fn needs_physics_endpoints(
    prediction: Res<super::field_view::Prediction>,
    cache: Res<EquipmentCache>,
    roots: Query<&Instance>,
) -> bool {
    prediction.poses().is_some()
        && roots.iter().filter(|root| root.sampled).any(|root| {
            cache.equipment[&root.item].has_dynamics(root.layer)
                && root
                    .future
                    .as_ref()
                    .is_none_or(|(key, _)| *key != prediction.counts()[0])
        })
}

fn visit<const VISIT: u8>() -> ScheduleConfigs<ScheduleSystem> {
    (sample::<VISIT>, choreograph::<VISIT>, dynamics::<VISIT>).chain()
}

pub(super) fn present() -> ScheduleConfigs<ScheduleSystem> {
    visit::<DRAW>()
}

fn sample<const VISIT: u8>(
    state: State,
    prediction: Res<super::field_view::Prediction>,
    cache: Res<EquipmentCache>,
    mut roots: Query<&mut Instance>,
    mut transforms: Query<&mut Transform>,
    mut affine: ResMut<Locals>,
    mut failures: Failures,
) {
    let current = state.get();
    for mut root in &mut roots {
        if !root.sampled {
            continue;
        }
        let model = &cache.equipment[&root.item];
        if VISIT != DRAW && !model.has_dynamics(root.layer) {
            continue;
        }
        let layer = &model.layers[root.layer];
        let root = &mut *root;
        let age = current.effect_clock.tick().wrapping_sub(root.birth);
        let next_age = next_actor(current, &prediction, root.actor).map_or(age, |next| {
            next.effect_clock.tick().wrapping_sub(root.birth)
        });
        let fraction = match VISIT {
            FUTURE => 1.,
            DRAW => prediction.alpha,
            _ => 0.,
        };
        let result = (|| -> Result<()> {
            let binding = root.binding.as_ref().unwrap();
            if let Some((index, clip)) = layer.preview.selected_clip()? {
                let motion = layer.art.motions[index].as_ref();
                let ages = [age, next_age];
                if root.clip_ages != Some(ages) {
                    let duration = motion.duration_frames * resonance_content::ANIMATION_HZ
                        / resonance_content::animation::FRAME_HZ;
                    let clock = resonance_events::Animation::new(
                        u32::from(layer.art.spec.resource),
                        clip.resource_slot,
                        duration.ceil() as u32,
                        0,
                    );
                    let endpoint = |tick| {
                        (
                            motion,
                            clock.sample(tick, 0, duration) / resonance_content::ANIMATION_HZ,
                        )
                    };
                    binding.sample_endpoints(
                        endpoint(age),
                        endpoint(next_age),
                        &mut root.clip_poses,
                    )?;
                    root.clip_ages = Some(ages);
                }
                for &endpoint in &root.clip_poses {
                    endpoint.apply(fraction, &mut transforms, &mut affine);
                }
            } else {
                for &(entity, rest) in &binding.0 {
                    affine.replace_for_render(
                        entity,
                        &mut *transforms.get_mut(entity)?,
                        rest.into(),
                    );
                }
            }
            if let Some(camera) = &current.events.world.field_camera {
                let rotation = match VISIT {
                    DRAW => prediction.camera_transform(camera).rotation,
                    FUTURE => {
                        super::field_view::camera_transform(
                            prediction.camera(camera).unwrap_or(camera),
                        )
                        .rotation
                    }
                    _ => super::field_view::camera_transform(camera).rotation,
                };
                for &entity in &root.camera_facing {
                    affine.face_camera(entity, &mut *transforms.get_mut(entity)?, rotation);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            root.sampled = false;
            failures.skip("field equipment pose", error);
        }
    }
}

#[allow(clippy::type_complexity)]
fn dynamics<const VISIT: u8>(
    (mut overlays, mut workspace): (Local<Vec<PoseEndpoint>>, Local<PoseWorkspace>),
    prediction: Res<super::field_view::Prediction>,
    state: State,
    cache: Res<EquipmentCache>,
    parents: Query<&ChildOf>,
    mut roots: Query<(Entity, &mut Instance), With<crate::model_asset::Instance>>,
    mut transforms: ParamSet<(
        super::sparse_animation::affine::Helper,
        (Query<&mut Transform>, ResMut<Locals>),
    )>,
) {
    let field = state.get();
    let mut poses = BTreeMap::new();
    let mut locals = Vec::new();
    let mut future = BTreeMap::new();
    overlays.clear();
    {
        let helper = transforms.p0();
        for (entity, mut root) in &mut roots {
            if !root.prepared {
                continue;
            }
            let model = &cache.equipment[&root.item];
            let layer = &model.layers[root.layer];
            if !model.has_dynamics(root.layer) {
                continue;
            }
            if root.dynamics.is_none() {
                let Some(binding) = &root.binding else {
                    continue;
                };
                let names = layer
                    .art
                    .spec
                    .bone_names
                    .iter()
                    .zip(&binding.0)
                    .filter_map(|(name, &(bone, rest))| {
                        Some((name.clone(), (bone, rest, parents.get(bone).ok()?.parent())))
                    })
                    .collect();
                root.dynamics = super::secondary_motion::Rig::new(entity, &layer.art.spec, &names);
            }
            let root = &mut *root;
            let key = root.rig_key(&cache);
            let Some(live) = &mut root.dynamics else {
                continue;
            };
            if VISIT == DRAW && prediction.poses().is_some() {
                if let Some((_, cached)) = root
                    .future
                    .as_ref()
                    .filter(|(key, _)| *key == prediction.counts()[0])
                {
                    overlays.extend_from_slice(cached);
                }
                continue;
            }
            // Shared layers consume their rig's solved joints.
            if layer.rig_layer != root.layer {
                continue;
            }
            let actor_id = root.actor;
            let field = if VISIT == FUTURE {
                next_actor(field, &prediction, actor_id).unwrap_or(field)
            } else {
                field
            };
            let mut branch = (VISIT == FUTURE).then(|| live.clone());
            let rig = branch.as_mut().unwrap_or(live);
            if let Some(actor) = field.events.world.field_model_actor(actor_id) {
                rig.controls(actor, &field.events.world);
            }
            let yaw = helper
                .compute_global_transform(entity)
                .map(|world| {
                    super::sparse_animation::affine::rotation(world.affine())
                        .to_euler(EulerRot::ZYX)
                        .0
                        .to_degrees()
                })
                .unwrap_or_default();
            if rig
                .advance(
                    &helper,
                    yaw,
                    field.events.tick(),
                    state.checkpoint.is_some(),
                    layer
                        .preview
                        .selected_clip()
                        .ok()
                        .flatten()
                        .map(|(_, clip)| clip.pose_chains.as_slice())
                        .unwrap_or_default(),
                )
                .is_none()
            {
                continue;
            }
            if VISIT == DRAW {
                poses.insert(key, entity);
            }
            if let Some(branch) = branch {
                future.insert(key, live.equipment_endpoints(&branch));
            }
        }
        if VISIT == FUTURE {
            for (_, mut root) in &mut roots {
                if let Some(rig) = &root.dynamics
                    && let Some(pose) = future.get(&root.rig_key(&cache))
                {
                    root.future = Some((prediction.counts()[0], rig.bind_equipment(pose)));
                }
            }
        }
        if VISIT != DRAW {
            return;
        }
        for (_, root) in &roots {
            if let Some(rig) = &root.dynamics
                && let Some(&primary) = poses.get(&root.rig_key(&cache))
            {
                let primary = roots.get(primary).unwrap().1.dynamics.as_ref().unwrap();
                locals.extend(rig.locals(&helper, &primary.pose));
            }
        }
    }
    if !overlays.is_empty() {
        super::sparse_animation::affine::apply_poses(
            &overlays,
            prediction.alpha,
            &mut transforms,
            &mut workspace,
        );
    }
    let (mut nodes, mut affine) = transforms.p1();
    for (entity, local) in locals {
        if let Ok(mut transform) = nodes.get_mut(entity) {
            local.apply(entity, &mut transform, &mut affine);
        }
    }
}

#[cfg(test)]
mod tests;
