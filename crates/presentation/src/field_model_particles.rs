//! Model particles share field assets, but have separate instances and draw state.
use super::{
    draw_order::{DrawOrder, EFFECTS},
    field_audit::{Applied, Request},
    field_view::{Art, State},
    materials::{MaterialSlot, TitleSurface},
    scene::SampledImages,
};
use bevy::{prelude::*, world_serialization::WorldInstanceReady};
use resonance_events::model_particle::Blend;
use std::collections::BTreeSet;

pub(super) fn material(surface: &mut TitleSurface, blend: Blend) {
    surface.blend = true;
    surface.depth_write = false;
    surface.additive = blend == Blend::Additive;
    surface.subtractive = blend == Blend::Subtractive;
}

#[derive(Component)]
pub(super) struct Part {
    handle: i32,
    resource: u32,
    index: usize,
    phase: Phase,
    materials: Vec<Handle<TitleSurface>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Instantiating,
    BindingMaterials,
    Ready,
}

impl Part {
    pub(super) fn prepared(&self) -> bool {
        self.phase == Phase::Ready
    }
}

pub(super) fn retire(world: &mut World) {
    let roots: Vec<_> = world
        .query_filtered::<Entity, With<Part>>()
        .iter(world)
        .collect();
    for root in roots {
        world.despawn(root);
    }
}

#[allow(clippy::too_many_arguments)] // Shared assets and independent scene instances.
pub(super) fn sync(
    state: State,
    art: Res<Art>,
    mut commands: Commands,
    mut roots: Query<(Entity, &mut Part, &mut Transform)>,
    children: Query<&Children>,
    slots: Query<&MaterialSlot>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut images: ResMut<Assets<Image>>,
    mut sampled: ResMut<SampledImages>,
    mut applied: ResMut<Applied>,
) {
    if !art.ready {
        return;
    }
    let world = &state.get().events.world;
    let mut retained = BTreeSet::new();
    for (entity, mut part, mut transform) in &mut roots {
        let Some(particle) = world
            .model_particles
            .get(&part.handle)
            .filter(|p| p.resource == part.resource)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        retained.insert((part.handle, part.index));
        let request = Request::ModelParticle(part.handle, part.index);
        if part.phase == Phase::Instantiating {
            applied.loading(request);
            continue;
        }
        if part.phase == Phase::BindingMaterials {
            for child in children.iter_descendants(entity) {
                let Ok(slot) = slots.get(child) else { continue };
                let index = slot
                    .index(part.materials.len())
                    .expect("model particle material slot");
                let spec = &art.models[&part.resource][part.index].spec.materials[index];
                if spec.image == resonance_content::SceneImage::Capture {
                    commands
                        .entity(child)
                        .remove::<MeshMaterial3d<StandardMaterial>>()
                        .insert(super::field_capture::Part {
                            request: request.clone(),
                            particle: part.handle,
                            vertex_color: spec.vertex_color,
                        });
                    part.phase = Phase::Ready;
                    continue;
                }
                commands.entity(child).insert((
                    MeshMaterial3d(part.materials[index].clone()),
                    DrawOrder(
                        EFFECTS
                            + art.models[&part.resource][part.index].spec.materials[index]
                                .draw_order,
                        part.handle as usize,
                    ),
                ));
                part.phase = Phase::Ready;
            }
        }
        transform.translation = Vec3::from_array(particle.position);
        transform.scale = Vec3::from_array(particle.scale);
        transform.rotation = Quat::from_euler(
            EulerRot::ZYX,
            particle.rotation[2].to_radians(),
            particle.rotation[1].to_radians(),
            particle.rotation[0].to_radians(),
        );
        if matches!(
            particle.orientation,
            resonance_events::effect::SpriteOrientation::Camera
        ) {
            transform.rotation = world
                .field_camera
                .as_ref()
                .map_or(Quat::IDENTITY, |camera| {
                    super::field_view::camera_transform(camera).rotation
                })
                * transform.rotation;
        }
        let brightness = if particle.field_lighting {
            world.brightness()
        } else {
            1.
        };
        let tint = Vec4::from_array(particle.rgba.map(|v| f32::from(v) / 255.))
            * Vec4::new(4. * brightness, 4. * brightness, 4. * brightness, 1.);
        for handle in &part.materials {
            let material = surfaces
                .get(handle)
                .expect("retained model particle material");
            let additive = particle.blend == Blend::Additive;
            let subtractive = particle.blend == Blend::Subtractive;
            if material.tint != tint
                || material.additive != additive
                || material.subtractive != subtractive
            {
                let mut material = surfaces.get_mut(handle).unwrap();
                material.tint = tint;
                material.additive = additive;
                material.subtractive = subtractive;
            }
        }
        let captured = art.models[&part.resource][part.index]
            .spec
            .materials
            .iter()
            .any(|m| m.image == resonance_content::SceneImage::Capture);
        if part.prepared() && !captured {
            applied.ack(request);
        } else {
            applied.loading(request);
        }
    }
    for (&handle, particle) in &world.model_particles {
        let Some(parts) = art.models.get(&particle.resource) else {
            continue;
        };
        for (index, part) in parts.iter().enumerate() {
            if retained.contains(&(handle, index)) {
                continue;
            }
            let materials = part
                .surfaces(&mut images, &mut sampled)
                .into_iter()
                .map(|mut surface| {
                    material(&mut surface, particle.blend);
                    surfaces.add(surface)
                })
                .collect();
            commands
                .spawn((
                    WorldAssetRoot(part.scene.clone()),
                    Transform::default(),
                    Part {
                        handle,
                        resource: particle.resource,
                        index,
                        phase: Phase::Instantiating,
                        materials,
                    },
                ))
                .observe(
                    |event: On<WorldInstanceReady>, mut parts: Query<&mut Part>| {
                        if let Ok(mut part) = parts.get_mut(event.entity) {
                            part.phase = Phase::BindingMaterials;
                        }
                    },
                );
            applied.loading(Request::ModelParticle(handle, index));
        }
    }
}
