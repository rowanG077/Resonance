//! Immutable model layout, prepared once by the existing glTF loader.
//! Renderers can instantiate it without cloning a world or discovering bones.
use crate::materials::TitleSurface;
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::{
    asset::LoadContext,
    gltf::{
        GltfAssetLabel, GltfLoaderSettings,
        extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers},
        gltf,
    },
    image::ImageLoaderSettings,
    mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    prelude::*,
};
use resonance_content::{ScenePart, TextureBinding};
use resonance_events::effect::Blend;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

/// Source-world metadata is never copied into a live scene by reflection.
#[derive(Component)]
#[component(clone_behavior = Ignore)]
struct Model(Result<Arc<Data>, String>);

pub(super) fn prepared(scene: &WorldAsset) -> Result<Arc<Data>> {
    let mut query = scene
        .world
        .try_query::<&Model>()
        .context("scene layout missing")?;
    let model = query.single(&scene.world).context("scene layout missing")?;
    model
        .0
        .as_ref()
        .cloned()
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

pub(super) struct Data {
    pub nodes: Vec<Node>,
    pub meshes: Vec<Vec<Handle<Mesh>>>,
    pub bones: Vec<usize>,
    pub render: Vec<usize>,
    pub named: BTreeMap<String, usize>,
}
pub(super) struct Node {
    pub source: Entity,
    pub name: Option<Name>,
    pub transform: Transform,
    pub parent: Option<usize>,
    pub draw: Option<Draw>,
    pub bone: Option<usize>,
    pub suffix: Mat4,
    pub global: Mat4,
    pub camera_facing: bool,
}
pub(super) struct Draw {
    pub mesh: Handle<Mesh>,
    pub slot: crate::materials::MaterialSlot,
    pub skin: Option<Skin>,
}
pub(super) struct Skin {
    pub joints: Vec<usize>,
    pub inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
}

impl Data {
    pub fn check_bones(&self, count: usize) -> Result<()> {
        ensure!(
            count == 0 || count == self.bones.len(),
            "incomplete cooked animation skeleton: expected {count} bones, found {}",
            self.bones.len()
        );
        Ok(())
    }
    fn capture(world: &World, root: Entity, meshes: Vec<Vec<Handle<Mesh>>>) -> Result<Self> {
        profile_span!("model metadata");
        let mut query = world
            .try_query::<(Entity, &Transform)>()
            .context("model has no transforms")?;
        let sources: BTreeMap<_, _> = query
            .iter(world)
            .map(|(entity, transform)| (entity, *transform))
            .collect();
        let indices: HashMap<_, _> = sources
            .keys()
            .enumerate()
            .map(|(index, &entity)| (entity, index))
            .collect();
        let mut bones = BTreeMap::new();
        let mut nodes = Vec::new();
        for (source, transform) in sources {
            ensure!(transform.is_finite(), "invalid model transform");
            let index = indices[&source];
            let extras = world.get::<bevy::gltf::GltfExtras>(source);
            let mut camera_facing = false;
            if let Some(extras) = extras {
                let value: serde_json::Value = serde_json::from_str(&extras.value)?;
                camera_facing = value["resonance_camera_facing"] == true;
                if let Some(bone) = value.get("resonance_bone") {
                    let bone =
                        usize::try_from(bone.as_u64().context("invalid cooked bone index")?)?;
                    ensure!(
                        bone <= usize::from(u16::MAX),
                        "cooked bone index exceeds native range"
                    );
                    ensure!(
                        bones.insert(bone, index).is_none(),
                        "duplicate cooked bone index"
                    );
                }
            }
            let parent = world
                .get::<ChildOf>(source)
                .map(|parent| {
                    indices
                        .get(&parent.parent())
                        .copied()
                        .context("model has an unbound parent")
                })
                .transpose()?;
            ensure!(
                world
                    .get::<crate::materials::MaterialSlot>(source)
                    .is_none()
                    || world.get::<Mesh3d>(source).is_some(),
                "material slot has no mesh"
            );
            let draw = world
                .get::<Mesh3d>(source)
                .map(|mesh| -> Result<_> {
                    let slot = *world
                        .get::<crate::materials::MaterialSlot>(source)
                        .context("model mesh has no material slot")?;
                    slot.index(meshes.len())?;
                    let skin = world
                        .get::<SkinnedMesh>(source)
                        .map(|skin| -> Result<_> {
                            Ok(Skin {
                                inverse_bindposes: skin.inverse_bindposes.clone(),
                                joints: skin
                                    .joints
                                    .iter()
                                    .map(|joint| {
                                        indices
                                            .get(joint)
                                            .copied()
                                            .context("model has an unbound skin joint")
                                    })
                                    .collect::<Result<_>>()?,
                            })
                        })
                        .transpose()?;
                    Ok(Draw {
                        mesh: mesh.0.clone(),
                        slot,
                        skin,
                    })
                })
                .transpose()?;
            nodes.push(Node {
                source,
                name: world.get::<Name>(source).cloned(),
                transform,
                parent,
                draw,
                bone: None,
                suffix: Mat4::IDENTITY,
                global: Mat4::IDENTITY,
                camera_facing,
            });
        }
        ensure!(
            bones.keys().copied().eq(0..bones.len()),
            "incomplete cooked skeleton"
        );
        let bone_indices: HashMap<_, _> = bones.iter().map(|(&bone, &node)| (node, bone)).collect();
        let mut render = std::collections::BTreeSet::new();
        for index in 0..nodes.len() {
            if let Some(draw) = &nodes[index].draw {
                render.insert(index);
                if let Some(skin) = &draw.skin {
                    render.extend(&skin.joints);
                }
            }
            let (mut current, mut suffix, mut global, mut bone) =
                (Some(index), Mat4::IDENTITY, Mat4::IDENTITY, None);
            let mut visited = std::collections::BTreeSet::new();
            while let Some(parent) = current {
                ensure!(visited.insert(parent), "cyclic scene transform ancestry");
                if bone.is_none() {
                    bone = bone_indices.get(&parent).copied();
                    if bone.is_none() {
                        suffix = nodes[parent].transform.to_matrix() * suffix;
                    }
                }
                global = nodes[parent].transform.to_matrix() * global;
                current = nodes[parent].parent;
            }
            ensure!(
                global.is_finite() && suffix.is_finite(),
                "invalid scene transform matrix"
            );
            nodes[index].bone = bone;
            nodes[index].suffix = suffix;
            nodes[index].global = global;
        }
        ensure!(indices.contains_key(&root), "model scene root missing");
        // Match the existing breadth-first name resolution, excluding geometry wrappers.
        let mut named = BTreeMap::new();
        let mut pending = std::collections::VecDeque::from([root]);
        while let Some(entity) = pending.pop_front() {
            let children = world.get::<Children>(entity);
            pending.extend(children.into_iter().flat_map(|children| children.iter()));
            let node = &nodes[indices[&entity]];
            if node.draw.is_none()
                && node.parent.is_some()
                && !children.is_some_and(|children| {
                    children
                        .iter()
                        .any(|child| world.get::<Mesh3d>(child).is_some())
                })
                && let Some(name) = &node.name
            {
                named.insert(name.as_str().to_owned(), indices[&entity]);
            }
        }
        Ok(Self {
            nodes,
            meshes,
            bones: bones.into_values().collect(),
            render: render.into_iter().collect(),
            named,
        })
    }
}

#[derive(Clone, Default)]
struct Prepared {
    meshes: Vec<Vec<Handle<Mesh>>>,
}
impl GltfExtensionHandler for Prepared {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }
    fn on_root(
        &mut self,
        context: &mut LoadContext<'_>,
        gltf: &gltf::Gltf,
        _: &GltfLoaderSettings,
    ) {
        self.meshes = gltf
            .meshes()
            .map(|mesh| {
                mesh.primitives()
                    .map(|primitive| {
                        context.get_label_handle(
                            GltfAssetLabel::Primitive {
                                mesh: mesh.index(),
                                primitive: primitive.index(),
                            }
                            .to_string(),
                        )
                    })
                    .collect()
            })
            .collect();
    }
    fn on_scene_completed(
        &mut self,
        _: &mut LoadContext<'_>,
        _: &gltf::Scene<'_>,
        root: Entity,
        world: &mut World,
    ) {
        let data = Data::capture(world, root, self.meshes.clone())
            .map(Arc::new)
            .map_err(|error| format!("{error:#}"));
        world.entity_mut(root).insert(Model(data));
    }
}

