//! Temporal render diagnostics using real field updates and cooked assets.
use super::{ActorPart, Art, Session};
use anyhow::{Result, ensure};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

/// Five minutes at 60 Hz includes the seal scenes and their post-battle dialogue.
const MAX_SEQUENCE_UPDATES: u32 = 18_000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSequence {
    #[serde(default)]
    pub resolution: crate::Resolution,
    /// Selected zero-based render frames; empty records every frame.
    #[serde(default)]
    pub capture_frames: Vec<u32>,
    /// Compact field restart for paired oracle cases; no transient VM state.
    #[serde(default)]
    pub checkpoint: Option<resonance_game::field::FieldCheckpoint>,
    /// Scenario bytecode for a controlled command-driven effect comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Vec<u16>>,
    /// Register a random input after fixture setup, before effect commands run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_seed: Option<EffectSeed>,
    /// Use copied progress for an arrival scene instead of loading a free-control save.
    #[serde(default)]
    pub scene_entry: bool,
    /// Controlled render fixture: isolate effects or selected actors on a matte.
    #[serde(default)]
    pub isolation: Option<FieldIsolation>,
    /// Ordered formation IDs granted victory for controlled post-battle captures.
    #[serde(default)]
    pub battle_victories: Vec<u16>,
    pub start_tick: Option<u32>,
    pub probe: Option<crate::ClassroomProbe>,
    pub updates: u32,
    pub renders_per_update: u32,
    #[serde(default)]
    pub direction: [f32; 2],
    #[serde(default)]
    pub run: bool,
    #[serde(default)]
    pub accept_updates: Vec<u32>,
    #[serde(default)]
    pub ring_updates: Vec<u32>,
    /// Held input changes, indexed from the first recorded update.
    #[serde(default)]
    pub movement: Vec<FieldMovement>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSeed {
    pub update: u32,
    pub state: u32,
    #[serde(default)]
    pub effect_tick: Option<u32>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldIsolation {
    pub background: [u8; 3],
    pub visible_actors: Vec<i32>,
    #[serde(default)]
    pub camera: Option<IsolationCamera>,
    #[serde(default)]
    pub remove_actors: Vec<i32>,
    #[serde(default)]
    pub clear_effects: bool,
    /// Stop arrival scripts so an isolated ability can receive ordinary input.
    #[serde(default)]
    pub cancel_scripts: bool,
    /// Frozen renderer inputs for the effect-base delta suite, reapplied after each update.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<super::effect_probe::EffectProbe>,
    /// Stationary scenery for refraction tests, alongside live effects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backdrop: Vec<super::effect_probe::EffectProbe>,
    /// Replace frozen inputs at these update numbers without reloading the field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub samples: BTreeMap<u32, EffectSample>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSample {
    pub background: [u8; 3],
    pub effects: Vec<super::effect_probe::EffectProbe>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationCamera {
    pub position: [f32; 3],
    pub target: [f32; 3],
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldMovement {
    pub update: u32,
    pub direction: [f32; 2],
    #[serde(default)]
    pub run: bool,
}
impl FieldSequence {
    pub(super) fn validate(&self) -> Result<()> {
        if let Some(seed) = &self.random_seed {
            ensure!(
                self.script.is_some() && seed.update < self.updates,
                "effect random seed requires a script and an update inside the sequence"
            );
        }
        if let Some(script) = &self.script {
            ensure!(
                self.checkpoint.is_some() && self.start_tick == Some(0),
                "effect script requires a checkpoint and starts at update zero"
            );
            symphonia_script::Program::decode(
                &script
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<_>>(),
            )?;
        }
        if let Some(isolation) = &self.isolation {
            ensure!(
                isolation.effects.len() + isolation.backdrop.len() <= 128,
                "too many effect probes"
            );
            for (&update, sample) in &isolation.samples {
                ensure!(
                    update < self.updates && sample.effects.len() <= 128,
                    "invalid effect sample"
                );
            }
            for effect in isolation
                .effects
                .iter()
                .chain(&isolation.backdrop)
                .chain(isolation.samples.values().flat_map(|s| &s.effects))
            {
                effect.validate()?;
            }
        }
        if let Some(camera) = self.isolation.as_ref().and_then(|i| i.camera.as_ref()) {
            ensure!(
                camera
                    .position
                    .iter()
                    .chain(&camera.target)
                    .all(|v| v.is_finite())
                    && camera.position != camera.target,
                "invalid isolation camera"
            );
        }
        ensure!(
            !self.scene_entry || (self.checkpoint.is_some() && self.start_tick.is_some()),
            "scene entry requires copied progress and an explicit start tick"
        );
        ensure!(
            (1..=MAX_SEQUENCE_UPDATES).contains(&self.updates)
                && (1..=8).contains(&self.renders_per_update),
            "invalid sequence length/cadence"
        );
        ensure!(
            self.capture_frames.windows(2).all(|w| w[0] < w[1])
                && self
                    .capture_frames
                    .iter()
                    .all(|f| *f < self.updates * self.renders_per_update),
            "invalid capture frames"
        );
        ensure!(
            self.direction
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.),
            "invalid sequence direction"
        );
        ensure!(
            self.accept_updates
                .iter()
                .chain(&self.ring_updates)
                .all(|u| *u < self.updates),
            "input outside sequence"
        );
        ensure!(
            self.movement.windows(2).all(|w| w[0].update < w[1].update)
                && self.movement.iter().all(|m| m.update < self.updates
                    && m.direction.iter().all(|v| v.is_finite() && v.abs() <= 1.)),
            "invalid movement sequence"
        );
        Ok(())
    }
}
#[derive(Resource)]
pub(super) struct Recording {
    spec: FieldSequence,
    output: PathBuf,
    settled: u32,
    frame: u32,
    captured: u32,
    presenting: bool,
    battle_victories: usize,
    since: Instant,
}
pub(super) fn install(app: &mut App, output: &Path, spec: &FieldSequence) -> Result<()> {
    ensure!(!output.exists(), "sequence output already exists");
    fs::create_dir_all(output)?;
    fs::write(
        output.join("sequence.json"),
        serde_json::to_vec_pretty(spec)?,
    )?;
    app.insert_resource(Recording {
        spec: spec.clone(),
        output: output.into(),
        settled: 0,
        frame: 0,
        captured: 0,
        presenting: false,
        battle_victories: 0,
        since: Instant::now(),
    })
    .add_systems(PreUpdate, advance)
    .add_systems(
        PostUpdate,
        capture
            .after(bevy::transform::TransformSystems::Propagate)
            .after(bevy::asset::AssetEventSystems)
            .after(crate::field_audit::check),
    );
    Ok(())
}
fn advance(
    mut recording: ResMut<Recording>,
    mut session: ResMut<Session>,
    mut exit: MessageWriter<AppExit>,
) {
    let frame_count = recording.spec.updates * recording.spec.renders_per_update;
    let capture_count = if recording.spec.capture_frames.is_empty() {
        frame_count
    } else {
        recording.spec.capture_frames.len() as u32
    };
    if recording.frame > frame_count && recording.captured == capture_count {
        exit.write(AppExit::Success);
        return;
    }
    if recording.since.elapsed().as_secs() > 60 + u64::from(frame_count) / 20 {
        error!(
            frame = recording.frame,
            tick = session.0.events.tick(),
            "field sequence timed out"
        );
        exit.write(AppExit::error());
        return;
    }
    if recording.presenting
        || recording.settled < 20
        || recording.frame >= recording.spec.updates * recording.spec.renders_per_update
    {
        return;
    }
    if recording
        .frame
        .is_multiple_of(recording.spec.renders_per_update)
    {
        let update = recording.frame / recording.spec.renders_per_update;
        if let Some(isolation) = &recording.spec.isolation {
            if update == 0 && isolation.cancel_scripts {
                session.0.events.cancel();
                session.0.dialogue.clear();
                let world = &mut session.0.events.world;
                world.input_enabled = true;
                world.triggers.clear();
                world.fade = Some(resonance_events::Fade {
                    start_tick: world.tick,
                    duration: 0,
                    from: 0.,
                    to: 0.,
                    white: false,
                });
            }
            let world = &mut session.0.events.world;
            if update == 0 {
                for id in &isolation.remove_actors {
                    world.actors.remove(id);
                }
                if isolation.clear_effects {
                    world.billboards.clear();
                    world.model_particles.clear();
                    world.refractions.clear();
                }
            }
            if recording.spec.script.is_none() || update == 0 {
                for (&id, actor) in &mut world.actors {
                    actor.appearance.model_hidden = !isolation.visible_actors.contains(&id);
                    actor.casts_shadow = false;
                    if recording.spec.script.is_some() {
                        // A borrowed effect stage need not have floor under its actors.
                        actor.grounded = false;
                        if id == world.controlled_actor {
                            if let Some(ai) = &mut actor.autonomy {
                                ai.activity = resonance_events::Activity::Idle;
                                ai.remaining = i32::MAX;
                                ai.initialized = true;
                            }
                        } else {
                            actor.autonomy = None;
                        }
                    }
                }
            }
        }
        if let Some(seed) = &recording.spec.random_seed
            && seed.update == update
        {
            session.0.events.world.random_state = seed.state;
            if let Some(tick) = seed.effect_tick {
                session.0.effect_clock = resonance_game::clock::PresentationClock::new(tick);
            }
        }
        let movement = recording
            .spec
            .movement
            .iter()
            .rev()
            .find(|m| m.update <= update);
        if let Some(camera) = &mut session.0.events.world.field_camera {
            camera.view_aspect_ratio = recording.spec.resolution.aspect();
        }
        if let Some(camera) = recording
            .spec
            .isolation
            .as_ref()
            .and_then(|i| i.camera.as_ref())
        {
            let world = &mut session.0.events.world;
            world.camera = None;
            let rig = world.field_camera.as_mut().unwrap();
            rig.motion = None;
            rig.position = camera.position;
            rig.target = camera.target;
            rig.current_mut().position_bounds = camera.position.map(|v| [v; 2]);
            rig.current_mut().target_bounds = camera.target.map(|v| [v; 2]);
        }
        if let Err(error) = session.0.step(resonance_game::field::FieldInput {
            direction: movement.map_or(recording.spec.direction, |m| m.direction),
            run: movement.map_or(recording.spec.run, |m| m.run),
            interact: recording.spec.accept_updates.contains(&update),
            alternate: recording.spec.ring_updates.contains(&update),
            ..Default::default()
        }) {
            error!("field sequence update failed: {error:#}");
            exit.write(AppExit::error());
            return;
        }
        session.0.events.world.audio_commands.clear();
        if let Some(isolation) = &recording.spec.isolation
            && (!isolation.effects.is_empty() || !isolation.samples.is_empty())
        {
            let world = &mut session.0.events.world;
            world.billboards.clear();
            world.model_particles.clear();
            world.refractions.clear();
            let effects = isolation
                .samples
                .range(..=update)
                .next_back()
                .map_or(&isolation.effects, |(_, sample)| &sample.effects);
            for (index, effect) in effects.iter().enumerate() {
                effect.apply(world, index as i32 + 1);
            }
        }
        if let Some(isolation) = &recording.spec.isolation {
            for (index, effect) in isolation.backdrop.iter().enumerate() {
                let world = &mut session.0.events.world;
                let id = -(index as i32 + 1);
                effect.apply(world, id);
                if let Some(sprite) = world.billboards.get_mut(&id) {
                    sprite.field_fog = true;
                }
            }
        }
        if !recording.spec.battle_victories.is_empty() {
            let result = (|| -> Result<()> {
                let world = &mut session.0.events.world;
                if let Some(request) = &world.battle_request {
                    let expected = recording
                        .spec
                        .battle_victories
                        .get(recording.battle_victories);
                    ensure!(
                        expected.is_some_and(|id| request.setup.encounter
                            == resonance_events::battle::Encounter::Formation(*id)),
                        "unexpected battle in field capture: {:?}",
                        request.setup
                    );
                    world.skip_battle_as_victory().map_err(anyhow::Error::msg)?;
                    recording.battle_victories += 1;
                }
                ensure!(
                    update + 1 != recording.spec.updates
                        || recording.battle_victories == recording.spec.battle_victories.len(),
                    "unused battle victory grants"
                );
                Ok(())
            })();
            if let Err(error) = result {
                error!("field sequence battle grant failed: {error:#}");
                exit.write(AppExit::error());
                return;
            }
        }
    }
    recording.frame += 1;
    recording.presenting = true;
}
#[allow(clippy::too_many_arguments)] // Readiness, current poses and GPU readback.
fn capture(
    mut commands: Commands,
    mut recording: ResMut<Recording>,
    session: Res<Session>,
    art: Res<Art>,
    ready: Res<crate::RenderReady>,
    refraction: Res<crate::field_refraction::Ready>,
    applied: Res<crate::field_audit::Applied>,
    target: Res<crate::Framebuffer>,
    roots: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    bones: Query<(&Name, &Transform, &GlobalTransform)>,
    ui: Res<crate::field_ui::Artwork>,
    effects: Query<(&Mesh3d, &Visibility), With<crate::field_effects::EffectDraw>>,
    meshes: Res<Assets<Mesh>>,
    mut clear: ResMut<ClearColor>,
) {
    if let Some(isolation) = &recording.spec.isolation {
        let update = recording.frame.saturating_sub(1) / recording.spec.renders_per_update;
        let background = isolation
            .samples
            .range(..=update)
            .next_back()
            .map_or(isolation.background, |(_, sample)| sample.background);
        let [r, g, b] = background.map(|v| f32::from(v) / 255.);
        clear.0 = Color::linear_rgb(r, g, b);
    }
    if recording.settled < 20 {
        if art.ready
            && !roots.is_empty()
            && roots.iter().all(|(_, p)| p.prepared)
            && ready.0.load(std::sync::atomic::Ordering::Relaxed)
            && refraction.get()
        {
            recording.settled += 1;
        } else {
            recording.settled = 0;
        }
        return;
    }
    if recording.frame == 0
        || recording.frame > recording.spec.updates * recording.spec.renders_per_update
    {
        return;
    }
    // A spawned model may need several render updates to instantiate. Keep the
    // simulation on its birth update until the model can be captured. Dialogue
    // initialization needs another game update, so it must not block this gate.
    if !recording.presenting
        || !applied
            .model_particles_ready(session.0.events.tick())
            .expect("field sequence model effects did not become ready")
        || roots.iter().any(|(_, p)| !p.prepared)
        || session.0.events.world.actors.iter().any(|(id, actor)| {
            art.models.contains_key(&actor.resource) && !art.instances.contains_key(id)
        })
    {
        return;
    }
    recording.presenting = false;
    let frame = recording.frame - 1;
    if recording.frame == recording.spec.updates * recording.spec.renders_per_update {
        recording.frame += 1;
    }
    if !recording.spec.capture_frames.is_empty()
        && recording.spec.capture_frames.binary_search(&frame).is_err()
    {
        return;
    }
    let path = recording.output.join(format!("frame-{frame:04}.png"));
    let world = &session.0.events.world;
    let state = serde_json::json!({
        "frame":frame, "tick":world.tick, "effect_tick":world.effect_tick,
        "random_state":world.random_state, "audio_device":false,
        "resolution":recording.spec.resolution,
        "output_stage":"framebuffer",
        "input_enabled":world.input_enabled,
        "battle_victories":recording.battle_victories,
        "camera":world.field_camera.as_ref().map(|c| serde_json::json!({"position":c.position,"target":c.target,"fov":c.fov_degrees()})),
        "actors":world.actors.iter().map(|(id,a)|serde_json::json!({"id":id,"resource":a.resource,"hidden_nodes":a.appearance.hidden_nodes,"position":a.position,"visual_position":a.visual_position(),"heading":a.heading,"animation":a.animation.as_ref().map(|a|serde_json::json!({"slot":a.slot,"start_tick":a.start_tick,"sample":a.sample(world.tick,0,a.duration_ticks as f32)}))})).collect::<Vec<_>>(),
        "model_particles":world.model_particles.iter().map(|(id,p)|serde_json::json!({"id":id,"resource":p.resource,"position":p.position,"rotation":p.rotation,"scale":p.scale,"rgba":p.rgba})).collect::<Vec<_>>(),
        "billboards":world.billboards.iter().map(|(id,p)|serde_json::json!({"id":id,"recipe":p.recipe,"born":p.born,"lifetime":p.lifetime,"position":p.position,"rotation":p.rotation,"size":p.size,"rgba":p.rgba,"alpha":p.alpha(world.tick),"velocity":p.velocity,"size_delta":p.size_delta,"blend":p.blend})).collect::<Vec<_>>(),
        "poses":roots.iter().filter(|(_,p)|p.actor==world.controlled_actor && p.part==0).flat_map(|(root,_)|children.iter_descendants(root)).filter_map(|e|bones.get(e).ok()).map(|(name,t,g)|serde_json::json!({"name":name.as_str(),"translation":t.translation.to_array(),"rotation":t.rotation.to_array(),"world":g.to_matrix().to_cols_array()})).collect::<Vec<_>>(),
        "emotes":format!("{:?}",world.emotes),
        "refractions":world.refractions.iter().map(|(id,p)| {
            let (size, alpha) = (p.size, p.alpha(world.tick));
            serde_json::json!({"id":id,"born":p.born,"position":p.position,"size":size,"alpha":alpha})
        }).collect::<Vec<_>>(),
        "save_points":world.save_points.iter().map(|p|serde_json::json!({"position":p.position,"active":p.active,"glow_scale":p.glow_scale})).collect::<Vec<_>>(),
        "effects":effects.iter().filter(|(_,v)| **v != Visibility::Hidden).map(|(mesh,visibility)|serde_json::json!({"visibility":format!("{visibility:?}"),"positions":format!("{:?}",meshes.get(mesh).and_then(|m|m.attribute(Mesh::ATTRIBUTE_POSITION)))})).collect::<Vec<_>>(),
        "dialogue_layouts":ui.diagnostic_layouts(&session.0),
    });
    commands.spawn(Screenshot(target.0.clone())).observe(
        move |event: On<ScreenshotCaptured>,
              mut recording: ResMut<Recording>,
              mut exit: MessageWriter<AppExit>| {
            let result = (|| -> Result<()> {
                crate::screenshot::write(&event.image, &path, None)?;
                fs::write(path.with_extension("json"), serde_json::to_vec(&state)?)?;
                Ok(())
            })();
            if let Err(error) = result {
                error!("sequence capture failed: {error:#}");
                exit.write(AppExit::error());
                return;
            }
            recording.captured += 1;
        },
    );
}
