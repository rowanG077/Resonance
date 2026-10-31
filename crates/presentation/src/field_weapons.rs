//! Replace opted-in equipment placeholders on playable characters' KK joints.
//! The original prop still owns choreography, visibility, flight and lifetime.
mod choreography;
mod equipment;
use equipment::Equipment;
pub(super) use equipment::{EquipmentCache, prepare};
#[cfg(test)]
use equipment::{Layer, capture};
mod publication;
mod ropes;
use super::{
    draw_order::DrawOrder,
    field_view::{ActorPart, Art, Failures, State},
    materials::{MaterialSlot, TitleSurface},
    sparse_animation::{Binding, affine::Locals},
};
use anyhow::{Context, Result};
use bevy::{prelude::*, world_serialization::WorldInstanceReady};
pub(super) use choreography::apply as choreograph;
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
        let owner = world.actors.get(&attachment.actor)?;
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
            let Some(actor) = world.actors.get(&part.actor) else {
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
    camera_facing: Vec<Entity>,
    materials: Vec<Handle<TitleSurface>>,
    geometry: Vec<(usize, Entity)>,
    dynamics: Option<super::secondary_motion::Rig>,
    choreography: Option<choreography::Mapping>,
    prepared: bool,
    sampled: bool,
    active: bool,
    rope: Option<(Entity, Handle<Mesh>, Handle<TitleSurface>)>,
}
impl Instance {
    fn state(&self) -> LayerState {
        LayerState {
            key: (self.owner, self.item, self.slot, self.layer),
            ready: self.prepared,
            active: self.active,
        }
    }
}
#[derive(Component)]
pub(super) struct Loaded;

#[allow(clippy::too_many_arguments)]
pub(super) fn instances(
    mut commands: Commands,
    state: State,
    cache: Res<EquipmentCache>,
    actors: Query<(Entity, &ActorPart)>,
    roots: Query<(Entity, &Instance)>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut images: ResMut<Assets<Image>>,
    mut sampled: ResMut<super::scene::SampledImages>,
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
            .actors
            .get(&part.actor)
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
                .surfaces(&mut images, &mut sampled)
                .into_iter()
                .map(|surface| surfaces.add(surface))
                .collect();
            commands
                .spawn((
                    WorldAssetRoot(layer.art.scene.clone()),
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
                        camera_facing: Vec::new(),
                        materials,
                        geometry: Vec::new(),
                        dynamics: None,
                        choreography: None,
                        prepared: false,
                        sampled: false,
                        active: false,
                        rope: None,
                    },
                ))
                .observe(
                    |event: On<WorldInstanceReady>,
                     mut commands: Commands,
                     mut roots: Query<&mut Instance>| {
                        if let Ok(mut root) = roots.get_mut(event.entity) {
                            root.binding = None;
                            root.camera_facing.clear();
                            root.geometry.clear();
                            root.dynamics = None;
                            root.choreography = None;
                            root.prepared = false;
                            if let Some((entity, _, _)) = root.rope.take()
                                && let Ok(mut entity) = commands.get_entity(entity)
                            {
                                entity.try_despawn();
                            }
                        }
                        commands.entity(event.entity).insert(Loaded);
                    },
                );
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