pub(super) fn install(app: &mut App) {
    app.world_mut()
        .resource_mut::<GltfExtensionHandlers>()
        .0
        .write_blocking()
        .push(Box::new(Prepared::default()));
}

#[cfg(test)]
mod tests;

pub(super) struct Part {
    pub(super) spec: ScenePart,
    // Retain all scenes, clips, meshes and skins for the field's lifetime.
    // Resolve labeled assets only after this canonical model load finishes.
    pub(super) gltf: Handle<bevy::gltf::Gltf>,
    pub layout: Option<Arc<Data>>,
    pub(super) scene: Handle<WorldAsset>,
    pub(super) clips: Vec<Handle<super::sparse_animation::Clip>>,
    pub(super) motions: Vec<std::sync::Arc<resonance_content::animation::Motion>>,
    pub(super) materials: Vec<Surface>,
}
pub(super) struct Surface {
    pub(super) color: Option<(Handle<Image>, TextureBinding)>,
    pub(super) multiply: Option<(Handle<Image>, TextureBinding)>,
    pub(super) sampled: [Option<Handle<Image>>; 2],
}

/// Resolution owns sampler views alongside their source model. Every consumer
/// then borrows the same uploaded images, including models hidden by scripts.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct ModelAssets<'w> {
    pub server: Res<'w, AssetServer>,
    gltfs: Res<'w, Assets<bevy::gltf::Gltf>>,
    scenes: Res<'w, Assets<WorldAsset>>,
    clips: Res<'w, Assets<super::sparse_animation::Clip>>,
    pub images: ResMut<'w, Assets<Image>>,
    sampled: ResMut<'w, super::scene::SampledImages>,
}
impl Part {
    pub(super) fn load(
        spec: ScenePart,
        server: &AssetServer,
        loads: &super::loading::LoadTasks,
    ) -> Self {
        let gltf = server
            .load_builder()
            .with_guard(loads.ticket())
            .load(spec.mesh.clone());
        let load = |b: &TextureBinding| {
            (
                server
                    .load_builder()
                    .with_guard(loads.ticket())
                    .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                    .load(spec.textures[b.texture].clone()),
                b.clone(),
            )
        };
        let prepared = spec.prepared_motion_snapshot();
        let materials = spec
            .materials
            .iter()
            .map(|m| Surface {
                color: m.color.as_ref().map(load),
                multiply: m.multiply.as_ref().map(load),
                sampled: Default::default(),
            })
            .collect();
        Self {
            gltf,
            layout: None,
            scene: Handle::default(),
            clips: spec
                .clips
                .iter()
                .filter(|_| prepared.is_none())
                .map(|clip| {
                    server
                        .load_builder()
                        .with_guard(loads.ticket())
                        .load(clip.motion.clone())
                })
                .collect(),
            motions: prepared.unwrap_or_default(),
            materials,
            spec,
        }
    }

