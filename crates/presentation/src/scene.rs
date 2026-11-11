use super::{Events, draw_order::DrawOrder, materials::TitleSurface, model_asset::Part};
use anyhow::Result;
use bevy::prelude::*;
use resonance_content::field::{
    FieldTextureAnimation, MODEL_DRAW_SPAN, SCENERY_RESOURCE_BASE, scenery_actor,
};
use resonance_content::{ANIMATION_HZ, TextureBinding, TextureWrap};
use resonance_events::effect::Blend;
use std::sync::Arc;

#[derive(Component)]
pub(super) struct AnimatedPart {
    index: usize,
    binding: super::sparse_animation::Binding,
}

/// Every title scene instance, including static parts sharing the same GLB.
#[derive(Component)]
pub(super) struct PartRoot;

struct TitlePart {
    art: Part,
    root: Entity,
    resource: u32,
    texture_actor: i32,
    order: u32,
    materials: Vec<Handle<TitleSurface>>,
    spawned: bool,
    bound: bool,
}

#[derive(Resource, Default)]
pub(super) struct FieldAssets {
    parts: Vec<TitlePart>,
    pub ready: bool,
    texture_animations: Vec<FieldTextureAnimation>,
}

impl FieldAssets {
    pub fn load<'a>(
        &mut self,
        scene: &'a resonance_content::field::FieldAssets,
        events: &resonance_events::EventRuntime,
        server: &AssetServer,
        commands: &mut Commands,
    ) {
        self.texture_animations
            .clone_from(&scene.texture_animations);
        let mut actors: Vec<_> = scene
            .actors
            .iter()
            .flat_map(|model| {
                let actor = events
                    .world
                    .actors
                    .iter()
                    .find(|(_, actor)| actor.resource == model.resource)
                    .map_or(i32::MAX, |(&id, _)| id);
                model
                    .parts
                    .iter()
                    .map(move |part| (model.resource, actor, part))
            })
            .collect();
        actors.sort_by_key(|(_, actor, _)| *actor);
        let scenery = |part: &'a resonance_content::ScenePart| {
            (
                SCENERY_RESOURCE_BASE + u32::from(part.resource),
                scenery_actor(part.resource),
                part,
            )
        };
        let parts = scene
            .parts
            .iter()
            .filter(|part| part.resource != 2)
            .map(scenery)
            .chain(actors)
            .chain(
                scene
                    .parts
                    .iter()
                    .filter(|part| part.resource == 2)
                    .map(scenery),
            );
        let loads = super::loading::LoadTasks::default();
        for (order, (resource, texture_actor, part)) in parts.enumerate() {
            let root = commands
                .spawn((
                    PartRoot,
                    Transform::from_translation(Vec3::from_array(part.translation)),
                ))
                .id();
            self.parts.push(TitlePart {
                art: Part::load(part.clone(), server, &loads),
                root,
                resource,
                texture_actor,
                order: order as u32 * MODEL_DRAW_SPAN,
                materials: Vec::new(),
                spawned: false,
                bound: false,
            });
        }
    }
}

pub(super) fn animate_field(
    events: Option<Res<Events>>,
    field: Res<FieldAssets>,
    roots: Query<(Entity, &AnimatedPart)>,
    mut transforms: Query<&mut Transform>,
    mut affine: ResMut<super::sparse_animation::affine::Locals>,
    mut visibility: Query<&mut Visibility>,
) {
    let Some(events) = events else { return };
    for (root, animation) in &roots {
        let part = &field.parts[animation.index];
        let spec = &part.art.spec;
        let mut visibility = visibility.get_mut(root).unwrap();
        let (clip, elapsed, repeat) = if spec.autoplay {
            (0, events.0.tick() as f32 / ANIMATION_HZ, true)
        } else {
            let Some(actor) = events.0.world.actor_for_resource(part.resource) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            *visibility = if actor.visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            transforms.get_mut(root).unwrap().translation = Vec3::from_array(actor.position);
            let Some(animation) = &actor.animation else {
                continue;
            };
            let clip = spec
                .clips
                .iter()
                .position(|c| c.resource_slot == animation.slot)
                .expect("validated event animation slot");
            let sampled = animation.sample(
                events.0.tick(),
                1,
                spec.clips[clip].duration_seconds * ANIMATION_HZ,
            ) / ANIMATION_HZ;
            (clip, sampled, false)
        };
        let spec = &spec.clips[clip];
        let time = if repeat && elapsed > spec.duration_seconds {
            let phase = elapsed % spec.duration_seconds;
            if phase == 0. {
                spec.duration_seconds
            } else {
                phase
            }
        } else {
            elapsed.min(spec.duration_seconds)
        };
        animation
            .binding
            .sample(&part.art.motions[clip], time, &mut transforms, &mut affine)
            .expect("validated title animation must evaluate");
    }
}

