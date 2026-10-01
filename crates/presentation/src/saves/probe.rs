//! Exercise the live save/load path and GPU resource reuse without an audio device.
use super::*;
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, atomic::Ordering},
};

const SAMPLE_COUNT: usize = 100;
const P95_INDEX: usize = SAMPLE_COUNT * 95 / 100 - 1;
const TARGET_MS: f64 = 250.;

pub fn run_quicksave_probe(
    root: &Path,
    checkpoint: &Path,
    output: &Path,
    transitions: bool,
) -> Result<()> {
    let mut app = app(root, checkpoint, output, crate::Resolution::default())?;
    let outcome = Arc::new(Mutex::new(None));
    app.insert_resource(Probe {
        started: Instant::now(),
        loaded: None,
        baseline: None,
        samples: Vec::new(),
        captures: Vec::new(),
        output: output.into(),
        outcome: outcome.clone(),
    })
    .add_systems(Update, drive.before(update));
    let exit = app.run();
    outcome
        .lock()
        .unwrap()
        .take()
        .context("quicksave probe did not complete")??;
    ensure!(
        exit == AppExit::Success,
        "quicksave probe exited with an error"
    );
    if transitions {
        // Navigation has its own ordinary-input scenario; benchmark samples
        // above retain the uncapped live clock and real wall-time measurements.
        drop(app);
        let scenario =
            serde_json::from_str(include_str!("../../examples/scenarios/field-route.json"))?;
        let route_output = output.join("route");
        replay::record_checkpoint(
            root,
            checkpoint,
            &route_output,
            &scenario,
            replay::CheckpointRecordingOptions {
                resolution: crate::Resolution::default(),
                paranoid: true,
                gamepad: false,
                save_directory: None,
            },
        )?;
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(route_output.join("recording.json"))?)?;
        let village = replay::capture_from(&report, "village")?;
        let returned = replay::capture_from(&report, "school-returned")?;
        ensure!(
            village["asset_reads"] == returned["asset_reads"],
            "revisited field reloaded its prepared assets"
        );
    }
    Ok(())
}

pub(super) fn app(
    root: &Path,
    checkpoint: &Path,
    output: &Path,
    resolution: crate::Resolution,
) -> Result<App> {
    app_with_saves(
        root,
        output,
        SaveOptions {
            directory: Some(output.join("slots")),
            quick_slot: Some("probe".into()),
            load: Some(checkpoint.into()),
        },
        resolution,
    )
}
pub(super) fn app_with_saves(
    root: &Path,
    output: &Path,
    saves: SaveOptions,
    resolution: crate::Resolution,
) -> Result<App> {
    app_with_saves_mode(root, output, saves, resolution, true)
}

pub(super) fn app_with_saves_mode(
    root: &Path,
    output: &Path,
    saves: SaveOptions,
    resolution: crate::Resolution,
    paranoid: bool,
) -> Result<App> {
    ensure!(!output.exists(), "probe output must be a fresh directory");
    fs::create_dir_all(output)?;
    let (mut app, _) = crate::build_app_with_display(
        crate::RunOptions {
            script_root: None,
            assets: root.into(),
            reveal: saves.load.is_none(),
            saves,
            capture_at: None,
            capture: Some(output.join("unused.png")),
            silent: true,
            paranoid,
            skip_intro: true,
            selected: 0,
            record_playthrough: None,
            record_title_ticks: 0,
            skip_battles: false,
            allow_incomplete_scripts: false,
        },
        resolution,
    )?;
    bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO).build(&mut app);
    Ok(app)
}

#[derive(Resource)]
struct Probe {
    started: Instant,
    loaded: Option<Instant>,
    baseline: Option<FieldCheckpoint>,
    samples: Vec<f64>,
    captures: Vec<f64>,
    output: PathBuf,
    outcome: Arc<Mutex<Option<Result<()>>>>,
}

