//! Model particles share field assets, but have separate instances and draw state.
use super::{
    draw_order::{DrawOrder, MODEL_EFFECTS},
    field_audit::{Applied, Request},
    field_effects::effect_rotation,
    field_view::{Art, State},
    materials::{MaterialSlot, TitleSurface},
    scene::SampledImages,
};
use bevy::{ecs::entity::EntityHashMap, prelude::*};
use resonance_events::effect::{Blend, SpriteOrientation};
use std::collections::BTreeSet;

pub(super) fn material(surface: &mut TitleSurface, blend: Blend) {
    surface.blend = Some(blend);
    surface.cull = resonance_content::CullFace::None;
    surface.depth_write = false;
    surface.clamp_color = true;
    surface.vertex_alpha = true;
}

#[derive(Component)]
pub(super) struct Part {
    handle: i32,
    resource: u32,
    index: usize,
    prepared: bool,
    materials: Vec<Handle<TitleSurface>>,
}

impl Part {
    pub(super) fn prepared(&self) -> bool {
        self.prepared
    }
}

fn instantiate(world: &mut World, root: Entity, scene: &Handle<WorldAsset>) {
    // All field models are loaded before play. Instantiate short-lived effects
    // immediately so their lifetime cannot run out while their scene is queued.
    world.resource_scope(|world, scenes: Mut<Assets<WorldAsset>>| {
        let registry = world.resource::<AppTypeRegistry>().clone();
        let mut entities = EntityHashMap::default();
        scenes
            .get(scene)
            .expect("loaded model particle scene")
            .write_to_world_with(world, &mut entities, &registry)
            .expect("model particle scene components");
        for entity in entities.values() {
            if world.get::<ChildOf>(*entity).is_none() {
                world.entity_mut(*entity).insert(ChildOf(root));
            }
        }
    });
}