mod sampled;
#[cfg(test)]
#[path = "scene/sampled_tests.rs"]
mod sampled_tests;
pub(super) use sampled::install as install_sampled_images;
pub(super) use sampled::{SampledImages, sampled_image};

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_field(
    mut field: ResMut<FieldAssets>,
    mut commands: Commands,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut assets: super::model_asset::ModelAssets,
    instances: Query<&super::model_asset::Instance>,
    mut exit: crate::diagnostics::Fatal,
) {
    if field.ready {
        return;
    }
    let result = (|| -> Result<bool> {
        let mut ready = true;
        for (index, part) in field.parts.iter_mut().enumerate() {
            if part.bound {
                continue;
            }
            if !part.art.resolve(&mut assets)? {
                ready = false;
                continue;
            }
            if !part.spawned {
                for (material, spec) in part.art.materials.iter().zip(&part.art.spec.materials) {
                    part.materials.push(surfaces.add(TitleSurface {
                        multiply: material.sampled[1].clone(),
                        vertex_color: spec.vertex_color,
                        blend: spec.blend.then_some(Blend::Alpha),
                        depth_write: spec.depth_write,
                        ..TitleSurface::textured(material.sampled[0].clone())
                    }));
                }
                super::model_asset::instantiate(&mut commands, part.root, part.art.scene.clone());
                part.spawned = true;
            }
            let Ok(instance) = instances.get(part.root) else {
                ready = false;
                continue;
            };
            if !part.art.spec.clips.is_empty() {
                commands.entity(part.root).insert(AnimatedPart {
                    index,
                    binding: instance.binding(part.art.spec.bone_names.len())?,
                });
            }
            for (entity, draw) in instance.draws() {
                let slot = draw.slot.index(part.materials.len())?;
                commands.entity(entity).insert((
                    MeshMaterial3d(part.materials[slot].clone()),
                    DrawOrder(
                        crate::draw_order::Layer::Scene(
                            part.order + part.art.spec.materials[slot].draw_order % MODEL_DRAW_SPAN,
                        ),
                        0,
                        0,
                    ),
                ));
            }
            part.bound = true;
        }
        Ok(ready)
    })();
    match result {
        Ok(ready) => field.ready = ready,
        Err(error) => exit.fail("title field preparation", error),
    }
}

pub(super) fn update_materials(
    events: Option<Res<Events>>,
    field: Res<FieldAssets>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    for part in &field.parts {
        for (handle, spec) in part.materials.iter().zip(&part.art.spec.materials) {
            let depth_write = spec.depth_write
                && events
                    .as_ref()
                    .and_then(|e| e.0.world.actor_for_resource(part.resource))
                    .is_none_or(|actor| actor.depth_write);
            let [color, multiply] = [&spec.color, &spec.multiply].map(|binding| {
                let Some(events) = &events else {
                    return [0.; 2];
                };
                let world = &events.0.world;
                field
                    .texture_animations
                    .iter()
                    .enumerate()
                    .filter(|(_, animation)| {
                        animation.actor.resolve(&world.texture_bindings) == part.texture_actor
                            && binding.as_ref().is_some_and(|binding| {
                                animation.texture.resolve(&world.texture_bindings)
                                    == binding.texture as i32
                            })
                    })
                    .fold([0.; 2], |mut offset, (index, _)| {
                        let uv = world
                            .texture_animation_states
                            .get(index)
                            .map_or([0.; 2], |state| state.offset);
                        offset[0] += uv[0];
                        offset[1] += uv[1];
                        offset
                    })
            });
            let [color, multiply] =
                [(color, &spec.color), (multiply, &spec.multiply)].map(|(offset, binding)| {
                    binding.as_ref().map_or(offset, |binding| {
                        [0, 1].map(|axis| {
                            offset[axis] * binding.scale[axis][0] as f32
                                / binding.scale[axis][1] as f32
                        })
                    })
                });
            let uv_offsets = Vec4::new(color[0], color[1], multiply[0], multiply[1]);
            if surfaces
                .get(handle)
                .is_some_and(|s| s.depth_write != depth_write || s.uv_offsets != uv_offsets)
            {
                let mut surface = surfaces.get_mut(handle).unwrap();
                surface.depth_write = depth_write;
                surface.uv_offsets = uv_offsets;
            }
        }
    }
}