fn texture_offset(animation: &AttachmentTextureAnimation, age: u32) -> (usize, [f32; 2]) {
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
    mut commands: Commands,
    state: State,
    cache: Res<EquipmentCache>,
    children: Query<&Children>,
    slots: Query<&MaterialSlot>,
    actors: Query<(Entity, &ActorPart)>,
    mut roots: Query<(Entity, &mut Instance), With<Loaded>>,
    mut transforms: ParamSet<(
        Query<(&Transform, &bevy::gltf::GltfExtras)>,
        Query<&mut Transform>,
    )>,
    mut orders: Query<&mut DrawOrder>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut affine: ResMut<Locals>,
    mut failures: Failures,
) {
    let field = state.get();
    let world = &field.events.world;
    for (entity, mut root) in &mut roots {
        root.sampled = false;
        root.prepared = false;
        let Some(actor) = world.actors.get(&root.actor) else {
            continue;
        };
        let Ok((_, owner)) = actors.get(root.owner) else {
            continue;
        };
        let Some(model) = cache.equipment.get(&root.item) else {
            continue;
        };
        let layer = &model.layers[root.layer];
        let result = (|| -> Result<()> {
            if root.binding.is_none() {
                let rest = transforms.p0();
                let binding =
                    Binding::new(entity, layer.art.spec.bone_names.len(), &children, &rest)?;
                for &(entity, _) in &binding.0 {
                    let extras: serde_json::Value =
                        serde_json::from_str(&rest.get(entity)?.1.value)?;
                    if extras["resonance_camera_facing"] == true {
                        root.camera_facing.push(entity);
                    }
                }
                root.binding = Some(binding);
                for child in children.iter_descendants(entity) {
                    if let Ok(slot) = slots.get(child) {
                        let index = slot.index(root.materials.len())?;
                        root.geometry.push((index, child));
                        commands.entity(child).insert((
                            MeshMaterial3d(root.materials[index].clone()),
                            super::field_view::ActorGeometry(root.actor),
                        ));
                    }
                }
            }
            let age = field.effect_clock.tick().wrapping_sub(root.birth);
            if let Some((index, clip)) = layer.preview.selected_clip()? {
                let motion = &layer.art.motions[index];
                let duration = motion.duration_frames * resonance_content::ANIMATION_HZ
                    / resonance_content::animation::FRAME_HZ;
                let clock = resonance_events::Animation::new(
                    u32::from(layer.art.spec.resource),
                    clip.resource_slot,
                    duration.ceil() as u32,
                    0,
                );
                root.binding.as_ref().unwrap().sample(
                    motion,
                    clock.sample(age, 0, duration) / resonance_content::ANIMATION_HZ,
                    &mut transforms.p1(),
                    &mut affine,
                )?;
            } else {
                for &(entity, rest) in &root.binding.as_ref().unwrap().0 {
                    affine.set(entity, &mut *transforms.p1().get_mut(entity)?, rest.into());
                }
            }
            if let Some(camera) = &world.field_camera {
                let rotation = super::field_view::camera_transform(camera).rotation;
                for &entity in &root.camera_facing {
                    affine.face_camera(entity, &mut *transforms.p1().get_mut(entity)?, rotation);
                }
            }
            let body_material = owner
                .materials
                .first()
                .and_then(|handle| surfaces.get(handle))
                .cloned();
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
                if let Some(body) = &body_material {
                    surface.tint = body.tint;
                    surface.field_light = body.field_light;
                    surface.shade_colors = body.shade_colors;
                    surface.ambient_color = body.ambient_color;
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
                    actor
                        .blend
                        .or((spec.blend || actor.opacity < 255).then_some(Blend::Alpha))
                };
                surface.depth_write =
                    spec.depth_write && actor.depth_write && !layer.preview.additive;
                surface.cull = if actor.model_scale().iter().product::<f32>() < 0. {
                    match spec.cull {
                        resonance_content::CullFace::Back => resonance_content::CullFace::Front,
                        resonance_content::CullFace::Front => resonance_content::CullFace::Back,
                        cull => cull,
                    }
                } else {
                    spec.cull
                };
                surface.uv_offsets = layer
                    .preview
                    .uv_offsets
                    .get(index)
                    .copied()
                    .map(Vec4::from_array)
                    .unwrap_or_default();
                if let Some(animation) = &layer.animation {
                    let (texture, uv) = texture_offset(animation, age);
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

#[allow(clippy::type_complexity)]
pub(super) fn dynamics(
    state: State,
    cache: Res<EquipmentCache>,
    parents: Query<&ChildOf>,
    mut roots: Query<(Entity, &mut Instance), With<Loaded>>,
    mut transforms: ParamSet<(
        super::sparse_animation::affine::Helper,
        (Query<&mut Transform>, ResMut<Locals>),
    )>,
) {
    let field = state.get();
    let mut poses = BTreeMap::new();
    let mut locals = Vec::new();
    {
        let helper = transforms.p0();
        for (entity, mut root) in &mut roots {
            let model = &cache.equipment[&root.item];
            let layer = &model.layers[root.layer];
            if layer.art.spec.secondary_motion.is_empty() {
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
            // Inverted hulls consume the primary layer's solved joints.
            if model.layers[..root.layer]
                .iter()
                .any(|other| other.part == layer.part)
            {
                continue;
            }
            let actor_id = root.actor;
            let Some(rig) = &mut root.dynamics else {
                continue;
            };
            if let Some(actor) = field.events.world.actors.get(&actor_id) {
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
            if let Some(pose) = rig.advance(
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
            ) {
                poses.insert((root.owner, root.item, root.slot), pose);
            }
        }
        for (_, root) in &roots {
            if let Some(rig) = &root.dynamics
                && let Some(pose) = poses.get(&(root.owner, root.item, root.slot))
            {
                locals.extend(rig.locals(&helper, pose));
            }
        }
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
