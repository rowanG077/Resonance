//! Prepared battle geometry drawn from gameplay poses and scene-owned particles.
//! Model matrices already map cooked Z-up bones into battle Y-up coordinates.
use resonance_events::effect::Blend;
use resonance_game::battle::encounter::{RenderEffect, RenderInstances, RenderModel, RenderTrail};
mod effects;
mod model_particles;
pub(super) use model_particles::Stats as ModelParticleStats;
mod accessories;
mod scene_sample;
mod shadows;
mod stage_color;
mod trails;
use crate::{
    draw_order::{ActorLayer, DrawOrder, Layer},
    materials::{MaterialSlot, TitleSurface},
    model_preview::gpu,
};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{
        CameraProjection, RenderTarget, SubCameraView,
        visibility::{NoCpuCulling, NoFrustumCulling, RenderLayers},
    },
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d, tonemapping::Tonemapping},
    ecs::system::SystemParam,
    gltf::Gltf,
    image::{ImageLoaderSettings, ImageSampler},
    mesh::skinning::SkinnedMesh,
    prelude::*,
    render::{
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, PipelineCache, TextureFormat, TextureUsages},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
    },
    world_serialization::WorldInstanceReady,
};

use effects::ModelStyle;
use resonance_battle::{ActorId, BattleFrame, ModelMaterial, ParticleId};
use resonance_content::{
    animation::{Matrix, Skeleton},
    battle_profile::WeaponStyle,
    battle_stage::Stage,
    diagnostics::Diagnostics,
    model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, atomic::Ordering},
};

// Layer 28 belongs to the live Shift+Tab overlay.
pub(super) const LAYER: usize = 26;
pub(super) const WARM_LAYER: usize = 27;
// Scene cloning runs after these commands, so budget queued roots rather than enqueue time.
const PREPARATION_ROOTS_PER_UPDATE: usize = 64;

fn model_uv(rectangle: [i16; 4]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        if rectangle[axis + 2] > 0 {
            f32::from(rectangle[axis]) / f32::from(rectangle[axis + 2])
        } else {
            0.
        }
    })
}
fn weapon_layer(
    actor: usize,
    slot: Option<resonance_battle::WeaponSlot>,
    style: Option<WeaponStyle>,
    overlay: bool,
) -> Layer {
    // Secondary body layers draw after carried weapons within the same actor layer.
    let slot = slot.map_or(u8::MAX, |slot| slot.0);
    Layer::Actor(
        actor,
        if style.is_some_and(|style| style.before_body) {
            ActorLayer::BackWeapon(slot, overlay)
        } else {
            ActorLayer::FrontWeapon(slot, overlay)
        },
    )
}

fn apply_weapon_style(surface: &mut TitleSurface, style: Option<WeaponStyle>) {
    if let Some(style) = style {
        if !style.toon {
            surface.toon_ramp = None;
        }
        surface.depth_equal = true;
        if style.additive {
            surface.blend = Some(Blend::Additive);
            surface.depth_write = false;
            surface.cull = resonance_content::CullFace::None;
        }
    }
}

struct ActorOrder {
    actors: Vec<usize>,
    independent: Vec<usize>,
}
impl ActorOrder {
    fn new(frame: &BattleFrame, independent: &[[f32; 3]]) -> Result<Self> {
        let camera = frame.camera.context("battle frame has no camera")?;
        let direction = Vec3::from_array(camera.focus) - Vec3::from_array(camera.eye);
        let mut depths: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                (
                    index,
                    Vec3::from_array(actor.target_center()).dot(direction),
                )
            })
            .chain(independent.iter().enumerate().map(|(index, position)| {
                (
                    frame.actors.len() + index,
                    Vec3::from_array(*position).dot(direction),
                )
            }))
            .collect();
        // Later actors precede earlier actors when depths are equal.
        depths.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
        let mut order = vec![0; depths.len()];
        for (rank, (index, _)) in depths.into_iter().enumerate() {
            order[index] = rank;
        }
        let independent = order.split_off(frame.actors.len());
        Ok(Self {
            actors: order,
            independent,
        })
    }
    fn effect(&self, actor: Option<ActorId>, after: bool) -> Result<Layer> {
        Ok(if let Some(actor) = actor {
            Layer::Actor(
                self.get(actor)?,
                if after {
                    ActorLayer::Front
                } else {
                    ActorLayer::Behind
                },
            )
        } else {
            Layer::Effects
        })
    }

    fn independent(&self, index: usize) -> Result<usize> {
        self.independent
            .get(index)
            .copied()
            .context("battle draw references a missing independent model")
    }

    fn get(&self, actor: ActorId) -> Result<usize> {
        self.actors
            .get(actor.index())
            .copied()
            .context("battle draw references a missing actor")
    }
}

#[derive(Resource, Clone, Default)]
pub(super) struct Warmup(Arc<Mutex<Report>>);

#[derive(Default)]
struct Report {
    diagnostics: Option<Diagnostics>,
    discarded: BTreeSet<Entity>,
    draws: gpu::Report,
}

impl Report {
    fn tolerant(&self) -> bool {
        self.diagnostics.as_ref().is_some_and(|d| !d.paranoid())
    }

    fn recover(&mut self) {
        if self.tolerant()
            && let Some(error) = self.draws.error.take()
        {
            // The policy has already selected recovery; report cannot return Err here.
            let _ = self
                .diagnostics
                .as_ref()
                .unwrap()
                .report("battle GPU", anyhow::anyhow!(error));
        }
    }

    fn disarm(&mut self) {
        self.draws.armed = false;
    }

    fn check(&self) -> Result<()> {
        if let Some(error) = &self.draws.error {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}

pub(super) fn install(app: &mut App) {
    scene_sample::install(app);
    let warmup = Warmup::default();
    app.insert_resource(warmup.clone());
    app.sub_app_mut(bevy::render::RenderApp)
        .insert_resource(warmup)
        .add_systems(
            bevy::render::Render,
            rendered.in_set(bevy::render::RenderSystems::Cleanup),
        );
}

#[allow(clippy::too_many_arguments)] // Shared render fence and pipeline cache.
fn rendered(
    warmup: Res<Warmup>,
    scene_sample: Res<scene_sample::Pipeline>,
    resident: Res<crate::loading::Resident>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    quads: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let mut report = warmup.0.lock().unwrap();
    report.recover();
    if !report.draws.armed || report.draws.error.is_some() {
        return;
    }
    match scene_sample.ready(&cache) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            report.draws.error = Some(error.to_string());
            return;
        }
    }
    for (entity, pipeline) in crate::field_warm::draws(&phases, &quads) {
        if report.draws.expected.contains(&entity)
            && let CachedPipelineState::Err(error) = cache.get_render_pipeline_state(pipeline)
            && !crate::model_preview::gpu::shader_pending(error)
        {
            if report.tolerant() {
                let _ = report
                    .diagnostics
                    .as_ref()
                    .unwrap()
                    .report("battle draw pipeline", anyhow::anyhow!("{error}"));
                report.discarded.insert(entity.id());
            } else {
                report.draws.error = Some(error.to_string());
            }
        }
    }
    if report.tolerant() {
        let discarded = report.discarded.clone();
        report
            .draws
            .expected
            .retain(|entity| !discarded.contains(&entity.id()));
        if report.draws.expected.is_empty() {
            report.draws.completed.store(true, Ordering::Release);
        }
    }
    gpu::render_report(&mut report.draws, &phases, &quads, &cache, &device, &queue);
    let unprepared_reads = resident.unprepared_reads.load(Ordering::Relaxed);
    if unprepared_reads != 0 && report.draws.error.is_none() {
        report.draws.error = Some(format!(
            "battle attempted an undeclared asset read ({unprepared_reads})"
        ));
    }
    report.recover();
}

#[derive(Component)]
struct Instantiated;

#[derive(SystemParam)]
pub(super) struct AssetsForView<'w> {
    pub images: ResMut<'w, Assets<Image>>,
    pub surfaces: ResMut<'w, Assets<TitleSurface>>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub gltfs: Res<'w, Assets<Gltf>>,
    pub scenes: Res<'w, Assets<WorldAsset>>,
    pub inverse_bindposes: Res<'w, Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
}

#[derive(SystemParam)]
pub(super) struct Entities<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    nodes: Query<'w, 's, (&'static Transform, &'static ChildOf)>,
    bones: Query<'w, 's, (&'static Transform, &'static bevy::gltf::GltfExtras)>,
    slots: Query<'w, 's, &'static MaterialSlot>,
    meshes: Query<'w, 's, (&'static Mesh3d, Option<&'static SkinnedMesh>)>,
    instantiated: Query<'w, 's, (), With<Instantiated>>,
}

/// A static suffix after the nearest sampled bone. Applying global matrices
/// directly also preserves singular (hidden) bones and affine/sheared keys.
struct Node {
    entity: Entity,
    bone: Option<usize>,
    suffix: Mat4,
}
struct Part {
    spec: Arc<PreviewPart>,
    gltf: Handle<Gltf>,
    textures: Vec<Handle<Image>>,
    root: Option<Entity>,
    nodes: Vec<Node>,
    draws: Vec<(Entity, usize)>,
    suppressed_draws: Vec<Entity>,
    materials: Vec<Handle<TitleSurface>>,
    depth_materials: Vec<Handle<TitleSurface>>,
    red_materials: Vec<Handle<TitleSurface>>,
    red_depth_materials: Vec<Handle<TitleSurface>>,
    warm: Vec<Entity>,

    bound: bool,
    disabled: bool,
}
struct Instance {
    key: Option<Key>,
    parts: Vec<Part>,
}
fn bone_effect_groups(skeleton: &Skeleton) -> [Vec<usize>; 16] {
    let mut groups: [Vec<usize>; 16] = std::array::from_fn(|_| Vec::new());
    for (index, bone) in skeleton.bones.iter().enumerate() {
        let name = bone.name.as_bytes();
        if name
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"ef"))
            && let Some(digits) = name.get(2..4).and_then(|v| std::str::from_utf8(v).ok())
            && let Ok(group) = usize::from_str_radix(digits, 16)
            && let Some(bones) = groups.get_mut(group)
        {
            bones.push(index);
        }
    }
    groups
}

struct Model {
    bone_count: usize,
    effect_groups: [Vec<usize>; 16],
    shadow_bounds: shadows::Bounds,
    target_anchor: Option<resonance_game::battle::encounter::TargetAnchor>,
    lit: bool,
    texture_channels: Vec<resonance_content::AtlasChannel>,
    weapon_style: Option<WeaponStyle>,
    suppressed_nodes: BTreeSet<usize>,
    instances: Vec<Instance>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Actor(ActorId),
    Weapon(ActorId, Option<resonance_battle::WeaponSlot>),
    Independent(usize),
    Accessory(ActorId, u8, bool),
}

pub(super) struct View {
    diagnostics: Diagnostics,
    stage: Stage,
    stage_color: stage_color::State,
    activation_feedback: crate::field_dissolve::ActivationFeedback,
    scenery: Vec<(u8, Part)>,
    scenery_models: BTreeMap<u8, resonance_game::battle::stage::Model>,
    scenery_tick: u64,
    scenery_update: Option<u64>,
    scenery_alpha: BTreeMap<u8, u8>,
    models: BTreeMap<resonance_battle::ResourceId, Model>,
    model_particles: model_particles::Pool,
    accessories: Vec<accessories::State>,
    effects: effects::Effects,
    trails: trails::Trails,
    toon: Handle<Image>,
    sampled: crate::scene::SampledImages,
    camera: Option<Entity>,
    warm_target: Option<Handle<Image>>,
    warmup: Option<Warmup>,
    prepared: bool,
    active: bool,
    held_pose: Vec<(Entity, GlobalTransform)>,
}