    pub(super) fn resolve(&mut self, assets: &mut ModelAssets) -> Result<bool> {
        profile_span!("model resolution");
        let ModelAssets {
            server,
            gltfs,
            scenes,
            clips,
            images,
            sampled,
        } = assets;
        if let Some(error) = self.load_error(server) {
            anyhow::bail!("{error}");
        }
        if !server.is_loaded_with_dependencies(self.gltf.id())
            || !self.clips.iter().all(|clip| clips.contains(clip))
            || !self.materials.iter().all(|m| {
                m.color
                    .iter()
                    .chain(&m.multiply)
                    .all(|(h, _)| images.contains(h.id()))
            })
        {
            return Ok(false);
        }
        if self.layout.is_none() {
            let gltf = gltfs.get(&self.gltf).context("loaded model missing")?;
            self.scene = gltf.scenes.first().context("model has no scene")?.clone();
            if !server.is_loaded_with_dependencies(self.scene.id()) {
                return Ok(false);
            }
            let layout = prepared(scenes.get(&self.scene).context("loaded scene missing")?)?;
            ensure!(
                layout.meshes.len() == self.materials.len(),
                "model material recipe differs: {}",
                self.spec.mesh
            );
            layout.check_bones(self.spec.bone_names.len())?;
            if self.motions.len() != self.spec.clips.len() {
                self.motions = self
                    .clips
                    .iter()
                    .zip(&self.spec.clips)
                    .map(|(handle, spec)| {
                        clips.get(handle).unwrap().bind(&self.spec.bone_names, spec)
                    })
                    .collect::<Result<_>>()?;
            }
            self.layout = Some(layout);
            for material in &mut self.materials {
                for (view, binding) in material
                    .sampled
                    .iter_mut()
                    .zip([material.color.clone(), material.multiply.clone()])
                {
                    if view.is_none() {
                        *view = super::scene::sampled_image(binding, images, sampled);
                    }
                }
            }
        }
        Ok(true)
    }

