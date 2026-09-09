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
    time::Instant,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSequence {
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
    /// Held input changes, indexed from the first recorded update.
    #[serde(default)]
    pub movement: Vec<FieldMovement>,
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
        ensure!(
            (1..=3600).contains(&self.updates) && (1..=8).contains(&self.renders_per_update),
            "invalid sequence length/cadence"
        );
        ensure!(
            self.direction
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.),
            "invalid sequence direction"
        );
        ensure!(
            self.accept_updates.iter().all(|u| *u < self.updates),
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
    let frame_count = recording.spec.updates * recording.spec.renders_per_update;
    if recording.since.elapsed().as_secs() > 60 + u64::from(frame_count) / 20 {
        error!("field sequence timed out");
        exit.write(AppExit::error());
        return;
    }
    if recording.settled < 20
        || recording.frame >= recording.spec.updates * recording.spec.renders_per_update
    {
        return;
    }
    if recording
        .frame
        .is_multiple_of(recording.spec.renders_per_update)
    {
        let update = recording.frame / recording.spec.renders_per_update;
        let movement = recording
            .spec
            .movement
            .iter()
            .rev()
            .find(|m| m.update <= update);
        if let Err(error) = session.0.step(resonance_game::field::FieldInput {
            direction: movement.map_or(recording.spec.direction, |m| m.direction),
            run: movement.map_or(recording.spec.run, |m| m.run),
            interact: recording.spec.accept_updates.contains(&update),
            ..Default::default()
        }) {
            error!("field sequence update failed: {error:#}");
            exit.write(AppExit::error());
            return;
        }
        session.0.events.world.audio_commands.clear();
    }
    recording.frame += 1;
}
#[allow(clippy::too_many_arguments)] // Readiness, current poses and GPU readback.
fn capture(
    mut commands: Commands,
    mut recording: ResMut<Recording>,
    session: Res<Session>,
    art: Res<Art>,
    ready: Res<crate::RenderReady>,
    target: Res<crate::Framebuffer>,
    roots: Query<(Entity, &ActorPart)>,
    children: Query<&Children>,
    bones: Query<(&Name, &Transform, &GlobalTransform)>,
    ui: Res<crate::field_ui::Artwork>,
    effects: Query<(&Mesh3d, &Visibility), With<crate::field_effects::EffectDraw>>,
    meshes: Res<Assets<Mesh>>,
) {
    if recording.settled < 20 {
        if art.ready
            && !roots.is_empty()
            && roots.iter().all(|(_, p)| p.prepared)
            && ready.0.load(std::sync::atomic::Ordering::Relaxed)
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
    let frame = recording.frame - 1;
    let path = recording.output.join(format!("frame-{frame:04}.png"));
    let world = &session.0.events.world;
    let state = serde_json::json!({
        "frame":frame, "tick":world.tick, "audio_device":false,
        "input_enabled":world.input_enabled,
        "actors":world.actors.iter().map(|(id,a)|serde_json::json!({"id":id,"position":a.position,"heading":a.heading,"animation":a.animation.as_ref().map(|a|serde_json::json!({"slot":a.slot,"start_tick":a.start_tick,"sample":a.sample(world.tick,0,a.duration_ticks as f32)}))})).collect::<Vec<_>>(),
        "poses":roots.iter().filter(|(_,p)|p.actor==world.controlled_actor && p.part==0).flat_map(|(root,_)|children.iter_descendants(root)).filter_map(|e|bones.get(e).ok()).map(|(name,t,g)|serde_json::json!({"name":name.as_str(),"translation":t.translation.to_array(),"rotation":t.rotation.to_array(),"world":g.to_matrix().to_cols_array()})).collect::<Vec<_>>(),
        "emotes":format!("{:?}",world.emotes),
        "effects":effects.iter().map(|(mesh,visibility)|serde_json::json!({"visibility":format!("{visibility:?}"),"positions":format!("{:?}",meshes.get(mesh).and_then(|m|m.attribute(Mesh::ATTRIBUTE_POSITION)))})).collect::<Vec<_>>(),
        "dialogue_layouts":ui.diagnostic_layouts(&session.0),
    });
    commands.spawn(Screenshot(target.0.clone())).observe(
        move |event: On<ScreenshotCaptured>,
              mut recording: ResMut<Recording>,
              mut exit: MessageWriter<AppExit>| {
            let result = (|| -> Result<()> {
                event.image.clone().try_into_dynamic()?.save(&path)?;
                fs::write(path.with_extension("json"), serde_json::to_vec(&state)?)?;
                Ok(())
            })();
            if let Err(error) = result {
                error!("sequence capture failed: {error:#}");
                exit.write(AppExit::error());
                return;
            }
            recording.captured += 1;
            if recording.captured == recording.spec.updates * recording.spec.renders_per_update {
                exit.write(AppExit::Success);
            }
        },
    );
    if recording.frame == recording.spec.updates * recording.spec.renders_per_update {
        recording.frame += 1;
    }
}
