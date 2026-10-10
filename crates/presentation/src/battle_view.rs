//! Prepared battle geometry drawn from gameplay poses and scene-owned particles.
//! Model matrices already map cooked Z-up bones into battle Y-up coordinates.
use resonance_events::effect::Blend;
use resonance_game::battle::encounter::{RenderEffect, RenderModel, RenderTrail};
mod effects;
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

pub(super) const LAYER: usize = 28;
pub(super) const WARM_LAYER: usize = 27;
fn weapon_layer(actor: usize, slot: u8, style: Option<WeaponStyle>, overlay: bool) -> Layer {
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

struct ActorOrder(Vec<usize>);
impl ActorOrder {
    fn new(frame: &BattleFrame) -> Result<Self> {
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
            .collect();
        // Later actors precede earlier actors when depths are equal.
        depths.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
        let mut order = vec![0; depths.len()];
        for (rank, (index, _)) in depths.into_iter().enumerate() {
            order[index] = rank;
        }
        Ok(Self(order))
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

    fn get(&self, actor: ActorId) -> Result<usize> {
        self.0
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
struct Model {
    skeleton: Skeleton,
    lit: bool,
    effect: bool,
    texture_channels: Vec<resonance_content::battle_profile::TextureChannel>,
    weapon_style: Option<WeaponStyle>,
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
    toon: Handle<Image>,
    sampled: crate::scene::SampledImages,
    camera: Option<Entity>,
    warm_target: Option<Handle<Image>>,
    warmup: Option<Warmup>,
    prepared: bool,
    active: bool,
}

impl View {
    #[allow(clippy::too_many_arguments)]
    pub fn load_with_diagnostics(
        stage: Stage,
        models: Vec<RenderModel>,
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
                let instances = (0..model.capacity)
                    .map(|_| Instance {
                        key: None,
                        parts: parts
                            .iter()
                            .cloned()
                            .map(|part| Part::load(part, server))
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
                                weapon_style: model.weapon_style,
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
        Ok(Self {
            stage,
            scenery,
            models: prepared,
            effects: effects::Effects::load(effect_banks, server, diagnostics.clone())?,
            trails: trails::Trails::load(trail_assets, server, diagnostics.clone())?,
            diagnostics,
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
            .chain(self.effects.images_mut())
            .chain(self.trails.images_mut())
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
        }
        let diagnostics = self.diagnostics.clone();
        recover_images(
            self.images_mut(),
            server,
            &mut assets.images,
            &diagnostics,
            "battle image",
        )?;
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
                false,
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
                        model.weapon_style,
                        &model.suppressed_nodes,
                        model.effect.then_some(&model_styles),
                        !model.effect && model.weapon_style.is_none(),
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
        )?;
        self.effects.set_layer(commands, WARM_LAYER, true);
        self.trails.prepare(commands, assets, &mut self.sampled)?;
        self.trails.set_layer(commands, WARM_LAYER, true);
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
        self.prepared = report.completed.load(Ordering::Acquire);
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
            let layer = if *slot == 0 {
                Layer::Scene(0)
            } else {
                Layer::Foreground(*slot)
            };
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
                }
            }
        }
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

    /// Schedule after ordinary and affine transform propagation, before Bevy's
    /// frustum/visibility systems. No animation or gameplay clock advances here.
    pub fn advance_trails(&mut self, frame: &BattleFrame, paused: bool) -> Result<()> {
        self.trails.advance(frame, paused)
    }

    pub fn apply(
        &mut self,
        (frame, particles): (&BattleFrame, &[resonance_battle::ParticleFrame]),
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
        for (_, part) in &self.scenery {
            part.visible(commands, true);
            let result = (|| -> Result<()> {
                part.apply(
                    stage_world
                        * Mat4::from_translation(Vec3::from_array(part.spec.scene.translation)),
                    &[],
                    globals,
                )?;
                part.tint(
                    self.stage.color,
                    None,
                    if frame.hourglass_remaining != 0 {
                        ModelMaterial::RedChannel
                    } else {
                        ModelMaterial::Normal
                    },
                    false,
                    self.stage.light_position,
                    None,
                    true,
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
                particles
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
                Some(crate::battle::appearance::outline(
                    &frame.actors[model.actor.index()],
                    frame.recognized_result.is_none(),
                    model.tint[3],
                )),
                model.material,
                model.depth_write,
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
                Some(default_outline(model.tint[3])),
                model.material,
                true,
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
        for (index, particle) in particles.iter().enumerate() {
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
                    None,
                    ModelMaterial::Normal,
                    true,
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
            particles,
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
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // One authoritative pose and the existing ECS render resources.
    fn model(
        &mut self,
        resource: u32,
        key: Key,
        visible: bool,
        tint: [u8; 4],
        outline_tint: Option<[u8; 4]>,
        material: ModelMaterial,
        depth_write: bool,
        world: &Matrix,
        bones: &[Matrix],
        texture_layers: Option<[u8; 4]>,
        light: Option<[f32; 3]>,
        effect: Option<(ModelStyle, Layer, usize)>,
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
                Key::Actor(actor) => Layer::Actor(actor_order.get(actor)?, ActorLayer::Body),
                Key::Weapon(actor, slot) => {
                    weapon_layer(actor_order.get(actor)?, slot, model.weapon_style, false)
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
                        outline_tint,
                        material,
                        effect.map_or(model.lit, |(style, _, _)| style.lit),
                        light.unwrap_or(self.stage.light_position),
                        effect.map(|(style, _, _)| style),
                        depth_write,
                        surfaces,
                    )?;
                    if let Some(layers) = texture_layers {
                        part.expression(
                            &model.texture_channels,
                            layers,
                            material,
                            depth_write,
                            surfaces,
                        )?;
                    }
                    for &(draw, index) in &part.draws {
                        if let Some((style, _, instance)) = effect {
                            let materials = part
                                .variants
                                .get(&style)
                                .context("unprepared model particle style")?;
                            commands.entity(draw).insert((
                                MeshMaterial3d(materials[index].clone()),
                                DrawOrder(
                                    order,
                                    instance,
                                    part.spec.scene.materials[index].draw_order,
                                ),
                            ));
                        } else {
                            commands.entity(draw).insert((
                                MeshMaterial3d(
                                    part.appearance_materials(None, material, depth_write)?[index]
                                        .clone(),
                                ),
                                DrawOrder(order, 0, part.spec.scene.materials[index].draw_order),
                            ));
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
        effect_styles: Option<&BTreeSet<ModelStyle>>,
        body: bool,
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
                let mut surface = TitleSurface {
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
                    blend: if self.spec.additive {
                        Some(Blend::Additive)
                    } else {
                        (material.blend || lit).then_some(Blend::Alpha)
                    },
                    depth_write: material.depth_write,
                    cull: material.cull,
                    ..TitleSurface::textured(color)
                };
                apply_weapon_style(&mut surface, weapon_style);
                self.materials.push(assets.surfaces.add(surface));
            }
            if body {
                self.prepare_body_depth(&mut assets.surfaces)?;
            }
            if body || weapon_style.is_some() {
                self.prepare_red_materials(&mut assets.surfaces)?;
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
                            surface.blend = Some(if style.additive {
                                Blend::Additive
                            } else {
                                Blend::Alpha
                            });
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
                let warm_materials: Vec<_> = self
                    .warm_materials()
                    .map(|variants| variants[index].clone())
                    .collect();
                for material in warm_materials {
                    let mut warm = commands.spawn((
                        mesh.clone(),
                        MeshMaterial3d(material),
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
        style: Option<ModelStyle>,
        depth_write: bool,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let materials = self.appearance_materials(style, material, depth_write)?;
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
            let (ambient, tint, field_light) = if self.spec.scene.outline_color.is_some()
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

    fn prepare_body_depth(&mut self, surfaces: &mut Assets<TitleSurface>) -> Result<()> {
        self.depth_materials = self
            .materials
            .iter()
            .map(|handle| {
                let mut surface = surfaces
                    .get(handle)
                    .context("missing body material")?
                    .clone();
                surface.depth_write = false;
                // Retain less-or-equal depth testing when disabling depth writes.
                surface.depth_equal = true;
                Ok(surfaces.add(surface))
            })
            .collect::<Result<_>>()?;
        Ok(())
    }

    fn prepare_red_materials(&mut self, surfaces: &mut Assets<TitleSurface>) -> Result<()> {
        // Alternate outlines use constant-color draws.
        if self.spec.scene.outline_color.is_some() {
            return Ok(());
        }
        let mut prepare = |handles: &[Handle<TitleSurface>]| -> Result<Vec<_>> {
            handles
                .iter()
                .map(|handle| {
                    let mut surface = surfaces
                        .get(handle)
                        .context("missing battle material for red-channel preparation")?
                        .clone();
                    surface.red_channel = true;
                    Ok(surfaces.add(surface))
                })
                .collect()
        };
        self.red_materials = prepare(&self.materials)?;
        self.red_depth_materials = prepare(&self.depth_materials)?;
        Ok(())
    }

    fn warm_materials(&self) -> impl Iterator<Item = &Vec<Handle<TitleSurface>>> {
        self.variants.values().chain(
            [
                &self.depth_materials,
                &self.red_materials,
                &self.red_depth_materials,
            ]
            .into_iter()
            .filter(|handles| !handles.is_empty()),
        )
    }

    fn appearance_materials(
        &self,
        style: Option<ModelStyle>,
        material: ModelMaterial,
        depth_write: bool,
    ) -> Result<&[Handle<TitleSurface>]> {
        if material == ModelMaterial::RedChannel && self.spec.scene.outline_color.is_none() {
            ensure!(
                style.is_none(),
                "particle model requested actor-only red-channel material"
            );
            let handles = if depth_write {
                &self.red_materials
            } else {
                &self.red_depth_materials
            };
            ensure!(
                handles.len() == self.materials.len(),
                "red-channel battle material was not prepared"
            );
            Ok(handles)
        } else {
            self.materials(style, depth_write)
        }
    }

    fn materials(
        &self,
        style: Option<ModelStyle>,
        depth_write: bool,
    ) -> Result<&[Handle<TitleSurface>]> {
        if let Some(style) = style {
            Ok(self
                .variants
                .get(&style)
                .context("unprepared model particle material")?)
        } else if depth_write {
            Ok(&self.materials)
        } else {
            ensure!(
                self.depth_materials.len() == self.materials.len(),
                "unprepared body depth material"
            );
            Ok(&self.depth_materials)
        }
    }

    fn expression(
        &self,
        channels: &[resonance_content::battle_profile::TextureChannel],
        layers: [u8; 4],
        appearance: ModelMaterial,
        depth_write: bool,
        surfaces: &mut Assets<TitleSurface>,
    ) -> Result<()> {
        for (index, handle) in self
            .appearance_materials(None, appearance, depth_write)?
            .iter()
            .enumerate()
        {
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
            },
            gltf: Handle::default(),
            textures: vec![],
            root: None,
            nodes: vec![],
            draws: vec![],
            suppressed_draws: vec![],
            materials: vec![handle.clone()],
            variants: BTreeMap::new(),
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
            None,
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
            None,
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
        part.spec.scene.outline_color = None;
        surfaces.get_mut(&handle).unwrap().constant_color = false;
        part.tint(
            body,
            Some(color),
            ModelMaterial::Normal,
            false,
            [123., 456., 789.],
            None,
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
    }
    #[test]
    fn body_depth_materials_are_prepared_and_keep_both_passes_appearance() -> Result<()> {
        let mut surfaces = Assets::<TitleSurface>::default();
        let mut images = Assets::<Image>::default();
        let color = images.add(Image::default());
        let sampling = images.add(Image::default());
        let multiply = images.add(Image::default());
        let primary = surfaces.add(TitleSurface {
            color: Some(color),
            sampling: Some(sampling),
            multiply: Some(multiply),
            depth_write: true,
            blend: Some(Blend::Alpha),
            depth_equal: true,
            toon_ramp: Some(Handle::default()),
            tint: Vec4::new(0.2, 0.3, 0.4, 0.5),
            uv_offsets: Vec4::new(1., 2., 3., 4.),
            ..Default::default()
        });
        let alternate = surfaces.add(TitleSurface {
            depth_write: true,
            constant_color: true,
            blend: Some(Blend::Alpha),
            cull: resonance_content::CullFace::Front,
            ..Default::default()
        });
        let part = |handle: Handle<TitleSurface>, alternate: bool| -> Result<Part> {
            let binding = |texture| {
                serde_json::json!({
                    "texture": texture, "wrap_u": "repeat", "wrap_v": "repeat",
                    "nearest_min": false, "nearest_mag": false
                })
            };
            Ok(Part {
                spec: PreviewPart {
                    scene: serde_json::from_value(serde_json::json!({
                        "resource": 0, "mesh": "unused.glb", "textures": ["color.ktx2", "multiply.ktx2"],
                        "materials": [{
                            "color": (!alternate).then(|| binding(0)),
                            "multiply": (!alternate).then(|| binding(1)),
                            "vertex_color": !alternate, "blend": true, "depth_write": true,
                            "draw_order": 0, "cull": if alternate { "front" } else { "back" }
                        }],
                        "translation": [0, 0, 0], "clips": [], "autoplay": false,
                        "texture_animations": [],
                        "outline_color": alternate.then_some([0, 0, 0, 127])
                    }))?,
                    animation: None,
                    attached_to: None,
                    additive: false,
                    uv_offsets: vec![[0.125, 0.25, 0.375, 0.5]],
                },
                gltf: Handle::default(),
                textures: vec![],
                root: None,
                nodes: vec![],
                draws: vec![],
                suppressed_draws: vec![],
                materials: vec![handle],
                variants: BTreeMap::new(),
                depth_materials: Vec::new(),
                red_materials: Vec::new(),
                red_depth_materials: Vec::new(),
                warm: vec![],

                bound: true,
                disabled: false,
            })
        };
        // Separate parts match the primary and constant-color alternate banks.
        let mut primary_part = part(primary.clone(), false)?;
        let mut alternate_part = part(alternate.clone(), true)?;
        assert!(primary_part.materials(None, false).is_err());
        assert!(alternate_part.materials(None, false).is_err());
        primary_part.prepare_body_depth(&mut surfaces)?;
        alternate_part.prepare_body_depth(&mut surfaces)?;
        let count = surfaces.len();
        assert_eq!(count, 4);
        let primary_depth = primary_part.materials(None, false)?[0].clone();
        let alternate_depth = alternate_part.materials(None, false)?[0].clone();
        for (ordinary, faded) in [(&primary, &primary_depth), (&alternate, &alternate_depth)] {
            let a = surfaces.get(ordinary).unwrap();
            let b = surfaces.get(faded).unwrap();
            assert!(a.depth_write);
            assert!(!b.depth_write);
            assert_eq!(a.tint, b.tint);
            assert_eq!(a.uv_offsets, b.uv_offsets);
            assert_eq!(a.toon_ramp, b.toon_ramp);
            assert_eq!(a.blend, b.blend);
            assert_eq!(a.constant_color, b.constant_color);
            assert_eq!(a.cull, b.cull);
            assert!(b.depth_equal);
        }
        let channels = [
            resonance_content::battle_profile::TextureChannel {
                texture: 0,
                frames: 4,
            },
            resonance_content::battle_profile::TextureChannel {
                texture: 1,
                frames: 8,
            },
        ];
        for part in [&primary_part, &alternate_part] {
            part.tint(
                [192, 128, 128, 248],
                Some([0, 0, 0, 124]),
                ModelMaterial::Normal,
                true,
                [150., 300., 200.],
                None,
                false,
                &mut surfaces,
            )?;
            part.expression(
                &channels,
                [2, 3, 0, 0],
                ModelMaterial::Normal,
                false,
                &mut surfaces,
            )?;
        }
        let body = surfaces.get(&primary_depth).unwrap();
        assert_eq!(body.ambient_scale, Vec3::new(3., 2., 2.));
        assert_eq!(body.tint, Vec4::new(1., 1., 1., 248. / 255.));
        assert_eq!(body.field_light, Vec4::new(150., 300., 200., 192.));
        assert_eq!(body.uv_offsets, Vec4::new(0.125, 0.75, 0.375, 0.875));
        assert!(!body.constant_color);
        let outline = surfaces.get(&alternate_depth).unwrap();
        assert_eq!(outline.ambient_scale, Vec3::ONE);
        assert_eq!(outline.tint, Vec4::new(0., 0., 0., 124. / 255.));
        assert_eq!(outline.field_light, Vec4::ZERO);
        assert_eq!(outline.uv_offsets, Vec4::new(0.125, 0.25, 0.375, 0.5));
        assert!(outline.constant_color);
        // The unused ordinary variant keeps its appearance until selected.
        assert_eq!(
            surfaces.get(&primary).unwrap().tint,
            Vec4::new(0.2, 0.3, 0.4, 0.5)
        );
        assert_eq!(
            surfaces.get(&primary).unwrap().uv_offsets,
            Vec4::new(1., 2., 3., 4.)
        );
        primary_part.expression(
            &channels,
            [1, 2, 0, 0],
            ModelMaterial::Normal,
            true,
            &mut surfaces,
        )?;
        assert_eq!(
            surfaces.get(&primary).unwrap().uv_offsets,
            Vec4::new(0.125, 0.5, 0.375, 0.75)
        );
        assert_eq!(
            surfaces.get(&primary_depth).unwrap().uv_offsets,
            Vec4::new(0.125, 0.75, 0.375, 0.875)
        );
        assert_eq!(primary_part.materials(None, true)?, [primary]);
        assert_eq!(alternate_part.materials(None, true)?, [alternate]);
        assert_eq!(surfaces.len(), count); // Runtime appearance/selection allocates nothing.

        // Both depth recipes must be warmed before either sampled material is used.
        assert!(
            primary_part
                .appearance_materials(None, ModelMaterial::RedChannel, true)
                .is_err()
        );
        primary_part.prepare_red_materials(&mut surfaces)?;
        alternate_part.prepare_red_materials(&mut surfaces)?;
        assert_eq!(surfaces.len(), count + 2); // No red constant-outline recipe.
        assert!(alternate_part.red_materials.is_empty());
        assert!(alternate_part.red_depth_materials.is_empty());
        let red =
            primary_part.appearance_materials(None, ModelMaterial::RedChannel, true)?[0].clone();
        let red_depth =
            primary_part.appearance_materials(None, ModelMaterial::RedChannel, false)?[0].clone();
        let warmed: Vec<_> = primary_part.warm_materials().flatten().cloned().collect();
        assert_eq!(
            warmed,
            [primary_depth.clone(), red.clone(), red_depth.clone()]
        );
        for (normal, stone) in [
            (&primary_part.materials[0], &red),
            (&primary_depth, &red_depth),
        ] {
            let normal = surfaces.get(normal).unwrap();
            let stone = surfaces.get(stone).unwrap();
            assert!(!normal.red_channel);
            assert!(stone.red_channel);
            assert_eq!(normal.color, stone.color);
            assert_eq!(normal.sampling, stone.sampling);
            assert_eq!(normal.multiply, stone.multiply);
            assert_eq!(normal.toon_ramp, stone.toon_ramp);
            assert_eq!(normal.depth_write, stone.depth_write);
            assert_eq!(normal.depth_equal, stone.depth_equal);
            assert_eq!(normal.blend, stone.blend);
            assert_eq!(normal.cull, stone.cull);
        }
        let prepared_count = surfaces.len();
        // Repeated held samples, a depth change and cure select existing handles.
        // The material operand is sampled with the body; no live condition read.
        for (material, depth, alpha) in [
            (ModelMaterial::RedChannel, true, 77),
            (ModelMaterial::RedChannel, true, 77),
            (ModelMaterial::RedChannel, false, 64),
            (ModelMaterial::Normal, false, 248),
            (ModelMaterial::Normal, true, 255),
        ] {
            let rgb = if material == ModelMaterial::RedChannel {
                64
            } else {
                192
            };
            primary_part.tint(
                [rgb, rgb, rgb, alpha],
                None,
                material,
                true,
                [150., 300., 200.],
                None,
                depth,
                &mut surfaces,
            )?;
            primary_part.expression(&channels, [2, 3, 0, 0], material, depth, &mut surfaces)?;
            let handle = &primary_part.appearance_materials(None, material, depth)?[0];
            let surface = surfaces.get(handle).unwrap();
            assert_eq!(surface.red_channel, material == ModelMaterial::RedChannel);
            assert_eq!(surface.ambient_scale, Vec3::splat(f32::from(rgb) / 64.));
            assert_eq!(surface.tint, Vec4::new(1., 1., 1., f32::from(alpha) / 255.));
            assert_eq!(surface.uv_offsets, Vec4::new(0.125, 0.75, 0.375, 0.875));
            assert_eq!(surfaces.len(), prepared_count);
        }
        alternate_part.tint(
            [64, 64, 64, 77],
            Some([0, 0, 0, 38]),
            ModelMaterial::RedChannel,
            true,
            [150., 300., 200.],
            None,
            false,
            &mut surfaces,
        )?;
        let outline = surfaces.get(&alternate_depth).unwrap();
        assert!(!outline.red_channel);
        assert_eq!(outline.tint, Vec4::new(0., 0., 0., 38. / 255.));

        // Carried parts have a single depth recipe, preserving weapon blend flags.
        let weapon_handle = surfaces.add(TitleSurface {
            blend: Some(Blend::Additive),
            depth_write: false,
            depth_equal: true,
            cull: resonance_content::CullFace::None,
            ..Default::default()
        });
        let mut weapon = part(weapon_handle.clone(), false)?;
        weapon.prepare_red_materials(&mut surfaces)?;
        assert_eq!(weapon.warm_materials().count(), 1);
        let stone_weapon =
            weapon.appearance_materials(None, ModelMaterial::RedChannel, true)?[0].clone();
        weapon.tint(
            [64, 64, 64, 101],
            None,
            ModelMaterial::RedChannel,
            false,
            [150., 300., 200.],
            None,
            true,
            &mut surfaces,
        )?;
        let surface = surfaces.get(&stone_weapon).unwrap();
        assert!(
            surface.red_channel && surface.blend == Some(Blend::Additive) && surface.depth_equal
        );
        assert!(!surface.depth_write);
        assert_eq!(surface.cull, resonance_content::CullFace::None);
        assert_eq!(surface.tint, Vec4::new(1., 1., 1., 101. / 255.));
        assert!(!surfaces.get(&weapon_handle).unwrap().red_channel);
        Ok(())
    }
}