pub(super) fn spawn(
    state: State,
    art: Res<Art>,
    mut commands: Commands,
    roots: Query<&Part>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut images: ResMut<Assets<Image>>,
    mut sampled: ResMut<SampledImages>,
) {
    if !art.ready {
        return;
    }
    let retained: BTreeSet<_> = roots.iter().map(|part| (part.handle, part.index)).collect();
    for (&handle, particle) in &state.get().events.world.model_particles {
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
            let root = commands
                .spawn((
                    Transform::default(),
                    Visibility::Hidden,
                    Part {
                        handle,
                        resource: particle.resource,
                        index,
                        prepared: false,
                        materials,
                    },
                ))
                .id();
            let scene = part.scene.clone();
            commands.queue(move |world: &mut World| instantiate(world, root, &scene));
        }
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
    (
        &'static mut Transform,
        &'static ChildOf,
        Option<&'static bevy::gltf::GltfExtras>,
    ),
    Without<Part>,
>;

fn orient_model_roots(
    root: Entity,
    rotation: Quat,
    children: &Query<&Children>,
    nodes: &mut ModelNodes,
) {
    // Face the camera inside the particle's transform, so its stretch stays
    // on world axes. Authored child transforms retain multipart layouts.
    for child in children.iter_descendants(root) {
        let Ok((_, parent, Some(_))) = nodes.get(child) else {
            continue;
        };
        if !nodes
            .get(parent.parent())
            .is_ok_and(|(_, _, extras)| extras.is_some())
        {
            *nodes.get_mut(child).unwrap().0 = Transform::IDENTITY;
        }
    }
    for &child in children.get(root).into_iter().flatten() {
        if let Ok((mut transform, _, _)) = nodes.get_mut(child) {
            transform.rotation = rotation;
        }
    }
}

#[allow(clippy::too_many_arguments)] // Shared assets and independent scene instances.
pub(super) fn sync(
    state: State,
    art: Res<Art>,
    mut commands: Commands,
    mut roots: Query<(Entity, &mut Part, &mut Transform, &mut Visibility)>,
    mut nodes: ModelNodes,
    children: Query<&Children>,
    slots: Query<&MaterialSlot>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut applied: ResMut<Applied>,
) {
    if !art.ready {
        return;
    }
    let world = &state.get().events.world;
    let camera = world
        .field_camera
        .as_ref()
        .map_or(Quat::IDENTITY, |camera| {
            super::field_view::camera_transform(camera).rotation
        });
    for (entity, mut part, mut transform, mut visibility) in &mut roots {
        let Some(particle) = world
            .model_particles
            .get(&part.handle)
            .filter(|p| p.resource == part.resource)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        let model = &art.models[&part.resource][part.index];
        *visibility = if world.tick >= particle.born {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let request = Request::ModelParticle(part.handle, part.index);
        orient_model_roots(
            entity,
            effect_rotation(particle.orientation, [0.; 3], camera),
            &children,
            &mut nodes,
        );
        if !part.prepared {
            for child in children.iter_descendants(entity) {
                let Ok(slot) = slots.get(child) else { continue };
                let index = slot
                    .index(part.materials.len())
                    .expect("model particle material slot");
                let spec = &model.spec.materials[index];
                if spec.image == resonance_content::SceneImage::Capture {
                    commands
                        .entity(child)
                        .remove::<MeshMaterial3d<StandardMaterial>>()
                        .insert(super::field_capture::Part {
                            request: request.clone(),
                            particle: part.handle,
                            vertex_color: spec.vertex_color,
                        });
                    continue;
                }
                commands.entity(child).insert((
                    MeshMaterial3d(part.materials[index].clone()),
                    DrawOrder(MODEL_EFFECTS + spec.draw_order, part.handle as usize),
                ));
            }
            part.prepared = true;
        }
        transform.translation = Vec3::from_array(particle.position);
        transform.scale = Vec3::from_array(particle.scale);
        transform.rotation = effect_rotation(SpriteOrientation::World, particle.rotation, camera);
        let brightness = world.brightness();
        let tint = Vec4::new(
            brightness,
            brightness,
            brightness,
            f32::from(particle.rgba[3]) / 255.,
        );
        let ambient_color = Vec4::from_array(particle.rgba.map(f32::from));
        for handle in &part.materials {
            let material = surfaces
                .get(handle)
                .expect("retained model particle material");
            if material.tint != tint
                || material.ambient_color != ambient_color
                || material.blend != Some(particle.blend)
            {
                let mut material = surfaces.get_mut(handle).unwrap();
                material.tint = tint;
                material.ambient_color = ambient_color;
                material.blend = Some(particle.blend);
            }
        }
        let captured = model
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::RunSystemOnce, transform::helper::TransformHelper};

    #[test]
    fn loaded_particle_scene_is_available_in_its_birth_update() {
        let mut app = App::new();
        app.init_resource::<Assets<WorldAsset>>()
            .register_type::<Transform>()
            .register_type::<GlobalTransform>()
            .register_type::<ChildOf>()
            .register_type::<Children>()
            .register_type::<MaterialSlot>();
        let mut scene = World::new();
        let parent = scene.spawn(Transform::from_xyz(3., 0., 0.)).id();
        scene.spawn((ChildOf(parent), Transform::IDENTITY, MaterialSlot(0)));
        let scene = app
            .world_mut()
            .resource_mut::<Assets<WorldAsset>>()
            .add(WorldAsset::new(scene));
        let root = app.world_mut().spawn_empty().id();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                let scene = scene.clone();
                commands.queue(move |world: &mut World| instantiate(world, root, &scene));
            })
            .unwrap();
        let parent = app.world().get::<Children>(root).unwrap()[0];
        let mesh = app.world().get::<Children>(parent).unwrap()[0];
        assert_eq!(
            app.world().get::<MaterialSlot>(mesh),
            Some(&MaterialSlot(0))
        );
        assert_eq!(
            app.world().get::<Transform>(parent).unwrap().translation.x,
            3.
        );
        app.world_mut().despawn(root);
        assert!(app.world().get_entity(mesh).is_err());
    }

    #[test]
    fn particle_placement_preserves_layout_and_stretches_on_world_axes() {
        for (rotation, expected) in [
            (Quat::IDENTITY, Vec3::new(13., 20., 30.)),
            (
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                Vec3::new(10., 20., 18.),
            ),
        ] {
            let mut world = World::new();
            let particle = world
                .spawn(Transform::from_xyz(10., 20., 30.).with_scale(Vec3::new(0.5, 0.5, 2.)))
                .id();
            let scene = world.spawn((ChildOf(particle), Transform::IDENTITY)).id();
            let model = world
                .spawn((
                    ChildOf(scene),
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
                    orient_model_roots(particle, rotation, &children, &mut nodes);
                })
                .unwrap();
            world
                .run_system_once(move |transforms: TransformHelper| {
                    let transform = transforms.compute_global_transform(child).unwrap();
                    assert!(
                        transform
                            .transform_point(Vec3::X * 2.)
                            .abs_diff_eq(expected, 0.0001)
                    );
                })
                .unwrap();
        }
    }
}
