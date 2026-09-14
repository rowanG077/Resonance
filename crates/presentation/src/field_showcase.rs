//! Offline event capture. Game time advances once per saved video frame.
use super::{ActorPart, Art, Session};
use crate::ray_tracing::denoise::Accumulation;
use anyhow::{Result, ensure};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use resonance_game::field::{FieldInput, FieldSession};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

pub const FPS: u32 = 60;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClassroomShowcase {
    pub samples: u32,
    /// Physical output size; quality presets retain the same 16:9 framing.
    pub resolution: [u32; 2],
    /// A video-frame budget, independent of how long rendering takes.
    pub max_frames: Option<u32>,
    /// Skip this many event frames using the same dialogue timing as capture.
    pub from_frame: u32,
    /// Optional absolute VM tick for diagnosing a later shot.
    pub start_tick: Option<u32>,
    /// Walk the real script and report its extent without starting a renderer.
    pub plan_only: bool,
    /// Use Bevy's complete camera-path integrator instead of realtime ReSTIR GI.
    pub pathtracing: bool,
    pub strong_smaa: bool,
    pub audio: bool,
}
impl Default for ClassroomShowcase {
    fn default() -> Self {
        Self {
            samples: 256,
            resolution: [1920, 1080],
            max_frames: None,
            from_frame: 0,
            start_tick: None,
            plan_only: false,
            pathtracing: true,
            strong_smaa: true,
            audio: true,
        }
    }
}
impl ClassroomShowcase {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (1..=65536).contains(&self.samples),
            "capture needs 1–65536 lighting samples"
        );
        ensure!(
            matches!(
                self.resolution,
                [640, 360] | [1280, 720] | [1920, 1080] | [3840, 2160]
            ),
            "capture resolution must be 640x360, 1280x720, 1920x1080 or 3840x2160"
        );
        ensure!(
            self.max_frames.is_none_or(|f| (1..=36000).contains(&f)),
            "invalid capture duration (maximum 600 seconds)"
        );
        ensure!(
            self.start_tick.is_none_or(|t| t <= 20000),
            "invalid diagnostic start tick"
        );
        ensure!(
            self.from_frame <= 36000,
            "invalid capture start (maximum 600 seconds)"
        );
        ensure!(
            self.from_frame == 0 || self.start_tick.is_none(),
            "cannot combine an event offset with a diagnostic start tick"
        );
        Ok(())
    }
    pub(super) fn reached_start(&self, session: &FieldSession) -> bool {
        let world = &session.events.world;
        world.field_camera.is_some()
            && world.actors.contains_key(&1)
            && world
                .movie
                .as_ref()
                .is_some_and(|movie| !movie.operation.is_pending())
            && !world.blocked_by_movie()
            && self.start_tick.is_none_or(|tick| world.tick >= tick)
    }
    fn stop_reason(&self, session: &FieldSession, saved: u32) -> Option<&'static str> {
        if session.player_has_control() && session.events.world.controlled_actor == 1 {
            Some("lloyd_control")
        } else if self.max_frames.is_some_and(|limit| saved >= limit) {
            Some("duration_limit")
        } else {
            None
        }
    }
}
#[derive(Default)]
pub(super) struct Director {
    ready_since: BTreeMap<(u64, usize), u32>,
    audio: Vec<(u64, resonance_events::AudioCommand)>,
}
/// Advance before starting Bevy, retaining reading holds that began before the
/// requested offset. This makes a middle clip match that part of a full capture.
pub(super) fn seek(
    session: &mut FieldSession,
    spec: &ClassroomShowcase,
    mut director: Director,
) -> Result<Director> {
    director.observe_audio(session);
    for frame in 0..spec.from_frame {
        ensure!(
            !(session.player_has_control() && session.events.world.controlled_actor == 1),
            "capture start {:.6}s is past Lloyd's control handoff at {:.6}s",
            f64::from(spec.from_frame) / f64::from(FPS),
            f64::from(frame) / f64::from(FPS)
        );
        director.advance(session)?;
    }
    Ok(director)
}
impl Director {
    pub(super) fn observe_audio(&mut self, session: &mut FieldSession) {
        let at = audio_frame(session.events.tick());
        self.audio.extend(
            session
                .events
                .world
                .audio_commands
                .drain(..)
                .map(|command| (at, command)),
        );
    }