fn drive(world: &mut World) {
    let mut probe = world.remove_resource::<Probe>().unwrap();
    let result = probe.step(world);
    if let Err(error) = result {
        error!("Quicksave probe failed: {error:#}");
        *probe.outcome.lock().unwrap() = Some(Err(error));
        world.write_message(AppExit::error());
    }
    world.insert_resource(probe);
}
impl Probe {
    fn step(&mut self, world: &mut World) -> Result<()> {
        ensure!(
            self.started.elapsed().as_secs() < 120,
            "quicksave probe timed out"
        );
        let Ok(state) = checkpoint(world) else {
            return Ok(());
        };
        if self.baseline.is_none() {
            self.baseline = Some(state);
            let started = Instant::now();
            save(world)?;
            self.captures.push(started.elapsed().as_secs_f64() * 1000.);
            return Ok(());
        }
        if world.resource::<Persistence>().is_writing() {
            return Ok(());
        }
        let baseline = self.baseline.as_ref().unwrap();
        if let Some(started) = self.loaded.take() {
            replay::assert_checkpoint(
                world
                    .resource::<new_game::Session>()
                    .restored_checkpoint
                    .as_ref()
                    .context("loaded field has no publication snapshot")?,
                baseline,
            )?;
            ensure!(
                !world.contains_resource::<RetainedFrame>(),
                "restored field image was not released"
            );
            self.samples.push(started.elapsed().as_secs_f64() * 1000.);
            if self.samples.len() == 1 {
                fs::write(
                    self.output.join("restored.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "saved": baseline,
                        "restored": world.resource::<new_game::Session>().restored_checkpoint,
                    }))?,
                )?;
            }
            ensure!(
                state.progress.tick >= baseline.progress.tick
                    && state.played_ticks >= baseline.played_ticks,
                "restored field did not resume its clocks"
            );
            if self.samples.len() < SAMPLE_COUNT {
                self.baseline = Some(state);
                let started = Instant::now();
                save(world)?;
                self.captures.push(started.elapsed().as_secs_f64() * 1000.);
                return Ok(());
            }
        }
        if self.samples.len() == SAMPLE_COUNT {
            finish_timings(
                &self.output,
                &self.samples,
                &self.captures,
                world
                    .resource::<loading::Resident>()
                    .unprepared_reads
                    .load(Ordering::Acquire),
                self.started.elapsed().as_secs_f64(),
            )?;
            *self.outcome.lock().unwrap() = Some(Ok(()));
            world.write_message(AppExit::Success);
            return Ok(());
        }
        let persistence = world.resource::<Persistence>();
        let bytes = persistence.store.read(Kind::Quicksave, &persistence.slot)?;
        let (_, SceneCheckpoint::Field(saved)) = resonance_persistence::decode(&bytes)?
            .admit(&world.resource::<new_game::Session>().identity)?
        else {
            anyhow::bail!("probe requires a field save");
        };
        ensure!(
            serde_json::to_value(&saved)? == serde_json::to_value(baseline)?,
            "quicksave changed the captured payload"
        );
        // Change live state so a no-op restore cannot pass this probe.
        let mut session = world.resource_mut::<new_game::Session>();
        session
            .field_mut()
            .step(resonance_game::field::FieldInput {
                direction: [1., 0.],
                ..Default::default()
            })?;
        session
            .field_mut()
            .events
            .world
            .party
            .as_mut()
            .unwrap()
            .gald = baseline.progress.party.gald ^ 1;
        self.loaded = Some(Instant::now());
        load(world)?;
        Ok(())
    }
}

/// Publish measurements even when the declared target fails, then return the
/// failure to the caller so a successful process cannot hide a missed gate.
fn finish_timings(
    output: &Path,
    samples: &[f64],
    captures: &[f64],
    unprepared_reads: u64,
    elapsed_seconds: f64,
) -> Result<()> {
    ensure!(
        samples.len() == SAMPLE_COUNT && captures.len() == SAMPLE_COUNT,
        "quicksave probe has incomplete timing samples"
    );
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mut sorted_captures = captures.to_vec();
    sorted_captures.sort_by(f64::total_cmp);
    let load_p95 = sorted[P95_INDEX];
    let capture_p95 = sorted_captures[P95_INDEX];
    let target_met = load_p95 < TARGET_MS && capture_p95 < TARGET_MS;
    fs::write(
        output.join("timings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1, "resolution": [640, 480], "audio_device": false,
            "presentation": "uncapped, like the live player; gameplay uses its normal fixed tick",
            "metric": "quicksave read + field initialization to first render update with prepared actors and free control; excludes display scanout",
            "samples_ms":samples, "p95_ms":load_p95, "max_ms":sorted[SAMPLE_COUNT - 1],
            "capture_samples_ms":captures, "capture_p95_ms":capture_p95,
            "warm_target_ms":TARGET_MS as u32, "capture_target_ms":TARGET_MS as u32,
            "target_met":target_met, "unprepared_reads":unprepared_reads, "elapsed_seconds":elapsed_seconds,
        }))?,
    )?;
    info!(
        "{SAMPLE_COUNT} quicksaves/loads: capture p95={capture_p95:.2} ms; warm load p95={load_p95:.2} ms"
    );
    ensure!(
        unprepared_reads == 0,
        "warm loads read unprepared resources"
    );
    ensure!(
        target_met,
        "quicksave performance target missed: capture p95={capture_p95:.2} ms, warm load p95={load_p95:.2} ms; both must be below {TARGET_MS:.2} ms (metrics: {})",
        output.join("timings.json").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quicksave_target_failure_preserves_metrics_and_identifies_the_slow_operation() {
        let output = std::env::temp_dir().join(format!(
            "resonance-quicksave-target-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&output).unwrap();
        for (load, capture, succeeds) in
            [(249., 249., true), (250., 249., false), (249., 251., false)]
        {
            let result = finish_timings(
                &output,
                &[load; SAMPLE_COUNT],
                &[capture; SAMPLE_COUNT],
                0,
                1.,
            );
            let metrics: serde_json::Value =
                serde_json::from_slice(&fs::read(output.join("timings.json")).unwrap()).unwrap();
            assert_eq!(metrics["target_met"], succeeds);
            assert_eq!(metrics["p95_ms"], load);
            assert_eq!(metrics["capture_p95_ms"], capture);
            assert_eq!(result.is_ok(), succeeds);
            if let Err(error) = result {
                let message = error.to_string();
                assert!(message.contains("quicksave performance target missed"));
                assert!(message.contains(&format!("capture p95={capture:.2}")));
                assert!(message.contains(&format!("warm load p95={load:.2}")));
            }
        }
        fs::remove_dir_all(output).unwrap();
    }
}
