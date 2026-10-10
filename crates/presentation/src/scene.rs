use super::Events;
use super::draw_order::DrawOrder;
use super::materials::{MaterialSlot, TitleSurface};
use bevy::{
    image::{
        ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler,
        ImageSamplerDescriptor,
    },
    prelude::*,
};
use resonance_content::ANIMATION_HZ;
use resonance_content::field::{
    FieldTextureAnimation, MODEL_DRAW_SPAN, SCENERY_RESOURCE_BASE, scenery_actor,
};
use resonance_content::{SceneClip, TextureBinding, TextureWrap};
use resonance_events::effect::Blend;
use std::sync::Arc;

#[derive(Component)]
pub(super) struct AnimatedPart {
    resource: u32,
    autoplay: bool,
    clips: Vec<Handle<super::sparse_animation::Clip>>,
    bone_names: Vec<String>,
    binding: Option<super::sparse_animation::Binding>,
    disabled: bool,
    schedule: Vec<SceneClip>,
}

#[derive(Component)]
pub(super) struct Instantiated;

/// Every title scene instance, including static parts sharing the same GLB.
#[derive(Component)]
pub(super) struct PartRoot;

struct PendingSurface {
    root: Entity,
    resource: u32,
    slot: usize,
    color: Option<(Handle<Image>, TextureBinding)>,
    multiply: Option<(Handle<Image>, TextureBinding)>,
    vertex_color: bool,
    blend: bool,
    depth_write: bool,
    draw_order: u32,
    textures: [Option<usize>; 2],
    texture_actor: i32,
}

struct SurfaceBinding {
    root: Entity,
    resource: u32,
    slot: usize,
    surface: Handle<TitleSurface>,
    depth_write: bool,
    draw_order: u32,
    textures: [Option<usize>; 2],
    texture_actor: i32,
}

#[derive(Resource, Default)]
pub(super) struct FieldAssets {
    scenes: Vec<Handle<WorldAsset>>,
    pending: Vec<PendingSurface>,
    materials: Vec<SurfaceBinding>,
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
        // Title actors render in script actor order, between opaque and translucent scenery.
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
        for (order, (resource, texture_actor, part)) in parts.enumerate() {
            let handle = server.load(GltfAssetLabel::Scene(0).from_asset(part.mesh.clone()));
            self.scenes.push(handle.clone());
            let mut root = commands.spawn((
                PartRoot,
                WorldAssetRoot(handle),
                Transform::from_translation(Vec3::from_array(part.translation)),
            ));
            root.observe(
                |event: On<bevy::world_serialization::WorldInstanceReady>,
                 mut commands: Commands| {
                    commands.entity(event.entity).insert(Instantiated);
                },
            );
            if !part.clips.is_empty() {
                root.insert(AnimatedPart {
                    resource,
                    autoplay: part.autoplay,
                    clips: part
                        .clips
                        .iter()
                        .map(|clip| server.load(clip.motion.clone()))
                        .collect(),
                    bone_names: part.bone_names.clone(),
                    binding: None,
                    disabled: false,
                    schedule: part.clips.clone(),
                });
            }
            let load = |binding: &TextureBinding| {
                let image = server
                    .load_builder()
                    .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                    .load(part.textures[binding.texture].clone());
                (image, binding.clone())
            };
            for (index, material) in part.materials.iter().enumerate() {
                self.pending.push(PendingSurface {
                    root: root.id(),
                    resource,
                    slot: index,
                    color: material.color.as_ref().map(load),
                    multiply: material.multiply.as_ref().map(load),
                    vertex_color: material.vertex_color,
                    blend: material.blend,
                    depth_write: material.depth_write,
                    draw_order: order as u32 * MODEL_DRAW_SPAN
                        + material.draw_order % MODEL_DRAW_SPAN,
                    textures: [&material.color, &material.multiply]
                        .map(|binding| binding.as_ref().map(|binding| binding.texture)),
                    texture_actor,
                });
            }
        }
    }
}