fn actor_visible(frame: &BattleFrame, actor: ActorId) -> bool {
    frame
        .unison
        .as_ref()
        .is_none_or(|unison| !unison.hidden.contains(&actor))
}

impl View {
    pub(crate) fn stage_color(&self) -> [u8; 4] {
        self.stage_color.color()
    }

    pub fn target_anchor(&self, frame: &BattleFrame, actor: ActorId) -> Option<[f32; 3]> {
        let model = frame
            .models
            .iter()
            .find(|model| model.actor == actor && model.visible && actor_visible(frame, actor))?;
        let anchor = self.models.get(&model.resource)?.target_anchor?;
        let actor = frame.actors.get(actor.index())?;
        let Some(bone) = anchor.bone else {
            return Some(std::array::from_fn(|axis| {
                actor.position[axis] + actor.body.center_offset[axis] * actor.body.scale * 2.
            }));
        };
        let bone = model.bones.get(usize::from(bone))?;
        let origin = (Mat4::from_cols_array_2d(&model.world) * Mat4::from_cols_array_2d(bone))
            .transform_point3(Vec3::ZERO);
        let offset = Quat::from_rotation_y(actor.heading.to_radians())
            * (Vec3::from_array(anchor.offset) * actor.body.scale);
        Some((origin + offset).to_array())
    }

    pub fn body_center(&self, frame: &BattleFrame, actor: ActorId) -> Option<[f32; 3]> {
        let model = frame
            .models
            .iter()
            .find(|model| model.actor == actor && model.visible && actor_visible(frame, actor))?;
        let body = self.models.get(&model.resource)?;
        let instance = body
            .instances
            .iter()
            .find(|instance| instance.key == Some(Key::Actor(actor)))?;
        if !instance
            .parts
            .iter()
            .any(|part| !part.disabled && part.spec.scene.outline_color.is_none())
        {
            return None;
        }
        body.shadow_bounds
            .center(Mat4::from_cols_array_2d(&model.world), &model.bones)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn load_with_diagnostics(
        stage: Stage,
        scenery_models: BTreeMap<u8, resonance_game::battle::stage::Model>,
        models: Vec<RenderModel>,
        accessories: Vec<resonance_game::battle::encounter::ModelAccessory>,
        toon: Handle<Image>,
        effect_banks: Vec<RenderEffect>,
        trail_assets: Vec<RenderTrail>,
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let stage_valid = diagnostics
            .attempt("battle stage", stage.validate())?
            .is_some();
        let scenery = stage
            .layers
            .iter()
            .filter(|_| stage_valid)
            .map(|(&slot, part)| (slot, Part::load(part.clone(), server)))
            .collect();
        let mut prepared = BTreeMap::new();
        let mut model_particles = model_particles::Pool::default();
        for model in models {
            let resource = model.resource;
            let result = (|| -> Result<()> {
                model.skeleton.validate()?;
                ensure!(
                    model.texture_channels.len() <= 4
                        && model.texture_channels.iter().all(|c| c.frames > 0),
                    "invalid battle texture channels"
                );
                ensure!(
                    !matches!(model.instances, RenderInstances::Fixed(0))
                        && !model.parts.is_empty(),
                    "empty battle model {}",
                    model.resource
                );
                let mut parts = Vec::new();
                let mut particle_offsets = Vec::new();
                for (index, part) in model.parts.into_iter().enumerate() {
                    let validation = (|| -> Result<()> {
                        ensure!(
                            part.attached_to.is_none(),
                            "battle model attachments require a prepared sampled resource"
                        );
                        ensure!(
                            matches!(&model.instances, RenderInstances::Particles(_))
                                || part.scene.bone_names.len() == model.skeleton.bones.len(),
                            "battle model skeleton differs from geometry"
                        );
                        for animation in &part.scene.texture_animations {
                            animation.validate()?;
                        }
                        Ok(())
                    })();
                    if diagnostics
                        .attempt(
                            &format!("battle model part {}", part.scene.mesh),
                            validation,
                        )?
                        .is_some()
                    {
                        if let RenderInstances::Particles(offsets) = &model.instances {
                            particle_offsets.push(
                                *offsets
                                    .get(index)
                                    .context("missing particle layer bone offset")?,
                            );
                        }
                        parts.push(Part::load(part, server));
                    }
                }
                let instances = match model.instances {
                    RenderInstances::Fixed(count) => (0..count)
                        .map(|_| Instance {
                            key: None,
                            parts: parts.iter().map(Part::new_instance).collect(),
                        })
                        .collect(),
                    RenderInstances::Particles(_) => {
                        model_particles.load(
                            resource,
                            parts,
                            particle_offsets,
                            model.suppressed_nodes.clone(),
                        )?;
                        Vec::new()
                    }
                };
                ensure!(
                    !prepared.contains_key(&model.resource),
                    "duplicate battle render resource {}",
                    model.resource
                );
                prepared.insert(
                    model.resource,
                    Model {
                        bone_count: model.skeleton.bones.len(),
                        effect_groups: bone_effect_groups(&model.skeleton),
                        shadow_bounds: shadows::Bounds::prepare(&model.skeleton),
                        target_anchor: model.target_anchor,
                        lit: model.lit,
                        texture_channels: model.texture_channels,
                        weapon_style: model.weapon_style,
                        suppressed_nodes: model.suppressed_nodes,
                        instances,
                    },
                );
                Ok(())
            })();
            if let Err(error) = result {
                diagnostics.report(&format!("battle render model {resource}"), error)?;
            }
        }
        let effects = effects::Effects::load(
            effect_banks,
            accessories
                .iter()
                .find(|wing| wing.sparkle_texture().is_some()),
            server,
            diagnostics.clone(),
        )?;
        Ok(Self {
            stage_color: stage_color::State::new(stage.color),
            activation_feedback: Default::default(),
            stage,
            scenery,
            scenery_models,
            scenery_tick: 0,
            scenery_update: None,
            scenery_alpha: BTreeMap::new(),
            models: prepared,
            model_particles,
            accessories: accessories
                .into_iter()
                .map(accessories::State::new)
                .collect(),
            effects,
            trails: trails::Trails::load(trail_assets, server, diagnostics.clone())?,
            diagnostics,
            toon,
            sampled: Default::default(),
            camera: None,
            warm_target: None,
            warmup: None,
            prepared: false,
            active: false,
            held_pose: Vec::new(),
        })
    }

    pub fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        std::iter::once(&self.toon)
            .chain(self.scenery.iter().flat_map(|(_, p)| &p.textures))
            .chain(
                self.models
                    .values()
                    .flat_map(|m| &m.instances)
                    .flat_map(|i| &i.parts)
                    .flat_map(|p| &p.textures),
            )
            .chain(self.model_particles.images())
            .chain(self.effects.images())
            .chain(self.trails.images())
    }

    fn images_mut(&mut self) -> impl Iterator<Item = &mut Handle<Image>> {
        std::iter::once(&mut self.toon)
            .chain(self.scenery.iter_mut().flat_map(|(_, p)| &mut p.textures))
            .chain(
                self.models
                    .values_mut()
                    .flat_map(|m| &mut m.instances)
                    .flat_map(|i| &mut i.parts)
                    .flat_map(|p| &mut p.textures),
            )
            .chain(self.model_particles.images_mut())
            .chain(self.effects.images_mut())
            .chain(self.trails.images_mut())
    }