    pub(super) fn load_error(&self, server: &AssetServer) -> Option<String> {
        std::iter::once(self.gltf.id().untyped())
            .chain(self.clips.iter().map(|c| c.id().untyped()))
            .chain(self.materials.iter().flat_map(|m| {
                m.color
                    .iter()
                    .chain(&m.multiply)
                    .map(|(h, _)| h.id().untyped())
            }))
            .find_map(|id| match server.get_load_states(id) {
                Some((bevy::asset::LoadState::Failed(error), _, _))
                | Some((_, _, bevy::asset::RecursiveDependencyLoadState::Failed(error))) => {
                    Some(format!("{}: {error}", self.spec.mesh))
                }
                _ => None,
            })
    }

    pub(super) fn surfaces(&self) -> Vec<TitleSurface> {
        self.materials
            .iter()
            .zip(&self.spec.materials)
            .map(|(material, spec)| TitleSurface {
                field_fog: true,
                vertex_color: spec.vertex_color,
                alpha_cutoff: spec.alpha_cutoff,
                multiply: material.sampled[1].clone(),
                constant_color: self.spec.outline_color_for(spec).is_some(),
                blend: spec.blend.then_some(Blend::Alpha),
                depth_write: spec.depth_write,
                cull: spec.cull,
                tint: self.spec.outline_color_for(spec).map_or(Vec4::ONE, |c| {
                    Vec4::from_array(c.map(|v| f32::from(v) / 255.))
                }),
                ..TitleSurface::textured(material.sampled[0].clone())
            })
            .collect()
    }
}

/// Entity bindings belong to one live instance; immutable layout belongs to its scene.
#[derive(Component)]
pub(super) struct Instance {
    pub data: Arc<Data>,
    pub nodes: Vec<Entity>,
}
impl Instance {
    pub fn draws(&self) -> impl Iterator<Item = (Entity, &Draw)> {
        self.data
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((self.nodes[index], node.draw.as_ref()?)))
    }
    pub fn binding(&self, count: usize) -> Result<crate::sparse_animation::Binding> {
        self.data.check_bones(count)?;
        Ok(crate::sparse_animation::Binding(
            self.data
                .bones
                .iter()
                .take(count)
                .map(|&index| (self.nodes[index], self.data.nodes[index].transform))
                .collect(),
        ))
    }
    pub fn named_bones(&self) -> BTreeMap<String, (Entity, Transform, Entity)> {
        self.data
            .named
            .iter()
            .map(|(name, &index)| {
                let node = &self.data.nodes[index];
                (
                    name.clone(),
                    (
                        self.nodes[index],
                        node.transform,
                        self.nodes[node.parent.unwrap()],
                    ),
                )
            })
            .collect()
    }
    pub fn facing(&self) -> Vec<Entity> {
        self.data
            .bones
            .iter()
            .filter(|&&index| self.data.nodes[index].camera_facing)
            .map(|&index| self.nodes[index])
            .collect()
    }
}

/// Models are fully loaded before admission. Spawn synchronously within deferred
/// commands so even short-lived particles receive a complete instance immediately.
pub(super) fn instantiate(commands: &mut Commands, root: Entity, handle: Handle<WorldAsset>) {
    commands.queue(move |world: &mut World| {
        profile_span!("scene cloning");
        world.entity_mut(root).insert_if_new(Visibility::Inherited);
        let result = world.resource_scope(|world, scenes: Mut<Assets<WorldAsset>>| -> Result<()> {
            let scene = scenes
                .get(&handle)
                .context("prepared model scene missing")?;
            let data = prepared(scene)?;
            let registry = world.resource::<AppTypeRegistry>().clone();
            let mut entities = bevy::ecs::entity::EntityHashMap::default();
            scene.write_to_world_with(world, &mut entities, &registry)?;
            for &entity in entities.values() {
                if world.get::<ChildOf>(entity).is_none() {
                    world.entity_mut(entity).insert(ChildOf(root));
                }
            }
            let nodes = data
                .nodes
                .iter()
                .map(|node| entities[&node.source])
                .collect();
            world.entity_mut(root).insert(Instance { data, nodes });
            Ok(())
        });
        if let Err(error) = result {
            crate::diagnostics::fatal(world, "model instantiation", error);
        }
    });
}

#[cfg(all(test, feature = "preparation-trace"))]
#[path = "model_asset/tests/pipelines.rs"]
pub(super) mod pipeline_test;