pub(super) fn bind_animated(
    mut roots: Query<(Entity, &mut AnimatedPart), With<Instantiated>>,
    children: Query<&Children>,
    nodes: Query<(&Transform, &bevy::gltf::GltfExtras)>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
    diagnostics: Option<Res<super::diagnostics::Diagnostics>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let policy = diagnostics.map_or_else(
        || resonance_content::diagnostics::Diagnostics::new(true),
        |d| d.0.clone(),
    );
    for (root, mut part) in &mut roots {
        if part.disabled
            || part.binding.is_some()
            || !part.clips.iter().all(|clip| clips.contains(clip))
        {
            continue;
        }
        let result = (|| -> anyhow::Result<_> {
            for clip in &part.clips {
                clips
                    .get(clip)
                    .unwrap()
                    .0
                    .validate_bones(part.bone_names.len())?;
            }
            super::sparse_animation::Binding::new(root, part.bone_names.len(), &children, &nodes)
        })();
        match result {
            Ok(binding) => part.binding = Some(binding),
            Err(error) => {
                if policy.report("title animation binding", error).is_err() {
                    exit.write(AppExit::error());
                    return;
                }
                part.disabled = true;
                commands.entity(root).insert(Visibility::Hidden);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // Title animation and the session error policy.
pub(super) fn animate_field(
    events: Option<Res<Events>>,
    mut roots: Query<(Entity, &mut AnimatedPart)>,
    mut transforms: Query<&mut Transform>,
    mut affine: ResMut<super::sparse_animation::affine::Locals>,
    mut visibility: Query<&mut Visibility>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
    diagnostics: Option<Res<super::diagnostics::Diagnostics>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(events) = events else {
        return;
    };
    let policy = diagnostics.map_or_else(
        || resonance_content::diagnostics::Diagnostics::new(true),
        |d| d.0.clone(),
    );
    for (root, mut part) in &mut roots {
        if part.disabled {
            continue;
        }
        let result = (|| -> anyhow::Result<()> {
            let Some(binding) = &part.binding else {
                return Ok(());
            };
            let mut visibility = visibility.get_mut(root)?;
            let (clip, elapsed, repeat) = if part.autoplay {
                (0, events.0.tick() as f32 / ANIMATION_HZ, true)
            } else {
                let Some(actor) = events.0.world.actor_for_resource(part.resource) else {
                    *visibility = Visibility::Hidden;
                    return Ok(());
                };
                *visibility = if actor.visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                transforms.get_mut(root)?.translation = Vec3::from_array(actor.position);
                let Some(animation) = &actor.animation else {
                    return Ok(());
                };
                let clip = part
                    .schedule
                    .iter()
                    .position(|c| c.resource_slot == animation.slot)
                    .ok_or_else(|| {
                        anyhow::anyhow!("missing title animation slot {}", animation.slot)
                    })?;
                // Actor controllers start one update after event initialization;
                // static field groups advance immediately and have no such delay.
                let sampled = animation.sample(
                    events.0.tick(),
                    1,
                    part.schedule[clip].duration_seconds * ANIMATION_HZ,
                ) / ANIMATION_HZ;
                (clip, sampled, false)
            };
            let spec = &part.schedule[clip];
            // Sample the final key before wrapping the animation.
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
            let Some(clip) = clips.get(&part.clips[clip]) else {
                return Ok(());
            };
            binding.sample(&clip.0, time, &mut transforms, &mut affine)?;
            Ok(())
        })();
        if let Err(error) = result {
            if policy.report("title animation", error).is_err() {
                exit.write(AppExit::error());
                return;
            }
            part.disabled = true;
            if let Ok(mut visibility) = visibility.get_mut(root) {
                *visibility = Visibility::Hidden;
            }
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct SampledImages {
    images: std::collections::HashMap<SampleKey, std::sync::Weak<bevy::asset::StrongHandle>>,
    pub hits: u64,
    pub misses: u64,
}
type SampleKey = (AssetId<Image>, u8, u8, bool, bool);

#[cfg(test)]
#[path = "scene/sampled_tests.rs"]
mod sampled_tests;

pub(super) fn sampled_image(
    binding: Option<(Handle<Image>, TextureBinding)>,
    images: &mut Assets<Image>,
    cache: &mut SampledImages,
) -> Option<Handle<Image>> {
    binding.map(|(handle, binding)| {
        let key = (
            handle.id(),
            binding.wrap_u as u8,
            binding.wrap_v as u8,
            binding.nearest_min,
            binding.nearest_mag,
        );
        if let Some(cached) = cache.images.get(&key).and_then(std::sync::Weak::upgrade) {
            cache.hits += 1;
            return Handle::Strong(cached);
        }
        cache.misses += 1;
        cache.images.retain(|_, handle| handle.strong_count() > 0);
        // Different materials may sample the same image with different wrap
        // modes. A derived image prevents one loader setting winning globally.
        let mut image = images.get(&handle).expect("loaded texture").clone();
        // Scene shaders currently sample the base image. Keep that policy when
        // physical textures also retain their authored mip levels.
        let view = image.texture_view_descriptor.get_or_insert_default();
        view.base_mip_level = 0;
        view.mip_level_count = Some(1);
        let wrap = |mode| match mode {
            TextureWrap::Clamp => ImageAddressMode::ClampToEdge,
            TextureWrap::Repeat => ImageAddressMode::Repeat,
            TextureWrap::Mirror => ImageAddressMode::MirrorRepeat,
        };
        let filter = |nearest| {
            if nearest {
                ImageFilterMode::Nearest
            } else {
                ImageFilterMode::Linear
            }
        };
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: wrap(binding.wrap_u),
            address_mode_v: wrap(binding.wrap_v),
            min_filter: filter(binding.nearest_min),
            mag_filter: filter(binding.nearest_mag),
            ..ImageSamplerDescriptor::linear()
        });
        let derived = images.add(image);
        if let Handle::Strong(strong) = &derived {
            cache.images.insert(key, Arc::downgrade(strong));
        }
        derived
    })
}

#[allow(clippy::too_many_arguments)] // Scene dependencies, material instances and sampler cache.
pub(super) fn prepare_field(
    mut field: ResMut<FieldAssets>,
    server: Res<AssetServer>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut sampled: ResMut<SampledImages>,
    meshes: Query<(Entity, &MaterialSlot), Without<MeshMaterial3d<TitleSurface>>>,
    parents: Query<&ChildOf>,
    roots: Query<(Entity, Option<&WorldAssetRoot>), With<PartRoot>>,
    mut animated: Query<(Entity, &mut AnimatedPart)>,
    clips: Res<Assets<super::sparse_animation::Clip>>,
    mut exit: MessageWriter<AppExit>,
    diagnostics: Option<Res<super::diagnostics::Diagnostics>>,
) {
    let policy = diagnostics.map_or_else(
        || resonance_content::diagnostics::Diagnostics::new(true),
        |d| d.0.clone(),
    );
    if !field.ready {
        for (root, mut part) in &mut animated {
            if part.disabled {
                continue;
            }
            for clip in &part.clips {
                if let Some(bevy::asset::LoadState::Failed(error)) =
                    server.get_load_state(clip.id())
                {
                    if policy
                        .report("title animation load", anyhow::anyhow!("{error}"))
                        .is_err()
                    {
                        exit.write(AppExit::error());
                        return;
                    }
                    part.disabled = true;
                    commands.entity(root).insert(Visibility::Hidden);
                    break;
                }
            }
        }
        let mut failed_roots = std::collections::HashSet::new();
        for (root, scene) in &roots {
            let Some(scene) = scene else {
                continue;
            };
            let failure = match server.get_load_states(scene.0.id()) {
                Some(
                    (bevy::asset::LoadState::Failed(error), _, _)
                    | (_, _, bevy::asset::RecursiveDependencyLoadState::Failed(error)),
                ) => Some(error),
                _ => None,
            };
            if let Some(error) = failure {
                if policy
                    .report("title scene load", anyhow::anyhow!("{error}"))
                    .is_err()
                {
                    exit.write(AppExit::error());
                    return;
                }
                commands
                    .entity(root)
                    .insert(Visibility::Hidden)
                    .remove::<AnimatedPart>()
                    .remove::<WorldAssetRoot>();
                failed_roots.insert(root);
                field.scenes.retain(|handle| handle.id() != scene.0.id());
            }
        }
        field.pending.retain(|p| !failed_roots.contains(&p.root));
        if let Err(error) = super::battle_view::recover_images(
            field.pending.iter_mut().flat_map(|pending| {
                pending
                    .color
                    .iter_mut()
                    .chain(&mut pending.multiply)
                    .map(|(image, _)| image)
            }),
            &server,
            &mut images,
            &policy,
            "title scene texture",
        ) {
            error!("title texture preparation failed: {error:#}");
            exit.write(AppExit::error());
            return;
        }
        if !animated.iter().all(|(_, part)| {
            part.disabled
                || (part.binding.is_some() && part.clips.iter().all(|clip| clips.contains(clip)))
        }) || !field
            .scenes
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h.id()))
            || !field.pending.iter().all(|p| {
                p.color
                    .iter()
                    .chain(&p.multiply)
                    .all(|(h, _)| images.contains(h.id()))
            })
        {
            return;
        }
        let pending = std::mem::take(&mut field.pending);
        for p in pending {
            let surface = surfaces.add(TitleSurface {
                multiply: sampled_image(p.multiply, &mut images, &mut sampled),
                vertex_color: p.vertex_color,
                blend: p.blend.then_some(Blend::Alpha),
                depth_write: p.depth_write,
                ..TitleSurface::textured(sampled_image(p.color, &mut images, &mut sampled))
            });
            field.materials.push(SurfaceBinding {
                root: p.root,
                resource: p.resource,
                slot: p.slot,
                surface,
                depth_write: p.depth_write,
                draw_order: p.draw_order,
                textures: p.textures,
                texture_actor: p.texture_actor,
            });
        }
        field.ready = true;
    }
    // World assets can instantiate after their dependencies finish loading.
    if let Err(error) = bind_materials(&field, &mut commands, &meshes, &parents, &roots, &policy) {
        let _ = policy.report("title materials", error);
        exit.write(AppExit::error());
    }
}

fn bind_materials(
    field: &FieldAssets,
    commands: &mut Commands,
    meshes: &Query<(Entity, &MaterialSlot), Without<MeshMaterial3d<TitleSurface>>>,
    parents: &Query<&ChildOf>,
    roots: &Query<(Entity, Option<&WorldAssetRoot>), With<PartRoot>>,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
) -> anyhow::Result<()> {
    for (entity, material) in meshes {
        let Some(root) = parents
            .iter_ancestors(entity)
            .find(|&root| roots.contains(root))
        else {
            continue;
        };
        let Some(binding) = field
            .materials
            .iter()
            .find(|b| b.root == root && b.slot == material.0)
        else {
            diagnostics.report(
                "title material",
                anyhow::anyhow!("missing title material slot {}", material.0),
            )?;
            commands.entity(entity).insert(Visibility::Hidden);
            continue;
        };
        commands.entity(entity).insert((
            MeshMaterial3d(binding.surface.clone()),
            DrawOrder(crate::draw_order::Layer::Scene(binding.draw_order), 0, 0),
        ));
    }
    Ok(())
}

pub(super) fn update_materials(
    events: Option<Res<Events>>,
    field: Res<FieldAssets>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    for binding in &field.materials {
        let depth_write = binding.depth_write
            && events
                .as_ref()
                .and_then(|e| e.0.world.actor_for_resource(binding.resource))
                .is_none_or(|actor| actor.depth_write);
        let [color, multiply] = binding.textures.map(|texture| {
            let Some(events) = &events else {
                return [0.; 2];
            };
            let world = &events.0.world;
            field
                .texture_animations
                .iter()
                .enumerate()
                .filter(|(_, animation)| {
                    animation.actor.resolve(&world.texture_bindings) == binding.texture_actor
                        && texture.is_some_and(|texture| {
                            animation.texture.resolve(&world.texture_bindings) == texture as i32
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
        let uv_offsets = Vec4::new(color[0], color[1], multiply[0], multiply[1]);
        if surfaces
            .get(&binding.surface)
            .is_some_and(|s| s.depth_write != depth_write || s.uv_offsets != uv_offsets)
        {
            let mut surface = surfaces.get_mut(&binding.surface).unwrap();
            surface.depth_write = depth_write;
            surface.uv_offsets = uv_offsets;
        }
    }
}
