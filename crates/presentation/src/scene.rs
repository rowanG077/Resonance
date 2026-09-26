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
use resonance_content::{SceneClip, TextureAnimation, TextureBinding, TextureWrap, TitleScene};
use std::sync::Arc;

#[derive(Component)]
pub(super) struct AnimatedPart {
    resource: u16,
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
    resource: u16,
    slot: usize,
    color: Option<(Handle<Image>, TextureBinding)>,
    multiply: Option<(Handle<Image>, TextureBinding)>,
    vertex_color: bool,
    blend: bool,
    depth_write: bool,
    draw_order: u32,
    uv_animations: [Option<Arc<TextureAnimation>>; 2],
}

struct SurfaceBinding {
    root: Entity,
    resource: u16,
    slot: usize,
    surface: Handle<TitleSurface>,
    depth_write: bool,
    draw_order: u32,
    uv_animations: [Option<Arc<TextureAnimation>>; 2],
}

#[derive(Resource, Default)]
pub(super) struct FieldAssets {
    scenes: Vec<Handle<WorldAsset>>,
    pending: Vec<PendingSurface>,
    materials: Vec<SurfaceBinding>,
    pub ready: bool,
}

impl FieldAssets {
    pub fn load(&mut self, scene: &TitleScene, server: &AssetServer, commands: &mut Commands) {
        for part in &scene.parts {
            let handle = server.load(GltfAssetLabel::Scene(0).from_asset(part.mesh.clone()));
            self.scenes.push(handle.clone());
            let mut root = commands.spawn((
                PartRoot,
                WorldAssetRoot(handle),
                Transform::from_translation(Vec3::from_array(part.translation)),
            ));
            if !part.clips.is_empty() {
                root.insert(AnimatedPart {
                    resource: part.resource,
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
                })
                .observe(
                    |event: On<bevy::world_serialization::WorldInstanceReady>,
                     mut commands: Commands| {
                        commands.entity(event.entity).insert(Instantiated);
                    },
                );
            }
            let load = |binding: &TextureBinding| {
                let image = server
                    .load_builder()
                    .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                    .load(part.textures[binding.texture].clone());
                (image, binding.clone())
            };
            let animations: Vec<_> = part
                .texture_animations
                .iter()
                .cloned()
                .map(Arc::new)
                .collect();
            for (index, material) in part.materials.iter().enumerate() {
                self.pending.push(PendingSurface {
                    root: root.id(),
                    resource: part.resource,
                    slot: index,
                    color: material.color.as_ref().map(load),
                    multiply: material.multiply.as_ref().map(load),
                    vertex_color: material.vertex_color,
                    blend: material.blend,
                    depth_write: material.depth_write,
                    draw_order: material.draw_order,
                    uv_animations: [&material.color, &material.multiply].map(|binding| {
                        binding.as_ref().and_then(|binding| {
                            animations
                                .iter()
                                .find(|animation| animation.texture == binding.texture)
                                .cloned()
                        })
                    }),
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
                let Some(actor) = events.0.world.actor_for_resource(u32::from(part.resource))
                else {
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
            // Original controllers sample their last key before wrapping.
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
            if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(scene.0.id())
            {
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
                    .remove::<AnimatedPart>();
                failed_roots.insert(root);
                field.scenes.retain(|handle| handle.id() != scene.0.id());
            }
        }
        field.pending.retain(|p| !failed_roots.contains(&p.root));
        for pending in &field.pending {
            for (handle, _) in pending.color.iter().chain(&pending.multiply) {
                if !images.contains(handle.id())
                    && let Some(bevy::asset::LoadState::Failed(error)) =
                        server.get_load_state(handle.id())
                {
                    if policy
                        .report("title scene texture", anyhow::anyhow!("{error}"))
                        .is_err()
                    {
                        exit.write(AppExit::error());
                        return;
                    }
                    if let Err(error) =
                        images.insert(handle.id(), super::battle_view::placeholder_image())
                    {
                        let _ = policy.report("title texture placeholder", error.into());
                        exit.write(AppExit::error());
                        return;
                    }
                }
            }
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
                blend: p.blend,
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
                uv_animations: p.uv_animations,
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
            DrawOrder(binding.draw_order, 0),
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
                .and_then(|e| e.0.world.actor_for_resource(u32::from(binding.resource)))
                .is_none_or(|actor| actor.depth_write);
        let tick = events.as_ref().map_or(0, |e| u64::from(e.0.tick()));
        let [color, multiply] = binding.uv_animations.each_ref().map(|animation| {
            animation
                .as_ref()
                .map_or([0.; 2], |animation| animation.offset(tick))
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

#[cfg(test)]
mod instance_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn tolerant_title_materials_hide_missing_slots_and_bind_the_remaining_mesh() {
        let mut world = World::new();
        let root = world.spawn(PartRoot).id();
        let meshes = [0, 1, 2].map(|slot| world.spawn((ChildOf(root), MaterialSlot(slot))).id());
        let diagnostics = resonance_content::diagnostics::Diagnostics::default();
        world.insert_resource(super::super::diagnostics::Diagnostics(diagnostics.clone()));
        world.insert_resource(FieldAssets {
            materials: vec![SurfaceBinding {
                root,
                resource: 0,
                slot: 2,
                surface: Handle::default(),
                depth_write: false,
                draw_order: 3,
                uv_animations: [None, None],
            }],
            ..Default::default()
        });
        world
            .run_system_once(
                |field: Res<FieldAssets>,
                 mut commands: Commands,
                 meshes: Query<(Entity, &MaterialSlot), Without<MeshMaterial3d<TitleSurface>>>,
                 parents: Query<&ChildOf>,
                 roots: Query<(Entity, Option<&WorldAssetRoot>), With<PartRoot>>,
                 policy: Res<super::super::diagnostics::Diagnostics>| {
                    bind_materials(&field, &mut commands, &meshes, &parents, &roots, &policy.0)
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(diagnostics.entries().len(), 2);
        for entity in &meshes[..2] {
            assert_eq!(world.get::<Visibility>(*entity), Some(&Visibility::Hidden));
            assert!(world.get::<MeshMaterial3d<TitleSurface>>(*entity).is_none());
        }
        assert!(
            world
                .get::<MeshMaterial3d<TitleSurface>>(meshes[2])
                .is_some()
        );
    }

    #[test]
    fn shared_mesh_materials_keep_each_static_instance_settings() {
        let mut world = World::new();
        let mut surfaces = Assets::<TitleSurface>::default();
        let slot = MaterialSlot(0);
        let mut field = FieldAssets::default();
        let mut instances = Vec::new();
        for order in [7, 31] {
            let root = world.spawn(PartRoot).id();
            let group = world.spawn(ChildOf(root)).id();
            let mesh = world.spawn((ChildOf(group), slot)).id();
            let surface = surfaces.add(TitleSurface {
                depth_write: order == 7,
                ..TitleSurface::textured(None)
            });
            instances.push((mesh, surface.clone(), order));
            field.materials.push(SurfaceBinding {
                root,
                resource: 0,
                slot: 0,
                surface,
                depth_write: order == 7,
                draw_order: order,
                uv_animations: [None, None],
            });
        }
        let unrelated = world.spawn(slot).id();
        world.insert_resource(field);
        world
            .run_system_once(
                |field: Res<FieldAssets>,
                 mut commands: Commands,
                 meshes: Query<(Entity, &MaterialSlot), Without<MeshMaterial3d<TitleSurface>>>,
                 parents: Query<&ChildOf>,
                 roots: Query<(Entity, Option<&WorldAssetRoot>), With<PartRoot>>| {
                    bind_materials(
                        &field,
                        &mut commands,
                        &meshes,
                        &parents,
                        &roots,
                        &resonance_content::diagnostics::Diagnostics::new(true),
                    )
                    .unwrap();
                },
            )
            .unwrap();
        for (mesh, surface, order) in instances {
            assert_eq!(
                world
                    .get::<MeshMaterial3d<TitleSurface>>(mesh)
                    .unwrap()
                    .id(),
                surface.id()
            );
            assert_eq!(world.get::<DrawOrder>(mesh).unwrap().0, order);
            assert_eq!(world.get::<MaterialSlot>(mesh), Some(&slot));
        }
        assert_eq!(world.get::<MaterialSlot>(unrelated), Some(&slot));
        assert!(
            world
                .get::<MeshMaterial3d<TitleSurface>>(unrelated)
                .is_none()
        );
    }
}