    fn warm_entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.scenery
            .iter()
            .flat_map(|(_, p)| {
                p.draws
                    .iter()
                    .map(|&(e, _)| e)
                    .chain(p.warm.iter().copied())
            })
            .chain(
                self.models
                    .values()
                    .filter_map(|m| m.instances.first())
                    .flat_map(|i| &i.parts)
                    .flat_map(|p| {
                        p.draws
                            .iter()
                            .map(|&(e, _)| e)
                            .chain(p.warm.iter().copied())
                    }),
            )
            .chain(self.model_particles.warm_draws())
            .chain(self.effects.entities())
            .chain(self.trails.entities())
    }

    /// Poll loading and the submitted-draw fence for one instance of each
    /// model and its material variants. Other instances share those recipes.
    pub fn prepare(
        &mut self,
        commands: &mut Commands,
        server: &AssetServer,
        assets: &mut AssetsForView,
        entities: &Entities,
        warmup: &Warmup,
        other_draws: Option<&[Entity]>,
    ) -> Result<bool> {
        if self.prepared {
            return Ok(true);
        }
        let phase_timing = |phase: &'static str, started: std::time::Instant, ready: bool| {
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.;
            // Loading and fence polling may span many display frames.
            if elapsed_ms >= 1. || ready {
                debug!(
                    target: "resonance_presentation::battle_entry",
                    phase,
                    elapsed_ms,
                    ready,
                    "Battle view preparation"
                );
            }
        };
        let started = std::time::Instant::now();
        if self.camera.is_none() {
            *warmup.0.lock().unwrap() = Report {
                diagnostics: Some(self.diagnostics.clone()),
                ..default()
            };
            self.warmup = Some(warmup.clone());
            let target = assets.images.add(Image::new_target_texture(
                64,
                64,
                TextureFormat::Bgra8Unorm,
                None,
            ));
            self.camera = Some(
                commands
                    .spawn((
                        scene_camera(),
                        Tonemapping::None,
                        Msaa::Off,
                        Camera {
                            order: -15,
                            ..default()
                        },
                        RenderTarget::Image(target.clone().into()),
                        RenderLayers::layer(WARM_LAYER),
                        battle_projection(),
                        Transform::from_xyz(0., 300., 2000.).looking_at(Vec3::ZERO, Vec3::Y),
                    ))
                    .id(),
            );
            self.warm_target = Some(target);
        }
        let diagnostics = self.diagnostics.clone();
        recover_images(
            self.images_mut(),
            server,
            &mut assets.images,
            &diagnostics,
            "battle image",
        )?;
        let images_ready = self.images().all(|h| assets.images.contains(h));
        phase_timing("recover/images", started, false);
        if !images_ready {
            return Ok(false);
        }
        let started = std::time::Instant::now();
        let mut queued_roots = 0;
        for (slot, part) in &mut self.scenery {
            let had_root = part.root.is_some();
            let result = part.prepare(
                commands,
                server,
                assets,
                entities,
                &mut self.sampled,
                &self.toon,
                false,
                self.scenery_models[slot].skeleton.bones.len(),
                None,
                &BTreeSet::new(),
                false,
                true,
            );
            if let Err(error) = result {
                self.diagnostics
                    .report(&format!("battle scenery {}", part.spec.scene.mesh), error)?;
                part.disable(commands);
            }
            queued_roots += usize::from(!had_root && part.root.is_some());
            if queued_roots >= PREPARATION_ROOTS_PER_UPDATE {
                phase_timing("scenery/models", started, false);
                return Ok(false);
            }
        }
        for model in self.models.values_mut() {
            for (index, instance) in model.instances.iter_mut().enumerate() {
                for part in &mut instance.parts {
                    let had_root = part.root.is_some();
                    let result = part.prepare(
                        commands,
                        server,
                        assets,
                        entities,
                        &mut self.sampled,
                        &self.toon,
                        model.lit,
                        model.bone_count,
                        model.weapon_style,
                        &model.suppressed_nodes,
                        model.weapon_style.is_none(),
                        index == 0,
                    );
                    if let Err(error) = result {
                        self.diagnostics.report(
                            &format!("battle model part {}", part.spec.scene.mesh),
                            error,
                        )?;
                        part.disable(commands);
                    }
                    queued_roots += usize::from(!had_root && part.root.is_some());
                    if queued_roots >= PREPARATION_ROOTS_PER_UPDATE {
                        phase_timing("scenery/models", started, false);
                        return Ok(false);
                    }
                }
            }
        }
        if !self.model_particles.prepare(
            commands,
            server,
            assets,
            &mut self.sampled,
            &self.toon,
            &self.effects.model_styles(),
            &self.diagnostics,
        )? {
            return Ok(false);
        }
        phase_timing("scenery/models", started, false);
        let started = std::time::Instant::now();
        self.effects.prepare(
            commands,
            &mut assets.meshes,
            &mut assets.surfaces,
            &mut assets.images,
            &mut self.sampled,
        )?;
        self.effects.set_layer(commands, WARM_LAYER, true);
        self.trails.prepare(commands, assets, &mut self.sampled)?;
        self.trails.set_layer(commands, WARM_LAYER, true);
        phase_timing("effects/trails", started, false);
        let started = std::time::Instant::now();
        if !self.scenery.iter().all(|(_, p)| p.bound)
            || !self
                .models
                .values()
                .flat_map(|m| &m.instances)
                .flat_map(|i| &i.parts)
                .all(|p| p.bound)
        {
            return Ok(false);
        }
        let Some(other_draws) = other_draws else {
            return Ok(false);
        };
        let mut report = warmup.0.lock().unwrap();
        if let Err(error) = report.check() {
            self.diagnostics.report("battle GPU", error)?;
        }
        let report = &mut report.draws;
        report
            .expected
            .extend(self.warm_entities().map(MainEntity::from));
        report
            .expected
            .extend(other_draws.iter().copied().map(MainEntity::from));
        if report.expected.is_empty() {
            self.diagnostics.report(
                "battle preparation",
                anyhow::anyhow!("battle has no prepared draws"),
            )?;
            report.completed.store(true, Ordering::Release);
        }
        report.armed = true;
        self.prepared = report.completed.load(Ordering::Acquire);
        phase_timing("fence poll", started, self.prepared);
        Ok(self.prepared)
    }

    pub fn warm_target(&self) -> Option<Handle<Image>> {
        self.warm_target.clone()
    }

    /// Retained field camera/roots are hidden by the caller in this same handoff.
    pub fn activate(&mut self, commands: &mut Commands, target: RenderTarget) -> Result<()> {
        let started = std::time::Instant::now();
        let mut relayered_parts = 0;
        ensure!(
            self.prepared && !self.active,
            "battle view is not ready to activate"
        );
        self.active = true;
        activate_camera(
            commands,
            self.camera.context("missing prepared battle camera")?,
            target,
            battle_projection(),
            -4,
        );
        for (slot, part) in &self.scenery {
            part.layer(commands, LAYER, true);
            relayered_parts += usize::from(part.root.is_some());
            let layer = self
                .stage
                .foreground_layers
                .iter()
                .position(|entry| entry == slot)
                .map_or(Layer::Scene(0), |order| Layer::Foreground(order as u8));
            for &(draw, index) in &part.draws {
                commands.entity(draw).insert(DrawOrder(
                    layer,
                    0,
                    part.spec.scene.materials[index].draw_order,
                ));
            }
        }

        for model in self.models.values() {
            for instance in &model.instances {
                for part in &instance.parts {
                    part.layer(commands, LAYER, false);
                    relayered_parts += usize::from(part.root.is_some());
                }
            }
        }
        self.model_particles.activate(commands);
        self.effects.set_layer(commands, LAYER, false);
        self.trails.set_layer(commands, LAYER, false);
        let warmup = self
            .warmup
            .as_ref()
            .context("missing battle preparation report")?;
        let mut report = warmup.0.lock().unwrap();
        if let Err(error) = report.check() {
            self.diagnostics.report("battle GPU", error)?;
        }
        report.draws = gpu::Report {
            armed: true,
            expected: self
                .scenery
                .iter()
                .flat_map(|(_, p)| p.draws.iter().map(|&(e, _)| MainEntity::from(e)))
                .collect(),
            ..default()
        };
        debug!(
            target: "resonance_presentation::battle_entry",
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.,
            relayered_parts,
            "Battle view activation"
        );
        Ok(())
    }

    /// Finish the actual target's draws before simulation and audio start.
    pub fn finish_activation(&self) -> Result<bool> {
        if !self.active {
            return Ok(false);
        }
        let report = self
            .warmup
            .as_ref()
            .context("missing battle preparation report")?
            .0
            .lock()
            .unwrap();
        if let Err(error) = report.check() {
            self.diagnostics.report("battle GPU", error)?;
        }
        Ok(report.draws.completed.load(Ordering::Acquire))
    }

    /// Also polled before host writeback, so render failures cannot commit a result.
    pub fn check(&self) -> Result<()> {
        if let Some(warmup) = &self.warmup
            && let Err(error) = warmup.0.lock().unwrap().check()
        {
            self.diagnostics.report("battle GPU", error)?;
        }
        Ok(())
    }

    /// Advance scene-owned feedback once per published battle update.
    pub fn advance_feedback(&mut self, frame: &BattleFrame, paused: bool) -> Result<()> {
        self.activation_feedback.advance(frame);
        self.stage_color.advance(frame.clock, &frame.cues);
        self.stage_color
            .advance_unison(frame.clock, frame.unison.as_ref());
        self.trails.advance(frame, paused)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The scene borrows independently owned ECS transforms, projections, materials, and meshes."
    )]
    pub fn apply(
        &mut self,
        (frame, particles): (&BattleFrame, &[resonance_battle::ParticleFrame]),
        commands: &mut Commands,
        globals: &mut Query<&mut GlobalTransform>,
        camera_transforms: &mut Query<&mut Transform>,
        projections: &mut Query<&mut Projection>,
        surfaces: &mut Assets<TitleSurface>,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        ensure!(self.active, "battle view is not active");
        self.check()?;
        let camera = frame
            .camera
            .context("battle frame has no prepared camera")?;
        let camera_pose = Transform::from_translation(Vec3::from_array(camera.eye))
            .looking_at(Vec3::from_array(camera.focus), Vec3::Y);
        let camera_entity = self.camera.context("missing battle camera")?;
        crate::field_dissolve::battle(commands, camera_entity, frame, &self.activation_feedback);
        *camera_transforms.get_mut(camera_entity)? = camera_pose;
        *globals.get_mut(camera_entity)? = camera_pose.into();
        let lens = frame
            .camera_fov_degrees
            .unwrap_or(resonance_battle::VERTICAL_FOV_DEGREES);
        ensure!(
            lens.is_finite() && lens > 0. && lens < 180.,
            "invalid battle lens"
        );
        let mut projection = projections.get_mut(camera_entity)?;
        let Projection::Custom(custom) = &mut *projection else {
            anyhow::bail!("battle camera projection is not prepared");
        };
        let projection = custom
            .get_mut::<BattleProjection>()
            .context("battle projection type mismatch")?;
        projection.0.0.fov = lens.to_radians();
        let stage_world = stage_world(&self.stage);
        let fog_distance = Vec2::new(
            camera.eye[0] - camera.focus[0],
            camera.eye[2] - camera.focus[2],
        )
        .length();
        let previous_scenery_tick = self.scenery_tick;
        if self.scenery_update != Some(frame.update) {
            if !frame.clock.paused() {
                self.scenery_tick += self
                    .scenery_update
                    .map_or(0, |last| frame.update.saturating_sub(last));
            }
            self.scenery_update = Some(frame.update);
        }
        let scenery_poses = self
            .scenery_models
            .iter()
            .map(|(&slot, model)| model.pose(self.scenery_tick).map(|pose| (slot, pose)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut accessory_frames = Vec::new();
        let mut independent_positions = Vec::new();
        for accessory in &mut self.accessories {
            for pose in accessory.sample(frame)? {
                let key = if accessory.source.attachment.is_none() {
                    let index = independent_positions.len();
                    let [x, y, z, _] = pose.world[3];
                    independent_positions.push([x, y, z]);
                    Key::Independent(index)
                } else {
                    Key::Accessory(
                        accessory.source.owner,
                        pose.pass,
                        matches!(
                            accessory.source.style,
                            resonance_game::battle::encounter::AccessoryStyle::Ordinary
                        ),
                    )
                };
                accessory_frames.push((accessory.source.resource, key, pose));
            }
        }
        let order = ActorOrder::new(frame, &independent_positions)?;
        let scenery_elapsed = self.scenery_tick - previous_scenery_tick;
        let camera_position = Vec2::new(camera.eye[0], camera.eye[2]);
        let camera_focus = Vec2::new(camera.focus[0], camera.focus[2]);
        for (slot, part) in &self.scenery {
            let result = (|| -> Result<()> {
                let attachment = match self.stage.scenery_bones.get(slot) {
                    Some(&bone) => Mat4::from_cols_array_2d(&scenery_poses[&0][usize::from(bone)]),
                    None => Mat4::IDENTITY,
                };
                let world = stage_world
                    * attachment
                    * Mat4::from_translation(Vec3::from_array(part.spec.scene.translation));
                let mut color = self.stage_color.color();
                if let Some(fade) = &self.stage.scenery_fade
                    && let Some(&minimum) = fade.minimum_alpha.get(slot)
                {
                    let anchor = world.transform_point3(Vec3::ZERO);
                    // Only nearer scenery can obscure the combat focus. Off-axis
                    // scenery may be in front of the focus but still farther away.
                    let target = if scenery_in_foreground(
                        camera_position,
                        camera_focus,
                        Vec2::new(anchor.x, anchor.z),
                    ) {
                        minimum
                    } else {
                        u8::MAX
                    };
                    let alpha = self.scenery_alpha.entry(*slot).or_insert(u8::MAX);
                    let step = u64::from(fade.step)
                        .saturating_mul(scenery_elapsed)
                        .min(u64::from(u8::MAX)) as u8;
                    *alpha = if *alpha > target {
                        alpha.saturating_sub(step).max(target)
                    } else {
                        alpha.saturating_add(step).min(target)
                    };
                    color[3] = (u16::from(color[3]) * u16::from(*alpha) / 255) as u8;
                }
                part.visible(commands, color[3] != 0);
                part.apply(world, &scenery_poses[slot], globals)?;
                let material = if frame.hourglass_remaining != 0 {
                    ModelMaterial::RedChannel
                } else {
                    ModelMaterial::Normal
                };
                part.tint(
                    color,
                    None,
                    material,
                    false,
                    self.stage.light_position,
                    true,
                    surfaces,
                )?;
                part.stage_materials(
                    material,
                    self.stage
                        .texture_animations
                        .get(slot)
                        .map_or(&[], Vec::as_slice),
                    self.scenery_tick,
                    self.stage.fog.as_ref(),
                    fog_distance,
                    surfaces,
                )?;
                let materials = part.appearance_materials(material, true)?;
                for &(draw, index) in &part.draws {
                    commands
                        .entity(draw)
                        .insert_if_neq(MeshMaterial3d(materials[index].clone()));
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.diagnostics.report(
                    &format!("battle scenery draw {}", part.spec.scene.mesh),
                    error,
                )?;
                part.visible(commands, false);
            }
        }
        let wanted: BTreeSet<_> = frame
            .models
            .iter()
            .map(|m| (m.resource, Key::Actor(m.actor)))
            .chain(
                frame
                    .weapons
                    .iter()
                    .map(|m| (m.resource, Key::Weapon(m.owner, m.slot))),
            )
            .chain(
                accessory_frames
                    .iter()
                    .map(|&(resource, key, _)| (resource, key)),
            )
            .collect();
        for (&resource, model) in &mut self.models {
            for instance in &mut model.instances {
                if instance.key.is_none() {
                    continue;
                }
                // Only claimed slots can need hiding; dormant pools stay hidden.
                for part in &instance.parts {
                    part.visible(commands, false);
                }
                if instance
                    .key
                    .is_some_and(|key| !wanted.contains(&(resource, key)))
                {
                    instance.key = None;
                }
            }
        }
        for model in &frame.models {
            self.model(
                model.resource,
                Key::Actor(model.actor),
                model.visible && actor_visible(frame, model.actor),
                model.tint,
                Some(crate::battle::appearance::outline(
                    &frame.actors[model.actor.index()],
                    frame.recognized_result.is_none() && !frame.clock.paused(),
                    frame.update,
                    model.tint[3],
                )),
                model.material,
                model.depth_write,
                &model.world,
                &model.bones,
                Some((model.texture_layers, model.texture_tick)),
                model.light,
                None,
                &order,
                commands,
                globals,
                surfaces,
            )?;
        }
        for model in &frame.weapons {
            self.model(
                model.resource,
                Key::Weapon(model.owner, model.slot),
                model.visible && actor_visible(frame, model.owner),
                model.tint,
                Some(default_outline(model.tint[3])),
                model.material,
                true,
                &model.world,
                &model.bones,
                None,
                None,
                model.texture_scroll,
                &order,
                commands,
                globals,
                surfaces,
            )?;
        }
        for &(resource, key, ref wing) in &accessory_frames {
            self.model(
                resource,
                key,
                true,
                wing.tint,
                None,
                wing.material,
                wing.depth_write,
                &wing.world,
                &wing.bones,
                None,
                None,
                wing.uv,
                &order,
                commands,
                globals,
                surfaces,
            )?;
        }
        self.model_particles.retain(particles, commands, surfaces);
        self.model_particles.clear_pose();
        for (index, particle) in particles.iter().enumerate() {
            if particle.model.is_none() {
                continue;
            }
            let result = (|| {
                let (style, pass) = self.effects.model_pass(particle, &order)?;
                self.model_particles.apply(
                    particle,
                    style,
                    pass,
                    index,
                    self.stage.light_position,
                    commands,
                    surfaces,
                )
            })();
            if result.is_err() {
                self.model_particles.hide(particle.id, commands);
            }
            self.diagnostics.attempt(
                &format!(
                    "battle model particle {}:{}",
                    particle.resource, particle.member
                ),
                result,
            )?;
        }
        let mut actor_shadows = Vec::new();
        for model in frame
            .models
            .iter()
            .filter(|model| model.visible && actor_visible(frame, model.actor))
        {
            let Some(shadow) = model.shadow else {
                continue;
            };
            let body = self
                .models
                .get(&model.resource)
                .context("missing shadow body")?;
            let instance = body
                .instances
                .iter()
                .find(|instance| instance.key == Some(Key::Actor(model.actor)))
                .context("missing shadow body instance")?;
            if instance
                .parts
                .iter()
                .any(|part| !part.disabled && part.spec.scene.outline_color.is_none())
                && let Some((position, radius)) = body.shadow_bounds.sample(
                    Mat4::from_cols_array_2d(&model.world),
                    &model.bones,
                    shadow.scale,
                    shadow.position[1],
                )
            {
                actor_shadows.push((position, radius, shadow.color));
            }
        }
        let result = self.effects.apply(
            (frame, &actor_shadows),
            &self.models,
            particles,
            self.accessories
                .iter()
                .find(|wing| wing.source.sparkle_texture().is_some())
                .map_or(&[], accessories::State::sparkles),
            Mat3::from_quat(camera_pose.rotation),
            meshes,
            commands,
            &order,
        );
        let scene_samples = match result {
            Ok(draws) => draws,
            Err(error) => {
                self.diagnostics.report("battle effects", error)?;
                scene_sample::Draws::default()
            }
        };
        commands
            .entity(self.camera.context("missing battle camera")?)
            .insert(scene_samples);
        let result = self
            .trails
            .apply(frame, commands, meshes, &order, &self.models);
        if let Err(error) = result {
            self.diagnostics.report("battle trails", error)?;
        }
        self.held_pose.clear();
        let parts = self.scenery.iter().map(|(_, part)| part).chain(
            self.models
                .values()
                .flat_map(|model| &model.instances)
                .filter(|instance| instance.key.is_some())
                .flat_map(|instance| &instance.parts),
        );
        for part in parts {
            for entity in part
                .root
                .into_iter()
                .chain(part.nodes.iter().map(|node| node.entity))
            {
                self.held_pose.push((entity, *globals.get(entity)?));
            }
        }
        self.held_pose
            .extend_from_slice(self.model_particles.held_pose());
        self.held_pose
            .push((camera_entity, *globals.get(camera_entity)?));
        Ok(())
    }

    /// Keep sampled affine poses after ordinary propagation between simulation updates.
    pub fn restore_pose(&self, globals: &mut Query<&mut GlobalTransform>) -> Result<()> {
        restore_pose(&self.held_pose, globals)
    }

    #[allow(clippy::too_many_arguments)] // One authoritative pose and the existing ECS render resources.
    fn model(
        &mut self,
        resource: resonance_battle::ResourceId,
        key: Key,
        visible: bool,
        tint: [u8; 4],
        outline_tint: Option<[u8; 4]>,
        material: ModelMaterial,
        depth_write: bool,
        world: &Matrix,
        bones: &[Matrix],
        texture_layers: Option<([u8; 4], u64)>,
        light: Option<[f32; 3]>,
        texture_scroll: Option<(usize, [f32; 2])>,
        actor_order: &ActorOrder,
        commands: &mut Commands,
        globals: &mut Query<&mut GlobalTransform>,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        let diagnostics = self.diagnostics.clone();
        let result = (|| -> Result<()> {
            let model = self
                .models
                .get_mut(&resource)
                .with_context(|| format!("unprepared battle render model {resource}"))?;
            ensure!(
                bones.len() == model.bone_count,
                "battle draw pose differs from prepared skeleton"
            );
            let index = model
                .instances
                .iter()
                .position(|i| i.key == Some(key))
                .or_else(|| model.instances.iter().position(|i| i.key.is_none()))
                .context("battle model exceeds its prepared instance capacity")?;
            let instance = &mut model.instances[index];
            instance.key = Some(key);
            let order = match key {
                Key::Actor(actor) => Layer::Actor(actor_order.get(actor)?, ActorLayer::Body),
                Key::Weapon(actor, slot) => {
                    weapon_layer(actor_order.get(actor)?, slot, model.weapon_style, false)
                }
                Key::Accessory(actor, _, after_body) => Layer::Actor(
                    actor_order.get(actor)?,
                    if after_body {
                        ActorLayer::Front
                    } else {
                        ActorLayer::Behind
                    },
                ),
                Key::Independent(index) => {
                    Layer::Actor(actor_order.independent(index)?, ActorLayer::Body)
                }
            };
            for part in &mut instance.parts {
                if part.disabled {
                    continue;
                }
                let result = (|| -> Result<()> {
                    part.visible(commands, visible);
                    part.apply(Mat4::from_cols_array_2d(world), bones, globals)?;
                    part.tint(
                        tint,
                        outline_tint,
                        material,
                        model.lit,
                        light.unwrap_or(self.stage.light_position),
                        depth_write,
                        surfaces,
                    )?;
                    if let Some((texture, uv)) = texture_scroll {
                        part.scroll_uv(material, depth_write, Some(texture), uv, surfaces)?;
                    }
                    if let Some((layers, tick)) = texture_layers {
                        part.expression(
                            &model.texture_channels,
                            layers,
                            tick,
                            material,
                            depth_write,
                            surfaces,
                        )?;
                    }
                    for &(draw, index) in &part.draws {
                        let material =
                            part.appearance_materials(material, depth_write)?[index].clone();
                        let instance = match key {
                            Key::Accessory(_, pass, _) => usize::from(pass),
                            _ => 0,
                        };
                        commands
                            .entity(draw)
                            .insert_if_neq(MeshMaterial3d(material))
                            .insert_if_neq(DrawOrder(
                                order,
                                instance,
                                part.spec.scene.materials[index].draw_order,
                            ));
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    diagnostics.report(
                        &format!("battle model part draw {}", part.spec.scene.mesh),
                        error,
                    )?;
                    part.visible(commands, false);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            diagnostics.report(&format!("battle model draw {resource}"), error)?;
            if let Some(model) = self.models.get(&resource) {
                for instance in &model.instances {
                    if instance.key == Some(key) {
                        for part in &instance.parts {
                            part.visible(commands, false);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn model_particle_stats(&self) -> ModelParticleStats {
        self.model_particles.stats()
    }

    pub fn despawn(self, commands: &mut Commands) {
        // The retained field/title may specialize again as soon as it is restored.
        if let Some(warmup) = &self.warmup {
            warmup.0.lock().unwrap().disarm();
        }
        if let Some(camera) = self.camera {
            commands.entity(camera).despawn();
        }
        for (_, part) in self.scenery {
            if let Some(root) = part.root {
                commands.entity(root).despawn();
            }
        }
        for model in self.models.into_values() {
            for instance in model.instances {
                for part in instance.parts {
                    if let Some(root) = part.root {
                        commands.entity(root).despawn();
                    }
                }
            }
        }
        self.model_particles.despawn(commands);
        self.effects.despawn(commands);
        self.trails.despawn(commands);
    }
}

fn surface_recipes(
    spec: &PreviewPart,
    textures: &[Handle<Image>],
    images: &mut Assets<Image>,
    sampled: &mut crate::scene::SampledImages,
    toon: &Handle<Image>,
    lit: bool,
    weapon_style: Option<WeaponStyle>,
) -> Result<Vec<TitleSurface>> {
    let mut surfaces = Vec::with_capacity(spec.scene.materials.len());
    for (index, material) in spec.scene.materials.iter().enumerate() {
        let binding = |binding: &resonance_content::TextureBinding| -> Result<_> {
            Ok((
                textures
                    .get(binding.texture)
                    .context("invalid battle material texture")?
                    .clone(),
                binding.clone(),
            ))
        };
        let color = crate::scene::sampled_image(
            material.color.as_ref().map(binding).transpose()?,
            images,
            sampled,
        );
        let multiply = crate::scene::sampled_image(
            material.multiply.as_ref().map(binding).transpose()?,
            images,
            sampled,
        );
        let mut surface = TitleSurface {
            vertex_color: material.vertex_color,
            multiply,
            uv_offsets: Vec4::from_array(spec.uv_offsets.get(index).copied().unwrap_or([0.; 4])),
            constant_color: spec.scene.outline_color_for(material).is_some(),
            tint: spec
                .scene
                .outline_color_for(material)
                .map_or(Vec4::ONE, |color| {
                    Vec4::from_array(color.map(|value| f32::from(value) / 255.))
                }),
            toon_ramp: (lit && spec.scene.outline_color.is_none() && material.color.is_some())
                .then(|| toon.clone()),
            shade_colors: [49., 66.].map(|v| Vec3::splat(v / 255.).extend(1.)),
            blend: if spec.additive {
                Some(Blend::Additive)
            } else {
                (material.blend || lit).then_some(Blend::Alpha)
            },
            depth_write: material.depth_write,
            cull: material.cull,
            ..TitleSurface::textured(color)
        };
        apply_weapon_style(&mut surface, weapon_style);
        surfaces.push(surface);
    }
    Ok(surfaces)
}

impl Part {
    fn disable(&mut self, commands: &mut Commands) {
        if let Some(root) = self.root.take() {
            commands.entity(root).despawn();
        }
        self.nodes.clear();
        self.draws.clear();
        self.suppressed_draws.clear();
        self.warm.clear();
        self.bound = true;
        self.disabled = true;
    }

    fn load(spec: PreviewPart, server: &AssetServer) -> Self {
        let gltf = server.load(spec.scene.mesh.clone());
        let textures = spec
            .scene
            .textures
            .iter()
            .map(|path| {
                server
                    .load_builder()
                    .with_settings(|s: &mut ImageLoaderSettings| {
                        s.is_srgb = false;
                        s.sampler = ImageSampler::linear();
                    })
                    .load(path.clone())
            })
            .collect();
        Self::unbound(spec.into(), gltf, textures)
    }

    fn new_instance(&self) -> Self {
        Self::unbound(
            Arc::clone(&self.spec),
            self.gltf.clone(),
            self.textures.clone(),
        )
    }

    fn unbound(spec: Arc<PreviewPart>, gltf: Handle<Gltf>, textures: Vec<Handle<Image>>) -> Self {
        Self {
            spec,
            gltf,
            textures,
            root: None,
            nodes: Vec::new(),
            draws: Vec::new(),
            suppressed_draws: Vec::new(),
            materials: Vec::new(),

            depth_materials: Vec::new(),
            red_materials: Vec::new(),
            red_depth_materials: Vec::new(),
            warm: Vec::new(),

            bound: false,
            disabled: false,
        }
    }

    #[allow(clippy::too_many_arguments)] // Preparation borrows the existing asset and scene owners.
    fn prepare(
        &mut self,
        commands: &mut Commands,
        server: &AssetServer,
        assets: &mut AssetsForView,
        entities: &Entities,
        sampled: &mut crate::scene::SampledImages,
        toon: &Handle<Image>,
        lit: bool,
        bone_count: usize,
        weapon_style: Option<WeaponStyle>,
        suppressed_nodes: &BTreeSet<usize>,
        body: bool,
        warm_variants: bool,
    ) -> Result<()> {
        if self.bound {
            return Ok(());
        }
        check_load(server, self.gltf.id().untyped())?;
        if !server.is_loaded_with_dependencies(self.gltf.id()) {
            return Ok(());
        }
        let Some(gltf) = assets.gltfs.get(&self.gltf) else {
            return Ok(());
        };
        if self.root.is_none() {
            ensure!(
                gltf.meshes.len() == self.spec.scene.materials.len(),
                "battle material count differs from cooked geometry"
            );
            let recipes = surface_recipes(
                &self.spec,
                &self.textures,
                &mut assets.images,
                sampled,
                toon,
                lit,
                weapon_style,
            )?;
            self.prepare_materials(recipes, body, &mut assets.surfaces);
            self.root = Some(
                commands
                    .spawn((
                        WorldAssetRoot(
                            gltf.scenes
                                .first()
                                .context("battle model has no scene")?
                                .clone(),
                        ),
                        Transform::default(),
                        if warm_variants {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        },
                        RenderLayers::layer(WARM_LAYER),
                    ))
                    .observe(|event: On<WorldInstanceReady>, mut commands: Commands| {
                        commands.entity(event.entity).insert(Instantiated);
                    })
                    .id(),
            );
            return Ok(());
        }
        let root = self.root.unwrap();
        if !entities.instantiated.contains(root) {
            return Ok(());
        }
        let binding = if bone_count == 0 {
            Default::default()
        } else {
            crate::sparse_animation::Binding::new(
                root,
                bone_count,
                &entities.children,
                &entities.bones,
            )?
        };
        let indices: BTreeMap<_, _> = binding
            .0
            .iter()
            .enumerate()
            .map(|(i, (e, _))| (*e, i))
            .collect();
        let mut paths = BTreeMap::from([(root, (None, Mat4::IDENTITY))]);
        for entity in entities.children.iter_descendants(root) {
            let Ok((transform, parent)) = entities.nodes.get(entity) else {
                continue;
            };
            let (bone, suffix) = if let Some(&bone) = indices.get(&entity) {
                (Some(bone), Mat4::IDENTITY)
            } else {
                let &(bone, parent_suffix) = paths
                    .get(&parent.parent())
                    .context("battle scene has an unbound transform parent")?;
                (bone, parent_suffix * transform.to_matrix())
            };
            paths.insert(entity, (bone, suffix));
            self.nodes.push(Node {
                entity,
                bone,
                suffix,
            });
            commands
                .entity(entity)
                .insert((RenderLayers::layer(WARM_LAYER), NoFrustumCulling));
            if let Ok(slot) = entities.slots.get(entity) {
                let index = slot.index(self.materials.len())?;
                let (mesh, skin) = entities.meshes.get(entity)?;
                validate_draw_mesh(
                    assets
                        .meshes
                        .get(&mesh.0)
                        .context("missing battle mesh data")?,
                    assets
                        .surfaces
                        .get(&self.materials[index])
                        .context("missing battle material data")?,
                    skin.is_some(),
                )?;
                commands
                    .entity(entity)
                    .remove::<MeshMaterial3d<StandardMaterial>>()
                    .insert((
                        MeshMaterial3d(self.materials[index].clone()),
                        DrawOrder(
                            Layer::Scene(self.spec.scene.materials[index].draw_order),
                            0,
                            0,
                        ),
                    ));
                self.draws.push((entity, index));
                if bone.is_some_and(|bone| suppressed_nodes.contains(&bone)) {
                    // Hide the object while retaining anchors and skinning transforms for
                    // its children.
                    self.suppressed_draws.push(entity);
                }
                // Instances share mesh layouts and material recipes. One set of
                // variant draws prepares their pipelines without duplicate skins.
                for variants in [
                    &self.depth_materials,
                    &self.red_materials,
                    &self.red_depth_materials,
                ]
                .into_iter()
                .filter(|variants| warm_variants && !variants.is_empty())
                {
                    let mut warm = commands.spawn((
                        mesh.clone(),
                        MeshMaterial3d(variants[index].clone()),
                        Transform::default(),
                        Visibility::Inherited,
                        NoFrustumCulling,
                        RenderLayers::layer(WARM_LAYER),
                        ChildOf(root),
                    ));
                    if let Some(skin) = skin {
                        warm.insert(skin.clone());
                    }
                    self.warm.push(warm.id());
                }
            }
        }
        self.bound = true;
        Ok(())
    }

    fn apply(
        &self,
        world: Mat4,
        bones: &[Matrix],
        globals: &mut Query<&mut GlobalTransform>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        apply_nodes(
            self.root.context("unprepared battle root")?,
            &self.nodes,
            world,
            bones,
            globals,
        )
    }

    #[allow(clippy::too_many_arguments)] // One sampled appearance and its prepared material selection.
    fn tint(
        &self,
        color: [u8; 4],
        outline_tint: Option<[u8; 4]>,
        material: ModelMaterial,
        lit: bool,
        light: [f32; 3],
        depth_write: bool,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let materials = self.appearance_materials(material, depth_write)?;
        for handle in materials {
            let surface = surfaces
                .get(handle)
                .context("prepared battle material is missing")?;
            let base = if surface.constant_color {
                outline(&self.spec)
            } else {
                Vec4::ONE
            };
            let rgb = Vec3::new(
                f32::from(color[0]) / 64.,
                f32::from(color[1]) / 64.,
                f32::from(color[2]) / 64.,
            );
            let (ambient, tint, field_light) = if surface.constant_color
                && let Some(outline) = outline_tint
            {
                // Alternate models use a constant outline color.
                // It is independent of the primary tint/64 and stage lighting.
                (
                    Vec3::ONE,
                    Vec4::from_array(outline.map(|value| f32::from(value) / 255.)),
                    Vec4::ZERO,
                )
            } else if lit && surface.toon_ramp.is_some() {
                (
                    rgb,
                    base * Vec4::new(1., 1., 1., f32::from(color[3]) / 255.),
                    Vec3::from_array(light).extend(192.),
                )
            } else {
                (
                    Vec3::ONE,
                    base * rgb.extend(f32::from(color[3]) / 255.),
                    Vec4::ZERO,
                )
            };
            if surface.ambient_scale != ambient
                || surface.tint != tint
                || surface.field_light != field_light
            {
                let mut surface = surfaces.get_mut(handle).unwrap();
                surface.ambient_scale = ambient;
                surface.tint = tint;
                surface.field_light = field_light;
            }
        }
        Ok(())
    }

    fn prepare_materials(
        &mut self,
        recipes: Vec<TitleSurface>,
        body: bool,
        surfaces: &mut Assets<TitleSurface>,
    ) {
        for surface in recipes {
            let faded = body.then(|| TitleSurface {
                depth_write: false,
                depth_equal: true,
                ..surface.clone()
            });
            if self.spec.scene.outline_color.is_none() {
                self.red_materials.push(surfaces.add(TitleSurface {
                    red_channel: true,
                    ..surface.clone()
                }));
                if let Some(faded) = &faded {
                    self.red_depth_materials.push(surfaces.add(TitleSurface {
                        red_channel: true,
                        ..faded.clone()
                    }));
                }
            }
            if let Some(faded) = faded {
                self.depth_materials.push(surfaces.add(faded));
            }
            self.materials.push(surfaces.add(surface));
        }
    }

    fn appearance_materials(
        &self,
        material: ModelMaterial,
        depth_write: bool,
    ) -> Result<&[Handle<TitleSurface>]> {
        let red = material == ModelMaterial::RedChannel && self.spec.scene.outline_color.is_none();
        let handles = match (red, depth_write) {
            (false, true) => &self.materials,
            (false, false) => &self.depth_materials,
            (true, true) => &self.red_materials,
            (true, false) => &self.red_depth_materials,
        };
        ensure!(
            handles.len() == self.materials.len(),
            "battle material variant was not prepared"
        );
        Ok(handles)
    }

    fn scroll_uv(
        &self,
        material: ModelMaterial,
        depth_write: bool,
        texture: Option<usize>,
        scroll: [f32; 2],
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        for (index, handle) in self
            .appearance_materials(material, depth_write)?
            .iter()
            .enumerate()
        {
            let mut offset = self.spec.uv_offsets.get(index).copied().unwrap_or_default();
            let material = &self.spec.scene.materials[index];
            for (channel, binding) in [&material.color, &material.multiply]
                .into_iter()
                .enumerate()
            {
                if texture.is_none_or(|texture| {
                    binding
                        .as_ref()
                        .is_some_and(|binding| binding.texture == texture)
                }) {
                    for axis in 0..2 {
                        offset[channel * 2 + axis] += scroll[axis];
                    }
                }
            }
            let offset = Vec4::from_array(offset);
            let surface = surfaces
                .get(handle)
                .context("missing effect model material")?;
            if surface.uv_offsets != offset {
                surfaces.get_mut(handle).unwrap().uv_offsets = offset;
            }
        }
        Ok(())
    }

    fn stage_materials(
        &self,
        appearance: ModelMaterial,
        animations: &[resonance_content::battle_stage::TextureAnimation],
        tick: u64,
        fog: Option<&resonance_content::battle_stage::Fog>,
        fog_distance: f32,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        let fog_range = fog.map_or(Vec4::ZERO, |fog| {
            Vec4::new(fog_distance + fog.start, fog_distance + fog.end, 2., 0.)
        });
        let fog_color = fog.map_or(Vec4::ZERO, |fog| {
            Vec4::from_array(fog.color.map(|v| f32::from(v) / 255.))
        });
        for (material, handle) in self
            .spec
            .scene
            .materials
            .iter()
            .zip(self.appearance_materials(appearance, true)?)
        {
            let uv = [&material.color, &material.multiply].map(|binding| {
                binding
                    .as_ref()
                    .and_then(|binding| animations.iter().find(|a| a.texture == binding.texture))
                    .map_or([0.; 2], |animation| animation.offset(tick))
            });
            let uv_offsets = Vec4::new(uv[0][0], uv[0][1], uv[1][0], uv[1][1]);
            let surface = surfaces.get(handle).context("missing stage material")?;
            if surface.uv_offsets != uv_offsets
                || surface.fog_range != fog_range
                || surface.fog_color != fog_color
            {
                let mut surface = surfaces.get_mut(handle).unwrap();
                surface.uv_offsets = uv_offsets;
                surface.fog_range = fog_range;
                surface.fog_color = fog_color;
            }
        }
        Ok(())
    }

    fn expression(
        &self,
        channels: &[resonance_content::AtlasChannel],
        layers: [u8; 4],
        tick: u64,
        appearance: ModelMaterial,
        depth_write: bool,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        for (index, handle) in self
            .appearance_materials(appearance, depth_write)?
            .iter()
            .enumerate()
        {
            let material = &self.spec.scene.materials[index];
            let mut uv = self.spec.uv_offsets.get(index).copied().unwrap_or([0.; 4]);
            for (channel, binding) in [&material.color, &material.multiply]
                .into_iter()
                .enumerate()
            {
                if let Some(animation) = binding.as_ref().and_then(|binding| {
                    self.spec
                        .scene
                        .texture_animations
                        .iter()
                        .find(|animation| animation.texture == binding.texture)
                }) {
                    let offset = animation.offset(tick);
                    uv[channel * 2] += offset[0];
                    uv[channel * 2 + 1] += offset[1];
                }
            }
            for (layer, channel) in channels.iter().enumerate() {
                for (binding, axis) in [(&material.color, 1), (&material.multiply, 3)] {
                    if binding
                        .as_ref()
                        .is_some_and(|b| b.texture == channel.texture)
                    {
                        uv[axis] += (1. / f32::from(channel.frames)) * f32::from(layers[layer]);
                    }
                }
            }
            let uv = Vec4::from_array(uv);
            let surface = surfaces
                .get(handle)
                .context("prepared battle material is missing")?;
            if surface.uv_offsets != uv {
                surfaces.get_mut(handle).unwrap().uv_offsets = uv;
            }
        }
        Ok(())
    }

    fn visible(&self, commands: &mut Commands, visible: bool) {
        if let Some(root) = self.root {
            commands.entity(root).insert_if_neq(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
    fn layer(&self, commands: &mut Commands, layer: usize, visible: bool) {
        self.visible(commands, visible);
        for &entity in &self.warm {
            commands
                .entity(entity)
                .insert((Visibility::Hidden, NoCpuCulling));
        }
        for entity in self
            .root
            .into_iter()
            .chain(self.nodes.iter().map(|n| n.entity))
        {
            commands.entity(entity).insert(RenderLayers::layer(layer));
        }
        // All declared meshes participate in preparation's draw fence. Suppress
        // only this object's draw after layer assignment; Visibility::Hidden
        // would also hide its children.
        if layer != WARM_LAYER {
            for &entity in &self.suppressed_draws {
                commands.entity(entity).insert(RenderLayers::none());
            }
        }
    }
}

/// Reject malformed mesh bindings before they disappear inside Bevy's pipeline
/// specialization (where no pipeline ID exists for the submitted-draw fence).
fn validate_draw_mesh(mesh: &Mesh, surface: &TitleSurface, skinned: bool) -> Result<()> {
    ensure!(
        mesh.contains_attribute(Mesh::ATTRIBUTE_POSITION) && mesh.count_vertices() > 0,
        "battle mesh has no vertex positions"
    );
    if surface.color.is_some() && !surface.constant_color {
        ensure!(
            mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0),
            "textured battle mesh has no primary UVs"
        );
    }
    if surface.multiply.is_some() && !surface.constant_color {
        ensure!(
            mesh.contains_attribute(Mesh::ATTRIBUTE_UV_1),
            "battle mesh has no secondary UVs"
        );
    }
    if skinned {
        ensure!(
            mesh.contains_attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
                && mesh.contains_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT),
            "battle mesh has no skin binding"
        );
    }
    Ok(())
}

fn apply_nodes(
    root: Entity,
    nodes: &[Node],
    world: Mat4,
    bones: &[Matrix],
    globals: &mut Query<&mut GlobalTransform>,
) -> Result<()> {
    ensure!(world.is_finite(), "invalid battle draw world matrix");
    *globals.get_mut(root)? = GlobalTransform::from(bevy::math::Affine3A::from_mat4(world));
    for node in nodes {
        let matrix = match node.bone {
            Some(bone) => Mat4::from_cols_array_2d(
                bones
                    .get(bone)
                    .context("battle draw bone outside sampled pose")?,
            ),
            None => Mat4::IDENTITY,
        };
        ensure!(matrix.is_finite(), "invalid battle draw bone matrix");
        *globals.get_mut(node.entity)? = GlobalTransform::from(bevy::math::Affine3A::from_mat4(
            world * matrix * node.suffix,
        ));
    }
    Ok(())
}

fn restore_pose(
    poses: &[(Entity, GlobalTransform)],
    globals: &mut Query<&mut GlobalTransform>,
) -> Result<()> {
    for &(entity, pose) in poses {
        globals.get_mut(entity)?.set_if_neq(pose);
    }
    Ok(())
}

/// Black with full opacity is the default outline sentinel. Resolve it to half the primary
/// alpha for each model.
fn default_outline(primary_alpha: u8) -> [u8; 4] {
    [0, 0, 0, primary_alpha >> 1]
}

fn outline(part: &PreviewPart) -> Vec4 {
    part.scene.outline_color.map_or(Vec4::ONE, |c| {
        Vec4::from_array(c.map(|v| f32::from(v) / 255.))
    })
}
/// Rebind only this owner's failed images. Source IDs remain unavailable to
/// other scenes; one conspicuous replacement is shared by this recovery pass.
pub(super) fn recover_images<'a>(
    handles: impl Iterator<Item = &'a mut Handle<Image>>,
    server: &AssetServer,
    images: &mut Assets<Image>,
    diagnostics: &Diagnostics,
    scope: &str,
) -> Result<std::collections::HashMap<bevy::asset::AssetId<Image>, Handle<Image>>> {
    let mut replacements = std::collections::HashMap::new();
    let mut placeholder = None;
    for image in handles {
        if let Some(replacement) = replacements.get(&image.id()) {
            *image = Handle::clone(replacement);
        } else if !images.contains(image.id())
            && let Err(error) = check_load(server, image.id().untyped())
        {
            diagnostics.report(scope, error)?;
            let replacement = placeholder
                .get_or_insert_with(|| images.add(placeholder_image()))
                .clone();
            replacements.insert(image.id(), replacement.clone());
            *image = replacement;
        }
    }
    Ok(replacements)
}

fn placeholder_image() -> Image {
    Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &[255, 0, 255, 255],
        TextureFormat::Rgba8Unorm,
        bevy::asset::RenderAssetUsages::default(),
    )
}

fn check_load(server: &AssetServer, id: bevy::asset::UntypedAssetId) -> Result<()> {
    if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(id) {
        anyhow::bail!("battle asset failed: {error}");
    }
    Ok(())
}
fn stage_world(stage: &Stage) -> Mat4 {
    let yaw = Quat::from_rotation_y(stage.yaw_degrees.to_radians());
    Mat4::from_rotation_translation(
        yaw * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        yaw * Vec3::from_array(stage.translation),
    )
}

fn scenery_in_foreground(camera: Vec2, focus: Vec2, scenery: Vec2) -> bool {
    camera.distance_squared(scenery) < camera.distance_squared(focus)
}

#[derive(Debug, Clone)]
struct BattleProjection(crate::camera::TitleProjection);
impl BattleProjection {
    fn depth(&self, mut clip: Mat4) -> Mat4 {
        // Keep finite near and far planes and raster alignment while converting depth to
        // reverse Z.
        let projection = &self.0.0;
        let range = 1. / (projection.far - projection.near);
        clip.z_axis.z = projection.near * range;
        clip.w_axis.z = (projection.far * projection.near) * range;
        clip
    }
}
impl CameraProjection for BattleProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.depth(self.0.get_clip_from_view())
    }
    fn get_clip_from_view_for_sub(&self, view: &SubCameraView) -> Mat4 {
        self.depth(self.0.get_clip_from_view_for_sub(view))
    }
    fn update(&mut self, width: f32, height: f32) {
        // Scene targets contain 448 of the 480 display rows. The final output
        // expands them, so use the full display aspect for undistorted models.
        self.0.0.aspect_ratio = width / height * resonance_content::SCENE_HEIGHT as f32
            / resonance_content::HEIGHT as f32;
    }
    fn far(&self) -> f32 {
        self.0.far()
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [bevy::math::Vec3A; 8] {
        self.0.get_frustum_corners(near, far)
    }
}
fn scene_camera() -> Camera3d {
    // Scene-sampling particles read depth in their fragment shader. This camera
    // is retained when its warmup target is replaced by the live battle target.
    Camera3d {
        depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
            .into(),
        ..default()
    }
}

fn battle_projection() -> Projection {
    // Configure the vertical field of view, clipping planes, and display aspect.
    Projection::custom(BattleProjection(crate::camera::TitleProjection(
        PerspectiveProjection {
            fov: resonance_battle::VERTICAL_FOV_DEGREES.to_radians(),
            aspect_ratio: 4. / 3.,
            near: 50.,
            far: 21288.,
            ..default()
        },
    )))
}

pub(super) fn activate_camera(
    commands: &mut Commands,
    camera: Entity,
    target: RenderTarget,
    projection: Projection,
    order: isize,
) {
    // Bevy 0.19 does not recompute Camera::computed when only RenderTarget
    // changes. Preserve the camera and refresh its projection so the isolated
    // 64x64 target cannot survive the handoff to the actual scene target.
    commands
        .entity(camera)
        .insert((target, RenderLayers::layer(LAYER), projection))
        .entry::<Camera>()
        .and_modify(move |mut camera| {
            camera.order = order;
            camera.is_active = true;
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn scenery_fade_preserves_distant_side_pillars() {
        let eye = Vec2::new(0., -2500.);
        let focus = Vec2::ZERO;
        assert!(scenery_in_foreground(eye, focus, Vec2::new(0., -500.)));
        for pillar in [
            Vec2::new(-1600., -200.),
            Vec2::new(1600., -200.),
            Vec2::new(0., 500.),
            focus,
        ] {
            assert!(!scenery_in_foreground(eye, focus, pillar));
        }
    }

    #[test]
    fn failed_image_recovery_keeps_source_ids_unavailable_to_other_owners() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.path().to_string_lossy().into_owned(),
                ..Default::default()
            },
        ))
        .init_asset::<Image>()
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ));
        let server = app.world().resource::<AssetServer>().clone();
        let source: Handle<Image> = server.load("missing.png");
        let started = std::time::Instant::now();
        while !matches!(
            server.get_load_state(source.id()),
            Some(bevy::asset::LoadState::Failed(_))
        ) {
            ensure!(
                started.elapsed().as_secs() < 10,
                "missing image did not fail"
            );
            app.update();
            std::thread::yield_now();
        }
        let mut images = app.world_mut().remove_resource::<Assets<Image>>().unwrap();
        for paranoid in [true, false] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut owner = [source.clone(), source.clone()];
            let recovered = recover_images(
                owner.iter_mut(),
                &server,
                &mut images,
                &diagnostics,
                "owner image",
            );
            assert_eq!(recovered.is_err(), paranoid);
            assert!(diagnostics.has_errors());
            assert!(!images.contains(source.id()));
            assert!(crate::field_view::image_ready(&server, &images, &source).is_err());
            if paranoid {
                assert_eq!(owner[0], source);
            } else {
                assert_eq!(owner[0], owner[1]);
                assert_ne!(owner[0], source);
                assert!(images.contains(owner[0].id()));
                assert_eq!(recovered?.len(), 1);
            }
        }
        Ok(())
    }

    #[test]
    fn weapon_styles_preserve_mesh_shading_or_enable_additive_rendering() {
        let base = TitleSurface {
            toon_ramp: Some(Handle::default()),
            cull: resonance_content::CullFace::Front,
            ..Default::default()
        };
        let mut ordinary = base.clone();
        apply_weapon_style(&mut ordinary, Some(WeaponStyle::default()));
        assert!(ordinary.toon_ramp.is_none());
        assert_ne!(ordinary.blend, Some(Blend::Additive));
        assert!(ordinary.depth_write && ordinary.depth_equal);
        assert_eq!(ordinary.cull, base.cull);

        let mut luminous = base.clone();
        apply_weapon_style(
            &mut luminous,
            Some(WeaponStyle {
                toon: true,
                additive: true,
                ..Default::default()
            }),
        );
        assert_eq!(luminous.toon_ramp, base.toon_ramp);
        assert!(luminous.blend == Some(Blend::Additive) && luminous.depth_equal);
        assert!(!luminous.depth_write);
        assert_eq!(luminous.cull, resonance_content::CullFace::None);

        let mut body = base.clone();
        apply_weapon_style(&mut body, None);
        assert_eq!(body.toon_ramp, base.toon_ramp);
        assert!(body.depth_write && !body.depth_equal);
        assert_eq!(body.cull, base.cull);
    }

    #[test]
    fn attachment_suppression_preserves_child_draws_and_warmup_layers() {
        use bevy::camera::{primitives::Frustum, visibility::VisibilityPlugin};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), VisibilityPlugin))
            .init_asset::<Mesh>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>();
        let world = app.world_mut();
        world.spawn((
            Camera::default(),
            Frustum::default(),
            RenderLayers::layer(LAYER),
        ));
        let root = world
            .spawn((Visibility::Inherited, GlobalTransform::default()))
            .id();
        let object = world
            .spawn((
                Mesh3d::default(),
                NoFrustumCulling,
                GlobalTransform::default(),
                ChildOf(root),
            ))
            .id();
        let child = world
            .spawn((
                Mesh3d::default(),
                NoFrustumCulling,
                GlobalTransform::default(),
                ChildOf(object),
            ))
            .id();
        let part = Part {
            spec: PreviewPart {
                scene: serde_json::from_value(serde_json::json!({
                    "resource": 0, "mesh": "unused.glb", "textures": [], "materials": [],
                    "translation": [0, 0, 0], "clips": [], "autoplay": false,
                    "texture_animations": []
                }))
                .unwrap(),
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: vec![],
            }
            .into(),
            gltf: Handle::default(),
            textures: vec![],
            root: Some(root),
            nodes: [object, child]
                .into_iter()
                .map(|entity| Node {
                    entity,
                    bone: None,
                    suffix: Mat4::IDENTITY,
                })
                .collect(),
            draws: vec![(object, 0), (child, 1)],
            suppressed_draws: vec![object],
            materials: vec![],

            depth_materials: Vec::new(),
            red_materials: Vec::new(),
            red_depth_materials: Vec::new(),
            warm: vec![],

            bound: true,
            disabled: false,
        };
        for layer in [WARM_LAYER, LAYER, WARM_LAYER] {
            part.layer(&mut world.commands(), layer, true);
            world.flush();
            let expected_object = if layer == WARM_LAYER {
                RenderLayers::layer(layer)
            } else {
                RenderLayers::none()
            };
            assert_eq!(world.get::<RenderLayers>(object).unwrap(), &expected_object);
            for entity in [root, child] {
                assert_eq!(
                    world.get::<RenderLayers>(entity).unwrap(),
                    &RenderLayers::layer(layer)
                );
            }
            for entity in [root, object, child] {
                assert_eq!(
                    world.get::<Visibility>(entity).unwrap(),
                    &Visibility::Inherited
                );
            }
            assert_eq!(part.draws, [(object, 0), (child, 1)]);
        }
    }

    #[test]
    fn body_outline_constant_color_is_independent_of_primary_tint_and_weapon_default() -> Result<()>
    {
        let mut surfaces = Assets::<TitleSurface>::default();
        let handle = surfaces.add(TitleSurface {
            constant_color: true,
            ..Default::default()
        });
        let textured = surfaces.add(TitleSurface::default());
        let mut part = Part {
            spec: PreviewPart {
                scene: serde_json::from_value(serde_json::json!({
                    "resource": 0, "mesh": "unused.glb", "textures": [], "materials": [],
                    "translation": [0, 0, 0], "clips": [], "autoplay": false,
                    "texture_animations": [], "outline_color": [0, 0, 0, 127]
                }))?,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: vec![],
            }
            .into(),
            gltf: Handle::default(),
            textures: vec![],
            root: None,
            nodes: vec![],
            draws: vec![],
            suppressed_draws: vec![],
            materials: vec![handle.clone(), textured.clone()],

            depth_materials: Vec::new(),
            red_materials: Vec::new(),
            red_depth_materials: Vec::new(),
            warm: vec![],

            bound: true,
            disabled: false,
        };
        let body = [192, 64, 64, 173];
        let color = [128, 0, 0, 86];
        part.tint(
            body,
            Some(color),
            ModelMaterial::Normal,
            true,
            [123., 456., 789.],
            true,
            &mut surfaces,
        )?;
        let surface = surfaces.get(&handle).unwrap();
        assert_eq!(
            surface.tint,
            Vec4::from_array(color.map(|v| f32::from(v) / 255.))
        );
        assert_eq!(surface.ambient_scale, Vec3::ONE);
        assert_eq!(surface.field_light, Vec4::ZERO);
        assert!(surface.constant_color);
        // Textured surfaces within the same outline layer retain their colors and opacity.
        assert_eq!(
            surfaces.get(&textured).unwrap().tint,
            Vec4::new(3., 1., 1., 173. / 255.)
        );
        // A carried model receives its own initialized black outline, never
        // the body's red. The same generic consumer resolves its alpha255
        // sentinel by an integer shift, not a baked127/255 multiplication.
        let weapon_color = default_outline(body[3]);
        assert_eq!(weapon_color, [0, 0, 0, 86]);
        part.tint(
            body,
            Some(weapon_color),
            ModelMaterial::Normal,
            true,
            [123., 456., 789.],
            true,
            &mut surfaces,
        )?;
        assert_eq!(
            surfaces.get(&handle).unwrap().tint,
            Vec4::new(0., 0., 0., 86. / 255.)
        );
        for (alpha, half) in [
            (0, 0),
            (1, 0),
            (2, 1),
            (172, 86),
            (173, 86),
            (254, 127),
            (255, 127),
        ] {
            assert_eq!(default_outline(alpha), [0, 0, 0, half]);
        }
        // Primary geometry does not become constant red just because the body
        // also supplied a second-pass color.
        Arc::make_mut(&mut part.spec).scene.outline_color = None;
        surfaces.get_mut(&handle).unwrap().constant_color = false;
        part.tint(
            body,
            Some(color),
            ModelMaterial::Normal,
            false,
            [123., 456., 789.],
            true,
            &mut surfaces,
        )?;
        assert_eq!(
            surfaces.get(&handle).unwrap().tint,
            Vec4::new(3., 1., 1., 173. / 255.)
        );
        Ok(())
    }

    #[test]
    fn battle_projection_uses_the_full_display_aspect() {
        for resolution in ["640x480", "1920x1080"] {
            let size: crate::Resolution = resolution.parse().unwrap();
            let mut projection = battle_projection();
            projection.update(size.width as f32, size.scene_height() as f32);
            let Projection::Custom(p) = projection else {
                unreachable!()
            };
            let camera = p.get::<BattleProjection>().unwrap();
            assert!((camera.0.0.aspect_ratio - size.aspect()).abs() < 0.00001);
        }
    }

    #[test]
    fn camera_handoff_refreshes_both_actual_target_sizes() {
        use bevy::{
            render::{camera::camera_system, texture::ManualTextureViews},
            window::{WindowCreated, WindowResized, WindowScaleFactorChanged},
        };

        let mut app = App::new();
        app.add_message::<WindowResized>()
            .add_message::<WindowCreated>()
            .add_message::<WindowScaleFactorChanged>()
            .add_message::<AssetEvent<Image>>()
            .init_resource::<ManualTextureViews>()
            .init_resource::<Assets<Image>>()
            .add_systems(Update, camera_system);
        let (warm, target) = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            (
                images.add(Image::new_target_texture(
                    64,
                    64,
                    TextureFormat::Bgra8Unorm,
                    None,
                )),
                images.add(Image::new_target_texture(
                    640,
                    448,
                    TextureFormat::Bgra8Unorm,
                    None,
                )),
            )
        };
        let specifications = [
            (battle_projection as fn() -> Projection, -15, -4),
            (
                || crate::battle::overlay_projection(crate::Resolution::default()),
                -14,
                -3,
            ),
        ];
        let cameras = specifications.map(|(projection, warm_order, _)| {
            app.world_mut()
                .spawn((
                    Camera {
                        order: warm_order,
                        clear_color: ClearColorConfig::None,
                        ..default()
                    },
                    RenderTarget::Image(warm.clone().into()),
                    RenderLayers::layer(WARM_LAYER),
                    projection(),
                ))
                .id()
        });
        app.update();
        for camera in cameras {
            assert_eq!(
                app.world()
                    .get::<Camera>(camera)
                    .unwrap()
                    .physical_target_size(),
                Some(UVec2::splat(64))
            );
        }
        // Both image assets predate this handoff, so there is no image-change
        // event to accidentally repair a missing projection refresh.
        for (camera, (projection, _, order)) in cameras.into_iter().zip(specifications) {
            activate_camera(
                &mut app.world_mut().commands(),
                camera,
                RenderTarget::Image(target.clone().into()),
                projection(),
                order,
            );
        }
        app.world_mut().flush();
        for camera in cameras {
            assert_eq!(
                app.world()
                    .get::<Camera>(camera)
                    .unwrap()
                    .physical_target_size(),
                Some(UVec2::splat(64)),
                "handoff must preserve computed camera state until Bevy updates it"
            );
        }
        app.update();
        for (entity, (projection, _, order)) in cameras.into_iter().zip(specifications) {
            let camera = app.world().get::<Camera>(entity).unwrap();
            assert!(camera.is_active);
            assert_eq!(camera.order, order);
            assert!(matches!(camera.clear_color, ClearColorConfig::None));
            assert_eq!(camera.physical_target_size(), Some(UVec2::new(640, 448)));
            assert_eq!(camera.physical_viewport_size(), Some(UVec2::new(640, 448)));
            let mut expected = projection();
            expected.update(640., 448.);
            assert_eq!(camera.clip_from_view(), expected.get_clip_from_view());
            let RenderTarget::Image(actual) = app.world().get::<RenderTarget>(entity).unwrap()
            else {
                panic!("battle camera lost its image target");
            };
            assert_eq!(actual.handle.id(), target.id());
            assert_eq!(
                app.world().get::<RenderLayers>(entity).unwrap(),
                &RenderLayers::layer(LAYER)
            );
        }
    }

    #[test]
    fn missing_mesh_bindings_are_reported_before_gpu_warmup() {
        let mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        assert!(validate_draw_mesh(&mesh, &TitleSurface::default(), false).is_err());
        let mesh = mesh.with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.; 3]; 3]);
        assert!(validate_draw_mesh(&mesh, &TitleSurface::default(), false).is_ok());
        let textured = TitleSurface::textured(Some(Handle::default()));
        assert!(validate_draw_mesh(&mesh, &textured, false).is_err());
        let mesh = mesh.with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.; 2]; 3]);
        assert!(validate_draw_mesh(&mesh, &textured, false).is_ok());
        assert!(validate_draw_mesh(&mesh, &textured, true).is_err());
    }

    #[test]
    fn tolerant_gpu_failures_are_diagnosed_without_stopping_healthy_draws() {
        let diagnostics = Diagnostics::new(false);
        let mut report = Report {
            diagnostics: Some(diagnostics.clone()),
            ..default()
        };
        report.draws.armed = true;
        for message in ["missing draw pipeline", "undeclared asset read"] {
            report.draws.error = Some(message.into());
            report.recover();
            assert!(report.check().is_ok());
            assert!(report.draws.armed);
        }
        assert_eq!(diagnostics.entries().len(), 2);
    }

    #[test]
    fn paranoid_gpu_guard_does_not_clear_a_failed_draw() {
        let mut report = Report {
            diagnostics: Some(Diagnostics::new(true)),
            ..default()
        };
        report.draws.error = Some("missing draw pipeline".into());
        report.recover();
        assert!(
            report
                .check()
                .unwrap_err()
                .to_string()
                .contains("missing draw pipeline")
        );
    }

    #[test]
    fn sampled_globals_preserve_singular_bones_and_geometry_suffixes() {
        let mut world = World::new();
        let root = world.spawn(GlobalTransform::IDENTITY).id();
        let hidden = world.spawn(GlobalTransform::IDENTITY).id();
        let mesh = world.spawn(GlobalTransform::IDENTITY).id();
        // An authored zero scale cannot be inverted to reconstruct local TRS.
        // The sibling affine pose also contains shear, which must reach skinning.
        let bones = [
            Mat4::from_scale(Vec3::ZERO).to_cols_array_2d(),
            Mat4::from_cols(
                Vec4::new(2., 0., 0., 0.),
                Vec4::new(0.5, 1., 0., 0.),
                Vec4::Z,
                Vec4::new(3., 4., 5., 1.),
            )
            .to_cols_array_2d(),
        ];
        let placement = Mat4::from_translation(Vec3::new(10., 20., 30.));
        let nodes = vec![
            Node {
                entity: hidden,
                bone: Some(0),
                suffix: Mat4::IDENTITY,
            },
            Node {
                entity: mesh,
                bone: Some(1),
                suffix: Mat4::from_translation(Vec3::Y),
            },
        ];
        world
            .run_system_once(move |mut globals: Query<&mut GlobalTransform>| {
                apply_nodes(root, &nodes, placement, &bones, &mut globals).unwrap();
            })
            .unwrap();
        assert_eq!(
            world.get::<GlobalTransform>(hidden).unwrap().translation(),
            Vec3::new(10., 20., 30.)
        );
        let drawn = world.get::<GlobalTransform>(mesh).unwrap().to_matrix();
        assert_eq!(
            drawn.transform_point3(Vec3::ZERO),
            Vec3::new(13.5, 25., 35.)
        );
        assert_eq!(drawn.transform_vector3(Vec3::Y), Vec3::new(0.5, 1., 0.));
        let held = [root, hidden, mesh]
            .map(|entity| (entity, *world.get::<GlobalTransform>(entity).unwrap()));
        // Ordinary propagation can replace globals even when the battle clock is held.
        for entity in [root, hidden, mesh] {
            *world.get_mut::<GlobalTransform>(entity).unwrap() = GlobalTransform::IDENTITY;
        }
        world
            .run_system_once(move |mut globals: Query<&mut GlobalTransform>| {
                restore_pose(&held, &mut globals).unwrap();
            })
            .unwrap();
        assert_eq!(
            world.get::<GlobalTransform>(mesh).unwrap().to_matrix(),
            drawn
        );
        assert_eq!(
            world.get::<GlobalTransform>(hidden).unwrap().translation(),
            Vec3::new(10., 20., 30.)
        );
    }
    #[test]
    fn fading_and_petrification_preserve_body_textures_and_weapon_blending() -> Result<()> {
        let mut surfaces = Assets::<TitleSurface>::default();
        let part = |outline: bool,
                    body: bool,
                    recipe: TitleSurface,
                    surfaces: &mut Assets<TitleSurface>|
         -> Result<Part> {
            let binding = |texture| {
                serde_json::json!({
                    "texture": texture, "wrap_u": "repeat", "wrap_v": "repeat",
                    "nearest_min": false, "nearest_mag": false
                })
            };
            let spec = PreviewPart {
                scene: serde_json::from_value(serde_json::json!({
                    "resource": 0, "mesh": "unused.glb", "textures": ["color.ktx2", "multiply.ktx2"],
                    "materials": [{
                        "meshes": [0],
                        "color": (!outline).then(|| binding(0)),
                        "multiply": (!outline).then(|| binding(1)),
                        "vertex_color": !outline, "blend": true, "depth_write": true,
                        "cull": if outline { "front" } else { "back" }
                    }],
                    "translation": [0, 0, 0], "clips": [], "autoplay": false,
                    "texture_animations": [], "outline_color": outline.then_some([0, 0, 0, 127])
                }))?,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: vec![[0.125, 0.25, 0.375, 0.5]],
            };
            let mut part = Part::unbound(spec.into(), Handle::default(), vec![]);
            part.prepare_materials(vec![recipe], body, surfaces);
            Ok(part)
        };
        let mut body = part(
            false,
            true,
            TitleSurface {
                depth_write: true,
                blend: Some(Blend::Alpha),
                toon_ramp: Some(Handle::default()),
                ..Default::default()
            },
            &mut surfaces,
        )?;
        let outline = part(
            true,
            true,
            TitleSurface {
                depth_write: true,
                constant_color: true,
                blend: Some(Blend::Alpha),
                cull: resonance_content::CullFace::Front,
                ..Default::default()
            },
            &mut surfaces,
        )?;
        let channels = [
            resonance_content::AtlasChannel {
                texture: 0,
                frames: 4,
            },
            resonance_content::AtlasChannel {
                texture: 1,
                frames: 8,
            },
        ];
        // Petrification, fading, and curing retain expression and lighting updates.
        for (material, depth, alpha) in [
            (ModelMaterial::RedChannel, true, 77),
            (ModelMaterial::RedChannel, false, 64),
            (ModelMaterial::Normal, false, 248),
            (ModelMaterial::Normal, true, 255),
        ] {
            body.tint(
                [192, 128, 128, alpha],
                None,
                material,
                true,
                [150., 300., 200.],
                depth,
                &mut surfaces,
            )?;
            body.expression(&channels, [2, 3, 0, 0], 0, material, depth, &mut surfaces)?;
            let surface = surfaces
                .get(&body.appearance_materials(material, depth)?[0])
                .unwrap();
            assert_eq!(surface.depth_write, depth);
            assert_eq!(surface.red_channel, material == ModelMaterial::RedChannel);
            assert_eq!(surface.ambient_scale, Vec3::new(3., 2., 2.));
            assert_eq!(surface.tint, Vec4::new(1., 1., 1., f32::from(alpha) / 255.));
            assert_eq!(surface.field_light, Vec4::new(150., 300., 200., 192.));
            assert_eq!(surface.uv_offsets, Vec4::new(0.125, 0.75, 0.375, 0.875));
            assert_eq!(surface.blend, Some(Blend::Alpha));

            outline.tint(
                [192, 128, 128, alpha],
                Some([0, 0, 0, 38]),
                material,
                true,
                [150., 300., 200.],
                depth,
                &mut surfaces,
            )?;
            let surface = surfaces
                .get(&outline.appearance_materials(material, depth)?[0])
                .unwrap();
            assert!(!surface.red_channel);
            assert_eq!(surface.depth_write, depth);
            assert_eq!(surface.cull, resonance_content::CullFace::Front);
            assert_eq!(surface.ambient_scale, Vec3::ONE);
            assert_eq!(surface.field_light, Vec4::ZERO);
            assert_eq!(surface.tint, Vec4::new(0., 0., 0., 38. / 255.));
        }
        // Wing scrolling changes only the selected texture stage.
        body.scroll_uv(
            ModelMaterial::Normal,
            false,
            Some(0),
            [0.5, 0.],
            &mut surfaces,
        )?;
        assert_eq!(
            surfaces.get(&body.depth_materials[0]).unwrap().uv_offsets,
            Vec4::new(0.625, 0.25, 0.375, 0.5)
        );
        // Texture animation composes with expression layers and base offsets.
        Arc::make_mut(&mut body.spec).scene.texture_animations = vec![
            resonance_content::TextureAnimation {
                texture: 0,
                delay_ticks: 0,
                loop_start: 0,
                offsets: vec![[0.; 2], [0.125, 0.25]],
            },
            resonance_content::TextureAnimation {
                texture: 1,
                delay_ticks: 0,
                loop_start: 0,
                offsets: vec![[0.; 2], [0.25, 0.5]],
            },
        ];
        body.expression(
            &channels,
            [2, 3, 0, 0],
            1,
            ModelMaterial::Normal,
            false,
            &mut surfaces,
        )?;
        assert_eq!(
            surfaces.get(&body.depth_materials[0]).unwrap().uv_offsets,
            Vec4::new(0.25, 1., 0.625, 1.375)
        );

        let weapon = part(
            false,
            false,
            TitleSurface {
                blend: Some(Blend::Additive),
                depth_write: false,
                depth_equal: true,
                cull: resonance_content::CullFace::None,
                ..Default::default()
            },
            &mut surfaces,
        )?;
        weapon.tint(
            [64, 64, 64, 101],
            None,
            ModelMaterial::RedChannel,
            false,
            [150., 300., 200.],
            true,
            &mut surfaces,
        )?;
        let surface = surfaces
            .get(&weapon.appearance_materials(ModelMaterial::RedChannel, true)?[0])
            .unwrap();
        assert!(surface.red_channel && surface.depth_equal);
        assert!(!surface.depth_write);
        assert_eq!(surface.blend, Some(Blend::Additive));
        assert_eq!(surface.cull, resonance_content::CullFace::None);
        assert_eq!(surface.tint, Vec4::new(1., 1., 1., 101. / 255.));
        Ok(())
    }
}