    fn record_audio(&self, root: &Path, output: &Path, start: u32, frames: u32) -> Result<()> {
        crate::field_audio::record_window(
            root,
            &output.join("audio.wav"),
            audio_frame(start),
            audio_frame(frames),
            &self.audio,
            true,
            [127; 3],
        )?;
        let trace: Vec<_> = self.audio.iter().map(|(at, command)| serde_json::json!({
            "pcm_frame":at,"video_seconds":(*at as f64 - audio_frame(start) as f64) / f64::from(crate::field_audio::PCM_SPEC.sample_rate),
            "command":format!("{command:?}")
        })).collect();
        fs::write(
            output.join("audio-events.json"),
            serde_json::to_vec_pretty(&trace)?,
        )?;
        Ok(())
    }

    fn advance(&mut self, session: &mut FieldSession) -> Result<()> {
        ensure!(
            !session.events.world.blocked_by_movie(),
            "unexpected movie inside classroom event"
        );
        let tick = session.events.tick();
        let interact = session.dialogue.values().any(|p| {
            if p.closed
                || p.persistent
                || !p.operation.is_pending()
                || !p.fully_revealed()
                || !p.accepts_input()
                || !p.voice_finished()
            {
                return false;
            }
            tick - *self
                .ready_since
                .entry((p.operation.id(), p.page))
                .or_insert(tick)
                >= 120
        });
        session.step(FieldInput {
            interact,
            ..Default::default()
        })?;
        self.observe_audio(session);
        Ok(())
    }
}
fn audio_frame(tick: u32) -> u64 {
    u64::from(tick) * u64::from(crate::field_audio::PCM_SPEC.sample_rate) / u64::from(FPS)
}
fn summary(
    spec: &ClassroomShowcase,
    start: u32,
    end: u32,
    frames: u32,
    reason: &str,
) -> serde_json::Value {
    serde_json::json!({
        "width":spec.resolution[0],"height":spec.resolution[1],"fps":FPS,"frames":frames,"duration_seconds":frames as f64 / FPS as f64,
        "start_tick":start,"end_tick":end,"stop_reason":reason,"lighting_samples_per_frame":spec.samples,
        "plan_only":spec.plan_only,"audio":spec.audio,"simulation_updates_per_video_frame":1,
        "pathtracing":spec.pathtracing,"strong_smaa":spec.strong_smaa,
        "audio_sample_rate":crate::field_audio::PCM_SPEC.sample_rate,
        "from_frame":spec.from_frame,"from_seconds":f64::from(spec.from_frame)/f64::from(FPS),
    })
}
pub(super) fn plan(
    mut session: FieldSession,
    root: &Path,
    output: &Path,
    spec: &ClassroomShowcase,
    mut director: Director,
) -> Result<()> {
    ensure!(!output.exists(), "capture output already exists");
    fs::create_dir_all(output)?;
    let start = session.events.tick();
    let mut milestones = Vec::new();
    for frame in 0..36000 {
        let world = &session.events.world;
        if frame % FPS == 0 || spec.stop_reason(&session, frame + 1).is_some() {
            milestones.push(serde_json::json!({"frame":frame,"tick":world.tick,
                "event_frame":spec.from_frame + frame,
                "brightness":world.brightness(),"input_enabled":world.input_enabled,
                "controlled_actor":world.controlled_actor,"emotes":format!("{:?}",world.emotes),
                "dialogue":session.dialogue.values().filter(|p| !p.closed).map(|p|p.current().text()).collect::<Vec<_>>() }));
        }
        if let Some(reason) = spec.stop_reason(&session, frame + 1) {
            if spec.audio {
                director.record_audio(root, output, start, frame + 1)?;
            }
            let mut result = summary(spec, start, world.tick, frame + 1, reason);
            result["milestones"] = milestones.into();
            fs::write(
                output.join("recording.json"),
                serde_json::to_vec_pretty(&result)?,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }
        director.advance(&mut session)?;
    }
    anyhow::bail!("classroom did not hand control to Lloyd within 600 seconds")
}
#[derive(Resource)]
struct Recording {
    spec: ClassroomShowcase,
    output: PathBuf,
    start_tick: Option<u32>,
    frame: u32,
    settled: u32,
    accumulation: Option<Accumulation>,
    requested: bool,
    reported_samples: u32,
    advance: bool,
    director: Director,
    since: Instant,
}
pub(super) fn install(
    app: &mut App,
    output: &Path,
    spec: &ClassroomShowcase,
    director: Director,
) -> Result<()> {
    ensure!(!output.exists(), "capture output already exists");
    fs::create_dir_all(output.join("frames"))?;
    fs::write(
        output.join("settings.json"),
        serde_json::to_vec_pretty(spec)?,
    )?;
    app.insert_resource(Recording {
        spec: spec.clone(),
        output: output.into(),
        start_tick: None,
        frame: 0,
        settled: 0,
        accumulation: None,
        requested: false,
        reported_samples: 0,
        advance: false,
        director,
        since: Instant::now(),
    })
    .add_systems(PreUpdate, advance)
    .add_systems(
        PostUpdate,
        capture
            .after(bevy::transform::TransformSystems::Propagate)
            .after(bevy::asset::AssetEventSystems),
    );
    Ok(())
}
fn advance(
    mut recording: ResMut<Recording>,
    mut session: ResMut<Session>,
    mut exit: MessageWriter<AppExit>,
) {
    if recording.since.elapsed().as_secs() > 300 + u64::from(recording.spec.samples) * 30 {
        error!("event capture timed out waiting for a ray-traced frame");
        exit.write(AppExit::error());
        return;
    }
    if !recording.advance {
        return;
    }
    recording.advance = false;
    if recording.frame >= 35999 {
        error!("classroom did not hand control to Lloyd within 600 seconds");
        exit.write(AppExit::error());
        return;
    }
    if let Err(error) = recording.director.advance(&mut session.0) {
        error!("event capture update failed: {error:#}");
        exit.write(AppExit::error());
        return;
    }
    recording.frame += 1;
    recording.settled = 0;
    recording.accumulation = None;
    recording.requested = false;
    recording.reported_samples = 0;
    recording.since = Instant::now();
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Scene readiness, lighting progress and screenshot destination.
fn capture(
    mut commands: Commands,
    mut recording: ResMut<Recording>,
    session: Res<Session>,
    art: Res<Art>,
    ready: Res<crate::RenderReady>,
    refraction: Res<crate::field_refraction::Ready>,
    target: Res<crate::Framebuffer>,
    roots: Query<&ActorPart>,
    mut cameras: Query<
        (Entity, Option<&mut bevy::solari::prelude::SolariLighting>),
        With<crate::FieldCamera>,
    >,
    traced: Query<
        (),
        (
            With<crate::FieldCamera>,
            Or<(
                With<bevy::solari::prelude::SolariLighting>,
                With<crate::ray_tracing::pathtracer::Pathtraced>,
            )>,
        ),
    >,
    ui: Res<crate::field_ui::Artwork>,
    mut exit: MessageWriter<AppExit>,
) {
    if recording.requested {
        return;
    }
    let settle_frames = if recording.frame == 0 { 20 } else { 1 };
    if recording.settled < settle_frames {
        if art.ready
            && !roots.is_empty()
            && roots.iter().all(|p| p.prepared)
            && ready.0.load(std::sync::atomic::Ordering::Relaxed)
            && refraction.get()
        {
            recording.settled += 1;
        } else {
            recording.settled = 0;
        }
        return;
    }
    if recording.accumulation.is_none() {
        if traced.is_empty() {
            error!("event capture requires ray queries; select Lavapipe or a supported GPU");
            exit.write(AppExit::error());
            return;
        }
        let accumulation = Accumulation::new(recording.frame, recording.spec.samples);
        for (entity, lighting) in &mut cameras {
            if let Some(mut lighting) = lighting {
                // A video frame freezes a new pose/camera. Its lighting samples
                // must not reuse reservoirs from the previous game tick.
                lighting.reset = true;
            }
            commands.entity(entity).insert(accumulation.clone());
        }
        recording.start_tick.get_or_insert(session.0.events.tick());
        recording.accumulation = Some(accumulation);
        return;
    }
    let completed = recording.accumulation.as_ref().unwrap().completed();
    if completed < recording.spec.samples {
        if completed >= recording.reported_samples + (recording.spec.samples / 128).max(32) {
            info!(
                "Frame {} lighting: {completed}/{} samples",
                recording.frame, recording.spec.samples
            );
            recording.reported_samples = completed;
        }
        return;
    }
    recording.requested = true;
    // Keep the cameras active: Bevy redirects the screenshot target to an
    // intermediate texture that must be rendered before it can be read back.
    let path = recording
        .output
        .join(format!("frames/frame-{:06}.png", recording.frame));
    let world = &session.0.events.world;
    let reason = recording.spec.stop_reason(&session.0, recording.frame + 1);
    let result = reason.map(|reason| {
        summary(
            &recording.spec,
            recording.start_tick.unwrap(),
            world.tick,
            recording.frame + 1,
            reason,
        )
    });
    let state = serde_json::json!({
        "frame":recording.frame,"time_seconds":recording.frame as f64 / FPS as f64,"tick":world.tick,
        "event_frame":recording.spec.from_frame + recording.frame,
        "event_time_seconds":f64::from(recording.spec.from_frame + recording.frame)/f64::from(FPS),
        "lighting_samples":completed,"brightness":world.brightness(),"input_enabled":world.input_enabled,
        "controlled_actor":world.controlled_actor,"renderer":if recording.spec.pathtracing {"Bevy path tracer"} else {"Bevy Solari ReSTIR"},
        "emotes":format!("{:?}",world.emotes),"dialogue_layouts":ui.diagnostic_layouts(&session.0),
    });
    commands.spawn(Screenshot(target.0.clone())).observe(
        move |event: On<ScreenshotCaptured>,
              mut recording: ResMut<Recording>,
              root: Res<super::Root>,
              mut exit: MessageWriter<AppExit>| {
            let write = (|| -> Result<()> {
                let mut saved_state = state.clone();
                saved_state["render_seconds"] = recording.since.elapsed().as_secs_f64().into();
                crate::screenshot::write(&event.image, &path, Some(&saved_state))?;
                if let Some(result) = &result {
                    if recording.spec.audio {
                        recording.director.record_audio(
                            &root.0,
                            &recording.output,
                            recording.start_tick.unwrap(),
                            recording.frame + 1,
                        )?;
                    }
                    fs::write(
                        recording.output.join("recording.json"),
                        serde_json::to_vec_pretty(result)?,
                    )?;
                }
                Ok(())
            })();
            if let Err(error) = write {
                error!("event screenshot failed: {error:#}");
                exit.write(AppExit::error());
                return;
            }
            info!(
                "Saved frame {} (video {:.3}s, tick {}, {} lighting samples): {}",
                recording.frame,
                recording.frame as f64 / FPS as f64,
                state["tick"],
                state["lighting_samples"],
                path.display()
            );
            if result.is_some() {
                exit.write(AppExit::Success);
            } else {
                recording.advance = true;
            }
        },
    );
}
