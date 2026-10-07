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
    surface.cull = resonance_content::CullFace::None;
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

type ModelNodes<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static ChildOf),
    (With<bevy::gltf::GltfExtras>, Without<Part>),
>;

fn reset_model_roots(root: Entity, children: &Query<&Children>, nodes: &mut ModelNodes) {
    // Particle placement replaces the model's authored root transform.
    // Keep child transforms so multipart effects retain their shape.
    for child in children.iter_descendants(root) {
        let Ok((_, parent)) = nodes.get(child) else {
            continue;
        };
        if !nodes.contains(parent.parent()) {
            *nodes.get_mut(child).unwrap().0 = Transform::IDENTITY;
        }
    }
}

#[allow(clippy::too_many_arguments)] // Shared assets and independent scene instances.
pub(super) fn sync(
    state: State,
    art: Res<Art>,
    mut commands: Commands,
    mut roots: Query<(Entity, &mut Part, &mut Transform)>,
    mut nodes: ModelNodes,
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
            reset_model_roots(entity, &children, &mut nodes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::RunSystemOnce, transform::helper::TransformHelper};

    #[test]
    fn particle_placement_preserves_child_layout_without_scaling_twice() {
        let mut world = World::new();
        let particle = world
            .spawn(Transform::from_xyz(10., 20., 30.).with_scale(Vec3::splat(0.5)))
            .id();
        let model = world
            .spawn((
                ChildOf(particle),
                bevy::gltf::GltfExtras { value: "{}".into() },
                Transform::from_xyz(100., 0., 0.).with_scale(Vec3::splat(3.)),
            ))
            .id();
        let child = world
            .spawn((
                ChildOf(model),
                bevy::gltf::GltfExtras { value: "{}".into() },
                Transform::from_xyz(4., 0., 0.),
            ))
            .id();
        world
            .run_system_once(move |children: Query<&Children>, mut nodes: ModelNodes| {
                reset_model_roots(particle, &children, &mut nodes);
            })
            .unwrap();
        world
            .run_system_once(move |transforms: TransformHelper| {
                let transform = transforms.compute_global_transform(child).unwrap();
                assert_eq!(
                    transform.transform_point(Vec3::X * 2.),
                    Vec3::new(13., 20., 30.)
                );
            })
            .unwrap();
    }
}
