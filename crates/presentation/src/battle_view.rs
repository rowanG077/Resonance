//! Prepared battle geometry driven exclusively by the simulation's held poses.
//! Model matrices already map cooked Z-up bones into battle Y-up coordinates.
mod effects;
mod feedback;
mod markers;
mod refraction;
mod trails;
use crate::{
    draw_order::DrawOrder,
    materials::{MaterialSlot, TitleSurface},
    model_preview::gpu,
};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{
        CameraProjection, RenderTarget, SubCameraView,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d, tonemapping::Tonemapping},
    ecs::system::SystemParam,
    gltf::Gltf,
    image::{ImageLoaderSettings, ImageSampler},
    mesh::skinning::SkinnedMesh,
    prelude::*,
    render::{
        render_phase::ViewSortedRenderPhases,
        render_resource::{CachedPipelineState, PipelineCache, TextureFormat},
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
    },
    world_serialization::WorldInstanceReady,
};
pub(super) use effects::EffectBank;
use effects::ModelStyle;
use resonance_battle::{ActorId, BattleFrame, ParticleId};
use resonance_content::{
    animation::{Matrix, Skeleton},
    battle_stage::Stage,
    diagnostics::Diagnostics,
    model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
pub(super) use trails::TrailAsset;

pub(super) const LAYER: usize = 28;
pub(super) const WARM_LAYER: usize = 27;
const ACTORS: u32 = 1 << 20;
// 145C finishes fixed stage layers before 1878 draws global effects.
const LATE_STAGE: u32 = crate::draw_order::EFFECTS - 32768;
const SHADOWS: u32 = 1 << 19;
const ACTOR_STRIDE: u32 = 1 << 16;

struct ActorOrder(Vec<u32>);
impl ActorOrder {
    fn new(frame: &BattleFrame) -> Result<Self> {
        let camera = frame.camera.context("battle frame has no camera")?;
        let mut depths: Vec<_> = frame
            .actors
            .iter()
            .enumerate()
            .map(|(index, actor)| {
                (
                    index,
                    resonance_battle::project_depth(camera, actor.body.target_center),
                )
            })
            .collect();
        // 495E4 inserts later actors before an equal-depth earlier entry.
        depths.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
        let mut order = vec![0; depths.len()];
        for (rank, (index, _)) in depths.into_iter().enumerate() {
            order[index] = ACTORS + rank as u32 * ACTOR_STRIDE;
        }
        Ok(Self(order))
    }
    fn get(&self, actor: ActorId) -> Result<u32> {
        self.0
            .get(actor.index())
            .copied()
            .context("battle draw references a missing actor")
    }
}

/// One resource can have several simultaneous actors or model particles. The
/// encounter declares its maximum instance count before any battle is active.
pub(super) struct ModelAsset {
    pub resource: u32,
    pub skeleton: Skeleton,
    pub parts: Vec<PreviewPart>,
    pub capacity: usize,
    pub lit: bool,
    pub effect: bool,
    pub texture_channels: Vec<resonance_content::battle_profile::TextureChannel>,
    pub weapon_flags: Option<u8>,
    pub suppressed_nodes: BTreeSet<usize>,
}

#[derive(Resource, Clone, Default)]
pub(super) struct Warmup(Arc<Mutex<Report>>);

#[derive(Default)]
struct Report {
    diagnostics: Option<Diagnostics>,
    discarded: BTreeSet<Entity>,
    screen_failed: bool,
    draws: gpu::Report,
    scene_encoded: Arc<AtomicBool>,
    seal_requested: bool,
    prepared_pipelines: Option<usize>,
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

    /// Cache entries have stable, append-only IDs. Check the whole cache so
    /// engine compute passes and retained output passes cannot escape the guard.
    /// Returns true only on the visit which seals the final target.
    fn observe(&mut self, count: usize, pending: Option<&str>, late_reads: u64) -> bool {
        if !self.draws.armed || self.draws.error.is_some() {
            return false;
        }
        if late_reads != 0 {
            self.draws.error = Some(format!(
                "battle attempted a late or undeclared asset read ({late_reads})"
            ));
        } else if let Some(prepared) = self.prepared_pipelines {
            if count != prepared {
                self.draws.error = Some(format!(
                    "battle pipeline cache changed after activation (prepared {prepared}, now {count})"
                ));
            } else if let Some(pending) = pending {
                self.draws.error = Some(format!(
                    "battle pipeline became unprepared after activation: {pending}"
                ));
            }
        } else if self.seal_requested
            && self.draws.completed.load(Ordering::Acquire)
            && pending.is_none()
        {
            self.prepared_pipelines = Some(count);
            return true;
        }
        false
    }

    fn disarm(&mut self) {
        self.draws.armed = false;
        self.seal_requested = false;
        self.prepared_pipelines = None;
    }

    fn check(&self) -> Result<()> {
        if let Some(error) = &self.draws.error {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}

pub(super) fn install(app: &mut App) {
    feedback::install(app);
    refraction::install(app);
    let warmup = Warmup::default();
    app.insert_resource(warmup.clone());
    app.sub_app_mut(bevy::render::RenderApp)
        .insert_resource(warmup)
        .add_systems(
            bevy::render::Render,
            rendered.in_set(bevy::render::RenderSystems::Cleanup),
        );
}

#[allow(clippy::too_many_arguments)] // Shared render fence, pipeline cache and screen draw ownership.
fn rendered(
    warmup: Res<Warmup>,
    resident: Res<crate::loading::Resident>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    quads: Res<ViewSortedRenderPhases<Transparent2d>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    screens: Query<(&refraction::ScreenDraw, &MainEntity)>,
) {
    let mut report = warmup.0.lock().unwrap();
    report.recover();
    if !report.draws.armed || report.draws.error.is_some() {
        return;
    }
    if report.tolerant() {
        for (entity, pipeline) in crate::field_warm::draws(&phases, &quads) {
            if let CachedPipelineState::Err(error) = cache.get_render_pipeline_state(pipeline) {
                let _ = report
                    .diagnostics
                    .as_ref()
                    .unwrap()
                    .report("battle draw pipeline", anyhow::anyhow!("{error}"));
                report.discarded.insert(entity.id());
            }
        }
        let mut discarded = report.discarded.clone();
        if report.screen_failed {
            discarded.extend(
                screens
                    .iter()
                    .filter_map(|(screen, entity)| screen.0.then_some(entity.id())),
            );
        }
        report
            .draws
            .expected
            .retain(|entity| !discarded.contains(&entity.id()));
        if report.draws.expected.is_empty() {
            report.draws.completed.store(true, Ordering::Release);
        }
    }
    if report.scene_encoded.load(Ordering::Acquire) {
        gpu::render_report(&mut report.draws, &phases, &quads, &cache, &device, &queue);
    }
    // Cleanup follows Bevy's process_queue and render submission, so newly
    // queued IDs are visible even while asynchronous creation is still pending.
    let count = cache.pipelines().count();
    if report.tolerant() {
        for (id, pipeline) in cache.pipelines().enumerate() {
            if let CachedPipelineState::Err(error) = &pipeline.state {
                let _ = report.diagnostics.as_ref().unwrap().report(
                    "battle pipeline unavailable",
                    anyhow::anyhow!("id {id}: {error}"),
                );
            }
        }
    }
    let pending = cache.pipelines().enumerate().find_map(|(id, pipeline)| {
        (!matches!(pipeline.state, CachedPipelineState::Ok(_))
            && !(report.tolerant() && matches!(pipeline.state, CachedPipelineState::Err(_))))
        .then(|| format!("id {id}: {:?}", pipeline.state))
    });
    let late_reads = resident.late_reads.load(Ordering::Relaxed);
    if report.observe(count, pending.as_deref(), late_reads) {
        info!(
            "Battle final-target GPU preparation sealed: {} draws, {count} pipelines, late_reads={late_reads}",
            report.draws.expected.len()
        );
    }
    report.recover();
    if report.tolerant()
        && report.seal_requested
        && report.draws.completed.load(Ordering::Acquire)
        && pending.is_none()
    {
        report.prepared_pipelines = Some(count);
    }
}

#[derive(Component)]
struct Instantiated;

#[derive(SystemParam)]
pub(super) struct AssetsForView<'w> {
    pub images: ResMut<'w, Assets<Image>>,
    pub surfaces: ResMut<'w, Assets<TitleSurface>>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub gltfs: Res<'w, Assets<Gltf>>,
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
    spec: PreviewPart,
    gltf: Handle<Gltf>,
    textures: Vec<Handle<Image>>,
    root: Option<Entity>,
    nodes: Vec<Node>,
    draws: Vec<(Entity, usize)>,
    suppressed_draws: Vec<Entity>,
    materials: Vec<Handle<TitleSurface>>,
    variants: BTreeMap<ModelStyle, Vec<Handle<TitleSurface>>>,
    warm: Vec<Entity>,
    orders: Vec<u32>,
    bound: bool,
    disabled: bool,
}
struct Instance {
    key: Option<Key>,
    parts: Vec<Part>,
}
struct Model {
    skeleton: Skeleton,
    lit: bool,
    effect: bool,
    texture_channels: Vec<resonance_content::battle_profile::TextureChannel>,
    weapon_flags: Option<u8>,
    suppressed_nodes: BTreeSet<usize>,
    instances: Vec<Instance>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Actor(ActorId),
    Weapon(ActorId, u8),
    Particle(ParticleId),
}

pub(super) struct View {
    diagnostics: Diagnostics,
    stage: Stage,
    scenery: Vec<(u8, Part)>,
    models: BTreeMap<u32, Model>,
    effects: effects::Effects,
    trails: trails::Trails,
    markers: Option<markers::Markers>,
    feedback: Option<feedback::Feedback>,
    capture: Option<refraction::Capture>,
    toon: Handle<Image>,
    sampled: crate::scene::SampledImages,
    camera: Option<Entity>,
    warm_target: Option<Handle<Image>>,
    warmup: Option<Warmup>,
    prepared: bool,
    active: bool,
}

impl View {
    /// The AssetServer must already use the candidate's verified `Files` through
    /// the ordinary resident reader. This method never opens the filesystem.
    #[cfg(test)]
    pub fn load(
        stage: Stage,
        models: Vec<ModelAsset>,
        toon: Handle<Image>,
        effect_banks: Vec<EffectBank>,
        trail_assets: Vec<TrailAsset>,
        ui: &resonance_content::battle_ui::Art,
        server: &AssetServer,
    ) -> Result<Self> {
        Self::load_with_diagnostics(
            stage,
            models,
            toon,
            effect_banks,
            trail_assets,
            ui,
            server,
            Diagnostics::new(true),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn load_with_diagnostics(
        stage: Stage,
        models: Vec<ModelAsset>,
        toon: Handle<Image>,
        effect_banks: Vec<EffectBank>,
        trail_assets: Vec<TrailAsset>,
        ui: &resonance_content::battle_ui::Art,
        server: &AssetServer,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let stage_valid = diagnostics
            .attempt("battle stage", stage.validate())?
            .is_some();
        let scenery = stage
            .layers
            .iter()
            .filter(|(slot, _)| stage_valid && matches!(**slot, 0..=3))
            .map(|(&slot, part)| (slot, Part::load(part.clone(), server)))
            .collect();
        let mut prepared = BTreeMap::new();
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
                    model.capacity > 0 && !model.parts.is_empty(),
                    "empty battle model {}",
                    model.resource
                );
                let mut parts = Vec::new();
                for part in model.parts {
                    let validation = (|| -> Result<()> {
                        ensure!(
                            part.attached_to.is_none(),
                            "battle model attachments require a prepared sampled resource"
                        );
                        ensure!(
                            part.scene.bone_names.len() == model.skeleton.bones.len(),
                            "battle model skeleton differs from geometry"
                        );
                        ensure!(
                            part.scene.texture_animations.is_empty(),
                            "battle model texture animation is not prepared"
                        );
                        Ok(())
                    })();
                    if diagnostics
                        .attempt(
                            &format!("battle model part {}", part.scene.mesh),
                            validation,
                        )?
                        .is_some()
                    {
                        parts.push(part);
                    }
                }
                let orders: BTreeSet<_> = parts
                    .iter()
                    .flat_map(|p| &p.scene.materials)
                    .map(|m| m.draw_order)
                    .collect();
                ensure!(orders.len() < 1024, "too many battle model material passes");
                let orders: BTreeMap<_, _> = orders
                    .into_iter()
                    .enumerate()
                    .map(|(index, order)| (order, index as u32))
                    .collect();
                let instances = (0..model.capacity)
                    .map(|_| Instance {
                        key: None,
                        parts: parts
                            .iter()
                            .cloned()
                            .map(|p| {
                                let mut part = Part::load(p, server);
                                part.orders = part
                                    .spec
                                    .scene
                                    .materials
                                    .iter()
                                    .map(|m| orders[&m.draw_order])
                                    .collect();
                                part
                            })
                            .collect(),
                    })
                    .collect();
                ensure!(
                    !prepared.contains_key(&model.resource),
                    "duplicate battle render resource {}",
                    model.resource
                );
                ensure!(
                    prepared
                        .insert(
                            model.resource,
                            Model {
                                skeleton: model.skeleton,
                                lit: model.lit,
                                effect: model.effect,
                                texture_channels: model.texture_channels,
                                weapon_flags: model.weapon_flags,
                                suppressed_nodes: model.suppressed_nodes,
                                instances
                            }
                        )
                        .is_none(),
                    "duplicate battle render resource {}",
                    model.resource
                );
                Ok(())
            })();
            if let Err(error) = result {
                diagnostics.report(&format!("battle render model {resource}"), error)?;
            }
        }
        let mut effects =
            effects::Effects::load(effect_banks, &ui.sine, server, diagnostics.clone())?;
        if let Err(error) = effects.set_shadow_circle(ui.markers.shadow_circle) {
            diagnostics.report("battle shadow circle", error)?;
            effects.disable_shadows();
        }
        Ok(Self {
            stage,
            scenery,
            models: prepared,
            effects,
            trails: trails::Trails::load(trail_assets, server, diagnostics.clone())?,
            markers: diagnostics.attempt(
                "battle world markers",
                markers::Markers::load(ui.markers.clone(), server, diagnostics.clone()),
            )?,
            diagnostics,
            feedback: None,
            capture: None,
            toon,
            sampled: Default::default(),
            camera: None,
            warm_target: None,
            warmup: None,
            prepared: false,
            active: false,
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
            .chain(self.effects.images())
            .chain(self.trails.images())
            .chain(self.markers.iter().flat_map(markers::Markers::images))
    }

    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.scenery
            .iter()
            .flat_map(|(_, p)| p.draws.iter().map(|&(e, _)| e))
            .chain(
                self.models
                    .values()
                    .flat_map(|m| &m.instances)
                    .flat_map(|i| &i.parts)
                    .flat_map(|p| {
                        p.draws
                            .iter()
                            .map(|&(e, _)| e)
                            .chain(p.warm.iter().copied())
                    }),
            )
            .chain(self.effects.entities())
            .chain(self.trails.entities())
            .chain(self.markers.iter().flat_map(markers::Markers::entities))
    }

    /// Poll loading and the shared submitted-draw fence. Every instance and
    /// material variant is drawn to an isolated target before activation.
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
                        Camera3d::default(),
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
            let feedback = feedback::Feedback::new(&mut assets.images);
            feedback.attach(commands, self.camera.unwrap());
            self.feedback = Some(feedback);
            let capture = refraction::Capture::new(&mut assets.images, warmup);
            capture.attach(commands, self.camera.unwrap());
            self.capture = Some(capture);
        }
        for image in self.images() {
            if assets.images.contains(image) {
                continue;
            }
            if let Err(error) = check_load(server, image.id().untyped()) {
                self.diagnostics
                    .report(&format!("battle image {:?}", image.id()), error)?;
                // A conspicuous replacement keeps the healthy draw resources usable.
                assets.images.insert(image.id(), placeholder_image())?;
            }
        }
        if !self.images().all(|h| assets.images.contains(h)) {
            return Ok(false);
        }
        for (_, part) in &mut self.scenery {
            let result = part.prepare(
                commands,
                server,
                assets,
                entities,
                &mut self.sampled,
                &self.toon,
                false,
                0,
                None,
                &BTreeSet::new(),
                None,
            );
            if let Err(error) = result {
                self.diagnostics
                    .report(&format!("battle scenery {}", part.spec.scene.mesh), error)?;
                part.disable(commands);
            }
        }
        let model_styles = self.effects.model_styles();
        for model in self.models.values_mut() {
            for instance in &mut model.instances {
                for part in &mut instance.parts {
                    let result = part.prepare(
                        commands,
                        server,
                        assets,
                        entities,
                        &mut self.sampled,
                        &self.toon,
                        model.lit,
                        model.skeleton.bones.len(),
                        model.weapon_flags,
                        &model.suppressed_nodes,
                        model.effect.then_some(&model_styles),
                    );
                    if let Err(error) = result {
                        self.diagnostics.report(
                            &format!("battle model part {}", part.spec.scene.mesh),
                            error,
                        )?;
                        part.disable(commands);
                    }
                }
            }
        }
        self.effects.prepare(
            commands,
            &mut assets.meshes,
            &mut assets.surfaces,
            &mut assets.images,
            &mut self.sampled,
            &self.capture.as_ref().unwrap().image,
        )?;
        self.effects.set_layer(commands, WARM_LAYER, true);
        self.trails.prepare(commands, assets, &mut self.sampled)?;
        self.trails.set_layer(commands, WARM_LAYER, true);
        if let Some(markers) = &mut self.markers {
            let result = markers.prepare(
                commands,
                &mut assets.meshes,
                &mut assets.surfaces,
                &mut assets.images,
                &mut self.sampled,
            );
            if let Err(error) = result {
                self.diagnostics
                    .report("battle world marker preparation", error)?;
                self.markers.take().unwrap().despawn(commands);
            } else {
                markers.set_layer(commands, WARM_LAYER, true);
            }
        }
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
            .extend(self.entities().map(MainEntity::from));
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
        self.prepared = report.completed.load(Ordering::Acquire) && self.feedback_ready()?;
        Ok(self.prepared)
    }

    pub fn warm_target(&self) -> Option<Handle<Image>> {
        self.warm_target.clone()
    }

    /// Retained field camera/roots are hidden by the caller in this same handoff.
    pub fn activate(&mut self, commands: &mut Commands, target: RenderTarget) -> Result<()> {
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
            let order = match slot {
                0 => 0,
                1 => LATE_STAGE,
                3 => LATE_STAGE + 4096,
                2 => LATE_STAGE + 8192,
                _ => continue,
            };
            for &(draw, index) in &part.draws {
                commands.entity(draw).insert(DrawOrder(
                    order + part.spec.scene.materials[index].draw_order,
                    0,
                ));
            }
        }
        for model in self.models.values() {
            for instance in &model.instances {
                for part in &instance.parts {
                    part.layer(commands, LAYER, false);
                }
            }
        }
        self.effects.set_layer(commands, LAYER, false);
        self.trails.set_layer(commands, LAYER, false);
        if let Some(markers) = &self.markers {
            markers.set_layer(commands, LAYER, false);
        }
        let feedback = self.feedback.as_mut().context("missing battle feedback")?;
        feedback.rearm();
        feedback.attach(commands, self.camera.unwrap());
        // Exercise screen materials against the actual target before sealing.
        // The initial warm target's callback owns a different completion token.
        self.effects.warm_screen(commands, LAYER, true);
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
                .chain(self.effects.screen_entities().map(MainEntity::from))
                .collect(),
            ..default()
        };
        drop(report);
        let capture = self
            .capture
            .as_mut()
            .context("missing battle scene capture")?;
        capture.rearm();
        capture.attach(commands, self.camera.unwrap());
        Ok(())
    }

    /// The final target submits first, then the render world acknowledges a
    /// fully drained pipeline cache. Simulation/audio stay held across both gates.
    pub fn active_ready(&self) -> Result<bool> {
        self.check()?;
        if !self.active || !self.feedback_ready()? {
            return Ok(false);
        }
        let mut report = self
            .warmup
            .as_ref()
            .context("missing battle preparation report")?
            .0
            .lock()
            .unwrap();
        if let Err(error) = report.check() {
            self.diagnostics.report("battle GPU", error)?;
        }
        if !report.draws.completed.load(Ordering::Acquire) {
            return Ok(false);
        }
        report.seal_requested = true;
        Ok(report.prepared_pipelines.is_some())
    }

    /// Also polled before host writeback, so render failures cannot commit a result.
    pub fn check(&self) -> Result<()> {
        if let Some(warmup) = &self.warmup
            && let Err(error) = warmup.0.lock().unwrap().check()
        {
            self.diagnostics.report("battle GPU", error)?;
        }
        if self.feedback.is_some() {
            self.feedback_ready()?;
        }
        Ok(())
    }

    fn feedback_ready(&self) -> Result<bool> {
        match self
            .feedback
            .as_ref()
            .context("missing battle feedback")?
            .ready()
        {
            Ok(ready) => Ok(ready),
            Err(error) => {
                self.diagnostics.report("battle feedback disabled", error)?;
                Ok(true)
            }
        }
    }

    pub fn apply_feedback(
        &mut self,
        commands: &mut Commands,
        frame: crate::field_ui::FeedbackFrame,
    ) -> Result<()> {
        ensure!(
            self.active_ready()?,
            "battle feedback target is still preparing"
        );
        if self.feedback.as_ref().unwrap().ready().is_err() && !self.diagnostics.paranoid() {
            self.effects.warm_screen(commands, LAYER, false);
            return Ok(());
        }
        self.feedback.as_mut().unwrap().apply(
            commands,
            self.camera.unwrap(),
            feedback::Frame {
                generation: frame.generation,
                update: u64::from(frame.update),
                amount: frame.amount,
                alpha: frame.alpha,
            },
        );
        self.effects.warm_screen(commands, LAYER, false);
        Ok(())
    }

    /// Schedule after ordinary and affine transform propagation, before Bevy's
    /// frustum/visibility systems. No animation or gameplay clock advances here.
    pub fn apply(
        &mut self,
        frame: &BattleFrame,
        commands: &mut Commands,
        globals: &mut Query<&mut GlobalTransform>,
        camera_transforms: &mut Query<&mut Transform>,
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
        *camera_transforms.get_mut(camera_entity)? = camera_pose;
        *globals.get_mut(camera_entity)? = camera_pose.into();
        let stage_world = stage_world(&self.stage);
        let order = ActorOrder::new(frame)?;
        for (slot, part) in &self.scenery {
            part.visible(commands, true);
            let result = (|| -> Result<()> {
                part.apply(
                    stage_world
                        * Mat4::from_translation(Vec3::from_array(part.spec.scene.translation)),
                    &[],
                    globals,
                )?;
                part.tint(
                    frame.stage_colors[usize::from(*slot)].unwrap_or(self.stage.color),
                    false,
                    self.stage.light_position,
                    None,
                    surfaces,
                )?;
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
                frame
                    .particles
                    .iter()
                    .filter_map(|p| p.model.as_ref().map(|m| (m.resource, Key::Particle(p.id)))),
            )
            .collect();
        for (&resource, model) in &mut self.models {
            for instance in &mut model.instances {
                for part in &instance.parts {
                    part.visible(commands, false);
                }
                if instance
                    .key
                    .is_some_and(|key| !wanted.contains(&(resource, key)))
                {
                    instance.key = None;
                    for part in &instance.parts {
                        part.visible(commands, false);
                    }
                }
            }
        }
        for model in &frame.models {
            self.model(
                model.resource,
                Key::Actor(model.actor),
                model.visible,
                model.tint,
                &model.world,
                &model.bones,
                Some(model.texture_layers),
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
                model.visible,
                model.tint,
                &model.world,
                &model.bones,
                None,
                None,
                None,
                &order,
                commands,
                globals,
                surfaces,
            )?;
        }
        for (index, particle) in frame.particles.iter().enumerate() {
            if let Some(model) = &particle.model {
                let Some((style, pass)) = self.diagnostics.attempt(
                    &format!(
                        "battle model particle {}:{}",
                        particle.resource, particle.member
                    ),
                    self.effects.model_pass(particle, &order),
                )?
                else {
                    continue;
                };
                self.model(
                    model.resource,
                    Key::Particle(particle.id),
                    true,
                    particle.state.colors[0].map(|value| value as u8),
                    &model.world,
                    &model.bones,
                    None,
                    None,
                    Some((style, pass, index)),
                    &order,
                    commands,
                    globals,
                    surfaces,
                )?;
            }
        }
        let result = self.effects.apply(
            frame,
            Mat3::from_quat(camera_pose.rotation),
            meshes,
            commands,
            &order,
        );
        if let Err(error) = result {
            self.diagnostics.report("battle effects", error)?;
        }
        let result = self
            .trails
            .apply(frame, commands, meshes, &order, &self.models);
        if let Err(error) = result {
            self.diagnostics.report("battle trails", error)?;
        }
        if let Some(markers) = &mut self.markers {
            let result = markers.apply(
                frame,
                Mat3::from_quat(camera_pose.rotation),
                meshes,
                commands,
            );
            if let Err(error) = result {
                self.diagnostics.report("battle world markers", error)?;
            }
        }
        self.hide_unavailable_effects(commands);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // One authoritative pose and the existing ECS render resources.
    fn model(
        &mut self,
        resource: u32,
        key: Key,
        visible: bool,
        tint: [u8; 4],
        world: &Matrix,
        bones: &[Matrix],
        texture_layers: Option<[u8; 4]>,
        light: Option<[f32; 3]>,
        effect: Option<(ModelStyle, u32, usize)>,
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
                bones.len() == model.skeleton.bones.len(),
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
                Key::Actor(actor) => actor_order.get(actor)? + 24576,
                Key::Weapon(actor, slot) => {
                    actor_order.get(actor)?
                        + if model.weapon_flags.is_some_and(|flags| flags & 1 != 0) {
                            8192
                        } else {
                            32768
                        }
                        + u32::from(slot) * 1024
                }
                Key::Particle(_) => {
                    effect
                        .context("model particle has no prepared draw style")?
                        .1
                }
            };
            for part in &instance.parts {
                if part.disabled {
                    continue;
                }
                let result = (|| -> Result<()> {
                    part.visible(commands, visible);
                    part.apply(Mat4::from_cols_array_2d(world), bones, globals)?;
                    part.tint(
                        tint,
                        effect.map_or(model.lit, |(style, _, _)| style.lit),
                        light.unwrap_or(self.stage.light_position),
                        effect.map(|(style, _, _)| style),
                        surfaces,
                    )?;
                    if let Some(layers) = texture_layers {
                        part.expression(&model.texture_channels, layers, surfaces)?;
                    }
                    for &(draw, index) in &part.draws {
                        if let Some((style, _, instance)) = effect {
                            let materials = part
                                .variants
                                .get(&style)
                                .context("unprepared model particle style")?;
                            commands.entity(draw).insert((
                                MeshMaterial3d(materials[index].clone()),
                                DrawOrder(order, instance * 1024 + part.orders[index] as usize),
                            ));
                        } else {
                            commands
                                .entity(draw)
                                .insert(DrawOrder(order + part.orders[index], 0));
                        }
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

    fn hide_unavailable_effects(&self, commands: &mut Commands) {
        if let Some(warmup) = &self.warmup {
            let report = warmup.0.lock().unwrap();
            // Failed pipelines are skipped by Bevy. Pool entities can select a
            // healthy material next frame, so do not permanently hide their slot.
            if report.screen_failed {
                self.effects.hide_screen(commands);
            }
        }
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
        self.effects.despawn(commands);
        self.trails.despawn(commands);
        if let Some(markers) = self.markers {
            markers.despawn(commands);
        }
    }
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
        Self {
            gltf: server.load(spec.scene.mesh.clone()),
            textures: spec
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
                .collect(),
            spec,
            root: None,
            nodes: Vec::new(),
            draws: Vec::new(),
            suppressed_draws: Vec::new(),
            materials: Vec::new(),
            variants: BTreeMap::new(),
            warm: Vec::new(),
            orders: Vec::new(),
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
        weapon_flags: Option<u8>,
        suppressed_nodes: &BTreeSet<usize>,
        effect_styles: Option<&BTreeSet<ModelStyle>>,
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
            for (index, material) in self.spec.scene.materials.iter().enumerate() {
                let binding = |binding: &resonance_content::TextureBinding| -> Result<_> {
                    Ok((
                        self.textures
                            .get(binding.texture)
                            .context("invalid battle material texture")?
                            .clone(),
                        binding.clone(),
                    ))
                };
                let color = crate::scene::sampled_image(
                    material.color.as_ref().map(binding).transpose()?,
                    &mut assets.images,
                    sampled,
                );
                let multiply = crate::scene::sampled_image(
                    material.multiply.as_ref().map(binding).transpose()?,
                    &mut assets.images,
                    sampled,
                );
                self.materials.push(
                    assets.surfaces.add(TitleSurface {
                        vertex_color: material.vertex_color,
                        multiply,
                        uv_offsets: Vec4::from_array(
                            self.spec.uv_offsets.get(index).copied().unwrap_or([0.; 4]),
                        ),
                        constant_color: self.spec.scene.outline_color.is_some(),
                        tint: outline(&self.spec),
                        toon_ramp: (lit
                            && self.spec.scene.outline_color.is_none()
                            && material.color.is_some())
                        .then(|| toon.clone()),
                        shade_colors: [49., 66.].map(|v| Vec3::splat(v / 255.).extend(1.)),
                        blend: material.blend || lit,
                        additive: self.spec.additive || weapon_flags.is_some_and(|f| f & 0x10 != 0),
                        depth_write: material.depth_write
                            && weapon_flags.is_none_or(|f| f & 0x10 == 0),
                        depth_equal: weapon_flags.is_some(),
                        cull: if weapon_flags.is_some_and(|f| f & 0x10 != 0) {
                            resonance_content::CullFace::None
                        } else {
                            material.cull
                        },
                        ..TitleSurface::textured(color)
                    }),
                );
            }
            if let Some(styles) = effect_styles {
                for &style in styles {
                    let variants = self
                        .materials
                        .iter()
                        .map(|handle| {
                            let mut surface = assets.surfaces.get(handle).unwrap().clone();
                            surface.toon_ramp =
                                (style.lit && !surface.constant_color && surface.color.is_some())
                                    .then(|| toon.clone());
                            surface.blend = true;
                            surface.additive = style.additive;
                            surface.depth_test = style.depth_test;
                            surface.depth_write = style.depth_write;
                            surface.depth_equal = true;
                            if !surface.constant_color {
                                surface.cull = if style.cull_back {
                                    resonance_content::CullFace::Back
                                } else {
                                    resonance_content::CullFace::None
                                };
                            }
                            assets.surfaces.add(surface)
                        })
                        .collect();
                    self.variants.insert(style, variants);
                }
            }
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
                        Visibility::Inherited,
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
                        DrawOrder(self.spec.scene.materials[index].draw_order, 0),
                    ));
                self.draws.push((entity, index));
                if bone.is_some_and(|bone| suppressed_nodes.contains(&bone)) {
                    // 5B528 clears the object draw byte, not the bone or its
                    // children. Keep anchors and skinning transforms available.
                    self.suppressed_draws.push(entity);
                }
                for variants in self.variants.values() {
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
        ensure!(
            !self.draws.is_empty(),
            "battle model has no drawable meshes"
        );
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

    fn tint(
        &self,
        color: [u8; 4],
        lit: bool,
        light: [f32; 3],
        style: Option<ModelStyle>,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let materials = if let Some(style) = style {
            self.variants
                .get(&style)
                .context("unprepared model particle material")?
        } else {
            &self.materials
        };
        for handle in materials {
            let surface = surfaces
                .get(handle)
                .context("prepared battle material is missing")?;
            let base = outline(&self.spec);
            let rgb = Vec3::new(
                f32::from(color[0]) / 64.,
                f32::from(color[1]) / 64.,
                f32::from(color[2]) / 64.,
            );
            let (ambient, tint, field_light) = if lit && surface.toon_ramp.is_some() {
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

    fn expression(
        &self,
        channels: &[resonance_content::battle_profile::TextureChannel],
        layers: [u8; 4],
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        for (index, handle) in self.materials.iter().enumerate() {
            let material = &self.spec.scene.materials[index];
            let mut uv = self.spec.uv_offsets.get(index).copied().unwrap_or([0.; 4]);
            for (layer, channel) in channels.iter().enumerate() {
                for (binding, axis) in [(&material.color, 1), (&material.multiply, 3)] {
                    if binding
                        .as_ref()
                        .is_some_and(|b| b.texture == usize::from(channel.texture))
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
            commands.entity(root).insert(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
    fn layer(&self, commands: &mut Commands, layer: usize, visible: bool) {
        self.visible(commands, visible);
        for &entity in &self.warm {
            commands.entity(entity).insert(Visibility::Hidden);
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
        // would also hide its children, unlike the original object draw byte.
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

fn outline(part: &PreviewPart) -> Vec4 {
    part.scene.outline_color.map_or(Vec4::ONE, |c| {
        Vec4::from_array(c.map(|v| f32::from(v) / 255.))
    })
}
pub(super) fn placeholder_image() -> Image {
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
#[derive(Debug, Clone)]
struct BattleProjection(crate::camera::TitleProjection);
impl BattleProjection {
    fn depth(&self, mut clip: Mat4) -> Mat4 {
        // 800FE38C and GXProject use a finite near/far range. Preserve the
        // shared raster alignment while converting its depth to Bevy reverse Z.
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
        self.0.update(width, height);
    }
    fn far(&self) -> f32 {
        self.0.far()
    }
    fn get_frustum_corners(&self, near: f32, far: f32) -> [bevy::math::Vec3A; 8] {
        self.0.get_frustum_corners(near, far)
    }
}
fn battle_projection() -> Projection {
    // Battle REL 53300/53484: source vertical FOV, near/far and display aspect.
    Projection::custom(BattleProjection(crate::camera::TitleProjection(
        PerspectiveProjection {
            fov: 13.33_f32.to_radians(),
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
    fn attachment_suppression_preserves_child_draws_and_warmup_layers() {
        let mut world = World::new();
        let root = world.spawn(Visibility::Inherited).id();
        let object = world.spawn((Visibility::Inherited, ChildOf(root))).id();
        let child = world.spawn((Visibility::Inherited, ChildOf(object))).id();
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
            },
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
            variants: BTreeMap::new(),
            warm: vec![],
            orders: vec![],
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
            (crate::battle::overlay_projection, -14, -3),
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
    fn tolerant_gpu_guard_records_distinct_failures_and_can_keep_observing() {
        let diagnostics = Diagnostics::new(false);
        let mut report = Report {
            diagnostics: Some(diagnostics.clone()),
            ..default()
        };
        report.draws.armed = true;
        report.draws.completed.store(true, Ordering::Release);
        report.seal_requested = true;
        assert!(report.observe(12, None, 0));
        report.observe(13, None, 0);
        report.recover();
        assert!(report.check().is_ok());
        report.observe(13, None, 1);
        report.recover();
        assert!(report.check().is_ok());
        assert_eq!(diagnostics.entries().len(), 2);
        assert!(diagnostics.entries()[0].message.contains("cache changed"));
        assert!(
            diagnostics.entries()[1]
                .message
                .contains("late or undeclared")
        );
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
    fn final_target_seal_waits_for_draw_submission_and_the_shared_cache() {
        let mut report = Report::default();
        report.draws.armed = true;
        // Completing the isolated target alone cannot activate the guard.
        report.draws.completed.store(true, Ordering::Release);
        assert!(!report.observe(20, None, 0));
        assert_eq!(report.prepared_pipelines, None);
        // active_ready requests this only after the actual-target feedback fence.
        report.seal_requested = true;
        assert!(!report.observe(22, Some("id 21: Creating"), 0));
        assert_eq!(report.prepared_pipelines, None);
        assert!(report.draws.error.is_none());
        // Retained field/title and final-target entries all belong to the seal.
        assert!(report.observe(22, None, 0));
        assert_eq!(report.prepared_pipelines, Some(22));
        for _ in 0..3 {
            assert!(!report.observe(22, None, 0));
            assert!(report.check().is_ok());
        }
        report.disarm();
        assert!(!report.observe(30, Some("field pipeline Creating"), 0));
        assert!(report.check().is_ok());
        assert_eq!(report.prepared_pipelines, None);

        let mut next = Report::default();
        next.draws.armed = true;
        next.seal_requested = true;
        assert!(!next.observe(30, None, 0));
        assert_eq!(next.prepared_pipelines, None);
        next.draws.completed.store(true, Ordering::Release);
        assert!(next.observe(30, None, 0));
    }

    #[test]
    fn active_guard_rejects_cache_growth_recreation_and_late_reads_after_completed_fence() {
        for (count, pending, late_reads, expected) in [
            (13, Some("id 12: Queued"), 0, "cache changed"),
            (13, None, 0, "cache changed"),
            (12, Some("id 4: Creating"), 0, "became unprepared"),
            (12, None, 1, "late or undeclared asset read"),
        ] {
            let mut report = Report::default();
            report.draws.armed = true;
            report.draws.completed.store(true, Ordering::Release);
            report.seal_requested = true;
            assert!(report.observe(12, None, 0));
            assert!(!report.observe(count, pending, late_reads));
            let first = report.check().unwrap_err().to_string();
            assert!(first.contains(expected), "{first}");
            // Subsequent failures and a restored field cannot replace the cause.
            report.observe(20, Some("another pipeline failed"), 2);
            report.disarm();
            report.observe(30, None, 3);
            assert_eq!(report.check().unwrap_err().to_string(), first);
        }
    }

    #[test]
    fn render_projection_preserves_native_finite_depth_range() {
        let projection = battle_projection().get_clip_from_view();
        let camera = resonance_battle::CameraPose {
            eye: [0.; 3],
            focus: [0., 0., -1.],
            pitch: 0.,
            yaw: 0.,
            radius: 1.,
        };
        for depth in [50., 100., 4000., 21288.] {
            let point = [0., 0., -depth];
            let clip = projection * Vec3::from_array(point).extend(1.);
            let native = resonance_battle::project_depth(camera, point);
            assert!((clip.z / clip.w - (1. - native)).abs() < 0.000001);
        }
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
    }
}
