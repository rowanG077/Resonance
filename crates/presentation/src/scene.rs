use super::Events;
use super::draw_order::DrawOrder;
use super::materials::TitleSurface;
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
    graph: Handle<AnimationGraph>,
    nodes: Vec<AnimationNodeIndex>,
    schedule: Vec<SceneClip>,
}

struct PendingSurface {
    resource: u16,
    template: Handle<StandardMaterial>,
    color: Option<(Handle<Image>, TextureBinding)>,
    multiply: Option<(Handle<Image>, TextureBinding)>,
    blend: bool,
    depth_write: bool,
    draw_order: u32,
    uv_animations: [Option<Arc<TextureAnimation>>; 2],
}

struct SurfaceBinding {
    resource: u16,
    template: AssetId<StandardMaterial>,
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
    pub fn load(
        &mut self,
        scene: &TitleScene,
        server: &AssetServer,
        commands: &mut Commands,
        graphs: &mut Assets<AnimationGraph>,
    ) {
        for part in &scene.parts {
            let handle = server.load(GltfAssetLabel::Scene(0).from_asset(part.mesh.clone()));
            self.scenes.push(handle.clone());
            let mut root = commands.spawn((
                WorldAssetRoot(handle),
                Transform::from_translation(Vec3::from_array(part.translation)),
            ));
            if !part.clips.is_empty() {
                let (graph, nodes) = AnimationGraph::from_clips((0..part.clips.len()).map(|i| {
                    server.load(GltfAssetLabel::Animation(i).from_asset(part.mesh.clone()))
                }));
                root.insert(AnimatedPart {
                    resource: part.resource,
                    autoplay: part.autoplay,
                    graph: graphs.add(graph),
                    nodes,
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
            let animations: Vec<_> = part
                .texture_animations
                .iter()
                .cloned()
                .map(Arc::new)
                .collect();
            for (index, material) in part.materials.iter().enumerate() {
                self.pending.push(PendingSurface {
                    resource: part.resource,
                    template: server.load(format!("{}#Material{index}/std", part.mesh)),
                    color: material.color.as_ref().map(load),
                    multiply: material.multiply.as_ref().map(load),
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

pub(super) fn animate_field(
    events: Option<Res<Events>>,
    mut roots: Query<(Entity, &AnimatedPart, &mut Transform, &mut Visibility)>,
    children: Query<&Children>,
    mut players: Query<(&mut AnimationPlayer, Option<&AnimationGraphHandle>)>,
    mut commands: Commands,
) {
    let Some(events) = events else {
        return;
    };
    for (root, part, mut transform, mut visibility) in &mut roots {
        let (clip, elapsed, repeat) = if part.autoplay {
            (0, events.0.tick() as f32 / ANIMATION_HZ, true)
        } else {
            let Some(actor) = events.0.world.actor_for_resource(u32::from(part.resource)) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            *visibility = if actor.visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            transform.translation = Vec3::from_array(actor.position);
            let Some(animation) = &actor.animation else {
                continue;
            };
            let clip = part
                .schedule
                .iter()
                .position(|c| c.resource_slot == animation.slot)
                .expect("validated event animation slot");
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
        for entity in children.iter_descendants(root) {
            let Ok((mut player, graph)) = players.get_mut(entity) else {
                continue;
            };
            if graph.is_none() {
                commands
                    .entity(entity)
                    .insert(AnimationGraphHandle(part.graph.clone()));
            }
            for node in &part.nodes {
                if *node != part.nodes[clip] {
                    player.stop(*node);
                }
            }
            player.play(part.nodes[clip]).pause().set_seek_time(time);
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
    templates: Res<Assets<StandardMaterial>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
    mut sampled: ResMut<SampledImages>,
    meshes: Query<(Entity, &MeshMaterial3d<StandardMaterial>)>,
) {
    if !field.ready {
        if !field
            .scenes
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h.id()))
            || !field.pending.iter().all(|p| {
                templates.contains(p.template.id())
                    && p.color
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
                color: sampled_image(p.color, &mut images, &mut sampled),
                multiply: sampled_image(p.multiply, &mut images, &mut sampled),
                blend: p.blend,
                depth_write: p.depth_write,
                ..default()
            });
            field.materials.push(SurfaceBinding {
                resource: p.resource,
                template: p.template.id(),
                surface,
                depth_write: p.depth_write,
                draw_order: p.draw_order,
                uv_animations: p.uv_animations,
            });
        }
        field.ready = true;
    }
    // World assets can instantiate after their dependencies finish loading.
    for (entity, material) in &meshes {
        if let Some(binding) = field.materials.iter().find(|b| b.template == material.id()) {
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((
                    MeshMaterial3d(binding.surface.clone()),
                    DrawOrder(binding.draw_order),
                ));
        }
    }
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
