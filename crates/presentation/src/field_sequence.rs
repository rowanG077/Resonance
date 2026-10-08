//! Temporal render diagnostics using real field updates and cooked assets.
use super::{ActorPart, Art, Session};
use anyhow::{Result, ensure};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
    /// Record per-frame simulation and geometry diagnostics alongside images.
    #[serde(default)]
    pub trace: bool,
    /// The simulation's starting state; no transient VM state is restored.
    #[serde(default)]
    pub scene: FieldScene,
    #[serde(default)]
    pub at: CaptureMoment,
    /// Register a random input after fixture setup, before effect commands run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_seed: Option<EffectSeed>,
    /// Controlled render fixture: isolate effects or selected actors on a matte.
    #[serde(default)]
    pub isolation: Option<FieldIsolation>,
    /// Ordered formation IDs granted victory for controlled post-battle captures.
    #[serde(default)]
    pub battle_victories: Vec<u16>,
    pub probe: Option<crate::ClassroomProbe>,
    pub updates: u32,
    pub renders_per_update: u32,
    /// Complete held controls at each change; button edges follow transitions.
    #[serde(default)]
    pub inputs: Vec<FieldControls>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldScene {
    #[default]
    Classroom,
    NewGame,
    Restore {
        checkpoint: resonance_game::field::FieldCheckpoint,
    },
    Arrival {
        checkpoint: resonance_game::field::FieldCheckpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        script: Option<Vec<u16>>,
    },
}
impl FieldScene {
    pub(super) fn checkpoint(&self) -> Option<&resonance_game::field::FieldCheckpoint> {
        match self {
            Self::Restore { checkpoint } | Self::Arrival { checkpoint, .. } => Some(checkpoint),
            _ => None,
        }
    }
    pub(super) fn script(&self) -> Option<&[u16]> {
        match self {
            Self::Arrival { script, .. } => script.as_deref(),
            _ => None,
        }
    }
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureMoment {
    #[default]
    Control,
    Tick {
        update: u32,
    },
    Dialogue {
        prefix: String,
        hold_updates: u32,
    },
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
    /// Stationary scenery for refraction tests, alongside live effects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backdrop: Vec<super::backdrop::Backdrop>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationCamera {
    pub position: [f32; 3],
    pub target: [f32; 3],
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldControls {
    pub update: u32,
    #[serde(default)]
    pub direction: [f32; 2],
    #[serde(default)]
    pub run: bool,
    #[serde(default)]
    pub buttons: Vec<resonance_events::input::Button>,
}
impl FieldSequence {
    pub(super) fn validate(&self) -> Result<()> {
        if let Some(seed) = &self.random_seed {
            ensure!(
                self.scene.script().is_some() && seed.update < self.updates,
                "effect random seed requires a script and an update inside the sequence"
            );
        }
        if let Some(script) = self.scene.script() {
            ensure!(
                matches!(self.at, CaptureMoment::Tick { update: 0 }),
                "effect script starts at update zero"
            );
            symphonia_script::Program::decode(
                &script
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<_>>(),
            )?;
        }
        if let Some(isolation) = &self.isolation {
            ensure!(isolation.backdrop.len() <= 128, "too many backdrop tiles");
            for tile in &isolation.backdrop {
                tile.validate()?;
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
            self.inputs.windows(2).all(|w| w[0].update < w[1].update)
                && self.inputs.iter().all(|m| m.update < self.updates
                    && m.direction.iter().all(|v| v.is_finite() && v.abs() <= 1.)),
            "invalid input timeline"
        );
        Ok(())
    }

    fn input(&self, update: u32) -> resonance_game::field::FieldInput {
        let count = self.inputs.partition_point(|input| input.update <= update);
        let Some(current) = count.checked_sub(1).map(|i| &self.inputs[i]) else {
            return Default::default();
        };
        resonance_game::field::FieldInput {
            direction: current.direction,
            run: current.run,
            held_buttons: current.buttons.iter().copied().collect(),
            ..Default::default()
        }
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
    rendered: bool,
    battle_victories: usize,
    since: Instant,
    failure: Failure,
}

#[derive(Clone, Default)]
pub(super) struct Failure(Arc<Mutex<Option<anyhow::Error>>>);
impl Failure {
    fn record(&self, error: anyhow::Error, exit: &mut MessageWriter<AppExit>) {
        error!("field capture failed: {error:#}");
        self.0.lock().unwrap().get_or_insert(error);
        exit.write(AppExit::error());
    }
    pub(super) fn result(&self) -> Result<()> {
        self.0.lock().unwrap().take().map_or(Ok(()), Err)
    }
}

pub(super) fn install(app: &mut App, output: &Path, spec: &FieldSequence) -> Result<Failure> {
    ensure!(!output.exists(), "sequence output already exists");
    fs::create_dir_all(output)?;
    fs::write(
        output.join("sequence.json"),
        serde_json::to_vec_pretty(spec)?,
    )?;
    let failure = Failure::default();
    app.insert_resource(Recording {
        spec: spec.clone(),
        output: output.into(),
        settled: 0,
        frame: 0,
        captured: 0,
        presenting: false,
        rendered: false,
        battle_victories: 0,
        since: Instant::now(),
        failure: failure.clone(),
    })
    .add_systems(PreUpdate, advance)
    .add_systems(
        PostUpdate,
        capture
            .after(bevy::transform::TransformSystems::Propagate)
            .after(bevy::asset::AssetEventSystems)
            .after(crate::field_audit::check),
    );
    Ok(failure)
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
        recording.failure.record(
            anyhow::anyhow!(
                "field sequence timed out at frame {}, update {}",
                recording.frame,
                session.0.events.tick()
            ),
            &mut exit,
        );
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
            if recording.spec.scene.script().is_none() || update == 0 {
                for (&id, actor) in &mut world.actors {
                    actor.appearance.model_hidden = !isolation.visible_actors.contains(&id);
                    actor.casts_shadow = false;
                    if recording.spec.scene.script().is_some() {
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
        if let Err(error) = session.0.step(recording.spec.input(update)) {
            recording
                .failure
                .record(error.context("field sequence update failed"), &mut exit);
            return;
        }
        session.0.events.world.audio_commands.clear();
        if let Some(isolation) = &recording.spec.isolation {
            for (index, effect) in isolation.backdrop.iter().enumerate() {
                let world = &mut session.0.events.world;
                effect.apply(world, index);
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
                recording.failure.record(
                    error.context("field sequence battle grant failed"),
                    &mut exit,
                );
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
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(isolation) = &recording.spec.isolation {
        let [r, g, b] = isolation.background.map(|v| f32::from(v) / 255.);
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
    // Short-lived models must be ready in their birth update, as in live play.
    // Waiting here would hide flicker caused by delayed scene instantiation.
    let models_ready = match applied.model_particles_ready(session.0.events.tick()) {
        Ok(ready) => ready,
        Err(error) => {
            recording.failure.record(error, &mut exit);
            return;
        }
    };
    if !models_ready {
        recording.failure.record(
            anyhow::anyhow!("model particle missed its birth update"),
            &mut exit,
        );
        return;
    }
    if !recording.presenting
        || !ready.0.load(std::sync::atomic::Ordering::Relaxed)
        || !refraction.get()
        || roots.iter().any(|(_, p)| !p.prepared)
        || session.0.events.world.actors.iter().any(|(id, actor)| {
            art.models.contains_key(&actor.resource) && !art.instances.contains_key(id)
        })
    {
        recording.rendered = false;
        return;
    }
    // Render the prepared pose once before requesting readback. New material
    // pipelines are queued during rendering, after this system has run.
    if !std::mem::replace(&mut recording.rendered, true) {
        return;
    }
    recording.rendered = false;
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
    let state = recording.spec.trace.then(|| serde_json::json!({
        "frame":frame, "tick":world.tick, "effect_tick":world.effect_tick,
        "random_state":world.random_state, "audio_device":false,
        "resolution":recording.spec.resolution,
        "output_stage":"framebuffer",
        "input_enabled":world.input_enabled,
        "battle_victories":recording.battle_victories,
        "camera":world.field_camera.as_ref().map(|c| serde_json::json!({"position":c.position,"target":c.target,"fov":c.fov_degrees(),"shake":c.shake.offset})),
        "actors":world.actors.iter().map(|(id,a)|serde_json::json!({"id":id,"resource":a.resource,"hidden_nodes":a.appearance.hidden_nodes,"position":a.position,"autonomy":a.autonomy,"visual_position":a.visual_position(),"heading":a.heading,"animation":a.animation.as_ref().map(|a|serde_json::json!({"slot":a.slot,"start_tick":a.start_tick,"sample":a.sample(world.tick,0,a.duration_ticks as f32)}))})).collect::<Vec<_>>(),
        "model_particles":world.model_particles.iter().map(|(id,p)|serde_json::json!({"id":id,"resource":p.resource,"position":p.position,"rotation":p.rotation,"scale":p.scale,"rgba":p.rgba})).collect::<Vec<_>>(),
        "emotes":world.emotes.iter().map(|(id,e)|serde_json::json!({"id":id,"kind":e.kind,"start_tick":e.start_tick,"phase":e.phase})).collect::<Vec<_>>(),
        "billboards":world.billboards.iter().map(|(id,p)|serde_json::json!({"id":id,"draw_order":p.draw_order,"recipe":p.recipe,"born":p.born,"lifetime":p.lifetime,"position":p.position,"rotation":p.rotation,"size":p.size,"rgba":p.rgba,"alpha":p.alpha(world.tick),"velocity":p.velocity,"size_delta":p.size_delta,"blend":p.blend})).collect::<Vec<_>>(),
        "poses":roots.iter().filter(|(_,p)|p.actor==world.controlled_actor && p.part==0).flat_map(|(root,_)|children.iter_descendants(root)).filter_map(|e|bones.get(e).ok()).map(|(name,t,g)|serde_json::json!({"name":name.as_str(),"translation":t.translation.to_array(),"rotation":t.rotation.to_array(),"world":g.to_matrix().to_cols_array()})).collect::<Vec<_>>(),
        "refractions":world.refractions.iter().map(|(id,p)| {
            let (position, size, alpha) = (p.position, p.size, p.alpha(world.tick));
            serde_json::json!({"id":id,"born":p.born,"position":position,"size":size,"alpha":alpha})
        }).collect::<Vec<_>>(),
        "save_points":world.save_points.iter().map(|p|serde_json::json!({"position":p.position,"active":p.active,"glow_scale":p.glow_scale})).collect::<Vec<_>>(),
        "effects":effects.iter().filter(|(_,v)| **v != Visibility::Hidden).map(|(mesh,visibility)|serde_json::json!({"visibility":format!("{visibility:?}"),"positions":format!("{:?}",meshes.get(mesh).and_then(|m|m.attribute(Mesh::ATTRIBUTE_POSITION)))})).collect::<Vec<_>>(),
        "dialogue_layouts":ui.diagnostic_layouts(&session.0),
    }));
    commands.spawn(Screenshot(target.0.clone())).observe(
        move |event: On<ScreenshotCaptured>,
              mut recording: ResMut<Recording>,
              mut exit: MessageWriter<AppExit>| {
            let result = (|| -> Result<()> {
                crate::screenshot::write(&event.image, &path, None)?;
                if let Some(state) = &state {
                    fs::write(path.with_extension("json"), serde_json::to_vec(state)?)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                recording
                    .failure
                    .record(error.context("writing field sequence capture"), &mut exit);
                return;
            }
            recording.captured += 1;
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_events::input::Button;

    #[test]
    #[ignore = "requires locally cooked Dirk scenes and a graphics device"]
    fn fireplace_window_stays_visible_during_the_key_crest_conversation() -> Result<()> {
        use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
        let root = PathBuf::from(std::env::var("RESONANCE_WORLD_ASSETS")?);
        let field = crate::field_test::Scene::story(&root, 374, 501_000, |_| Ok(()))?;
        let checkpoint = resonance_game::field::FieldCheckpoint {
            map_id: 374,
            position: [1., 15., -3.],
            heading: 180.,
            camera: None,
            allow_incomplete_scripts: false,
            played_ticks: None,
            progress: field.events.save_progress()?,
        };
        let inputs: Vec<_> = (0..1401)
            .step_by(6)
            .map(|update| {
                serde_json::json!({
                    "update": update,
                    "buttons": if update % 12 == 0 { vec!["accept"] } else { vec![] }
                })
            })
            .collect();
        let sequence: FieldSequence = serde_json::from_value(serde_json::json!({
            "resolution": {"width": 1280, "height": 720},
            "scene": {"arrival": {"checkpoint": checkpoint}},
            "at": {"kind": "tick", "update": 0},
            "updates": 1401, "renders_per_update": 1,
            "capture_frames": [200, 1400], "inputs": inputs
        }))?;
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("frames");
        super::super::capture_field_sequence(&root, &output, &sequence)?;
        for frame in sequence.capture_frames {
            let image = Image::from_buffer(
                &fs::read(output.join(format!("frame-{frame:04}.png")))?,
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                true,
                ImageSampler::default(),
                default(),
            )?
            .try_into_dynamic()?
            .to_rgb8();
            // The right window pane should show greenery throughout the light animation.
            // Check its area, allowing smoke and small shading differences.
            let green = (125..155)
                .flat_map(|y| (300..330).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let [r, g, b] = image.get_pixel(x, y).0.map(u16::from);
                    g > 30 && g > r && g > b
                })
                .count();
            ensure!(
                green > 30 * 30 / 2,
                "window obscured at frame {frame}: {green} green pixels"
            );
        }
        Ok(())
    }

    #[test]
    fn held_controls_survive_movement_changes_and_press_again_after_release() {
        let sequence: FieldSequence = serde_json::from_value(serde_json::json!({
            "updates": 15, "renders_per_update": 1,
            "inputs": [
                {"update": 3, "buttons": ["accept", "ring"]},
                {"update": 7, "buttons": ["accept", "ring"], "direction": [1., 0.]},
                {"update": 9},
                {"update": 12, "buttons": ["accept", "ring"]}
            ]
        }))
        .unwrap();
        sequence.validate().unwrap();
        let mut buttons = resonance_events::input::Input::default();
        for update in 0..15 {
            let input = sequence.input(update);
            buttons.sample(input.held_buttons, input.pressed_buttons);
            assert_eq!(
                buttons.pressed.contains(Button::Accept),
                matches!(update, 3 | 12)
            );
            assert_eq!(
                buttons.pressed.contains(Button::Ring),
                matches!(update, 3 | 12)
            );
            assert_eq!(
                input
                    .held_buttons
                    .contains(resonance_events::input::Button::Ring),
                matches!(update, 3..=8 | 12..=14)
            );
            assert_eq!(
                input.direction,
                if matches!(update, 7..=8) {
                    [1., 0.]
                } else {
                    [0.; 2]
                }
            );
        }
    }
}
