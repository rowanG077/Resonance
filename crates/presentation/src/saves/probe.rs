//! Exercise the live save/load path and GPU resource reuse without an audio device.
use super::*;
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
    let completed = Arc::new(AtomicBool::new(false));
    app.insert_resource(Probe {
        started: Instant::now(),
        loaded: None,
        baseline: None,
        written: false,
        samples: Vec::new(),
        captures: Vec::new(),
        output: output.into(),
        completed: completed.clone(),
        route: transitions.then(Route::default),
    })
    .add_systems(Update, drive.before(update));
    ensure!(
        app.run() == AppExit::Success && completed.load(Ordering::Acquire),
        "quicksave probe did not complete"
    );
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
    ensure!(!output.exists(), "probe output must be a fresh directory");
    fs::create_dir_all(output)?;
    let (mut app, _) = crate::build_app_with_display(
        crate::RunOptions {
            script_root: None,
            assets: root.into(),
            reveal: saves.load.is_none(),
            saves,
            capture: Some(output.join("unused.png")),
            silent: true,
            skip_intro: true,
            tick: None,
            presentation_start: None,
            selected: 0,
            replay: None,
            movie_frame: None,
            boot_frame: None,
            record_playthrough: None,
            record_title_ticks: 0,
        },
        resolution,
    )?;
    // The application was built without a window/device; run its normal updates.
    // Setup must choose an offscreen target before automatic capture is disabled.
    app.add_systems(
        Startup,
        (|mut options: ResMut<crate::RunOptions>| options.capture = None).after(crate::setup),
    );
    bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO).build(&mut app);
    Ok(app)
}

#[derive(Resource)]
struct Probe {
    started: Instant,
    loaded: Option<Instant>,
    baseline: Option<FieldCheckpoint>,
    written: bool,
    samples: Vec<f64>,
    captures: Vec<f64>,
    output: PathBuf,
    completed: Arc<AtomicBool>,
    route: Option<Route>,
}

/// Registered-trigger coverage for the live loader, distinct from walking and
/// oracle comparison. Start from free control in the school grounds after Frank.
#[derive(Default)]
struct Route {
    started: Option<(Instant, u64)>,
    records: Vec<serde_json::Value>,
}
impl Route {
    fn step(&mut self, world: &mut World, state: &FieldCheckpoint) -> Result<bool> {
        const STEPS: [(u32, u32, bool, u32); 4] = [
            (332, 1001, false, 330),
            (330, 1002, false, 332),
            (332, 1011, true, 340),
            (340, 1000, true, 332),
        ];
        let step = STEPS[self.records.len()];
        if let Some((started, reads)) = self.started.take() {
            ensure!(state.map_id == step.3, "route reached the wrong field");
            let reads = world
                .resource::<loading::Resident>()
                .memory_reads
                .load(Ordering::Acquire)
                - reads;
            ensure!(
                step.3 != 332 || reads == 0,
                "revisited field reloaded its prepared assets"
            );
            self.records.push(serde_json::json!({
                "from":step.0, "trigger":step.1, "confirmed":step.2, "to":step.3,
                "milliseconds":started.elapsed().as_secs_f64()*1000.,
                "asset_reads":reads, "checkpoint":state,
            }));
            return Ok(self.records.len() == STEPS.len());
        }
        ensure!(
            state.map_id == step.0,
            "transition probe requires the post-Frank school-grounds checkpoint"
        );
        let mut session = world.resource_mut::<new_game::Session>();
        ensure!(
            session.field.story_progress()? == 2500,
            "transition probe requires story 2500"
        );
        ensure!(
            session.field.events.trigger(step.1, step.2)?,
            "route trigger is unavailable"
        );
        self.started = Some((
            Instant::now(),
            world
                .resource::<loading::Resident>()
                .memory_reads
                .load(Ordering::Acquire),
        ));
        Ok(false)
    }
}
fn drive(world: &mut World) {
    let mut probe = world.remove_resource::<Probe>().unwrap();
    let result = probe.step(world);
    world.insert_resource(probe);
    if let Err(error) = result {
        error!("Quicksave probe failed: {error:#}");
        world.write_message(AppExit::error());
    }
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
            // A request during a transition must not create a pending save.
            world.resource_mut::<new_game::Session>().ready_for_field = false;
            ensure!(save(world).is_err(), "saved during scene preparation");
            world.resource_mut::<new_game::Session>().ready_for_field = true;
            ensure!(
                !world.resource::<Persistence>().is_writing(),
                "unavailable save was queued"
            );
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
            ensure!(
                !world.contains_resource::<RetainedFrame>(),
                "restored field image was not released"
            );
            self.samples.push(started.elapsed().as_secs_f64() * 1000.);
            ensure!(
                state.position == baseline.position
                    && state.heading == baseline.heading
                    && state.progress.script_globals == baseline.progress.script_globals
                    && state.progress.party.gald == baseline.progress.party.gald,
                "restore changed player pose, script globals or gald"
            );
            if self.samples.len() < SAMPLE_COUNT {
                let started = Instant::now();
                save(world)?;
                self.captures.push(started.elapsed().as_secs_f64() * 1000.);
                return Ok(());
            }
        }
        if self.samples.len() == SAMPLE_COUNT {
            if let Some(route) = &mut self.route
                && !route.step(world, &state)?
            {
                return Ok(());
            }
            let mut sorted = self.samples.clone();
            sorted.sort_by(f64::total_cmp);
            let mut captures = self.captures.clone();
            captures.sort_by(f64::total_cmp);
            let late_reads = world
                .resource::<loading::Resident>()
                .late_reads
                .load(Ordering::Acquire);
            ensure!(late_reads == 0, "warm loads read unprepared resources");
            fs::write(
                self.output.join("timings.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "version": 1, "resolution": [640, 480], "audio_device": false,
                    "presentation": "uncapped, like the live player; gameplay uses its normal fixed tick",
                    "metric": "quicksave read + field initialization to first render update with prepared actors and free control; excludes display scanout",
                    "samples_ms": self.samples, "p95_ms": sorted[P95_INDEX], "max_ms": sorted[SAMPLE_COUNT - 1],
                    "capture_samples_ms": self.captures, "capture_p95_ms": captures[P95_INDEX],
                    "warm_target_ms": TARGET_MS as u32, "capture_target_ms": TARGET_MS as u32,
                    "target_met": sorted[P95_INDEX] < TARGET_MS && captures[P95_INDEX] < TARGET_MS, "late_asset_reads": late_reads,
                    "elapsed_seconds": self.started.elapsed().as_secs_f64(),
                    "registered_trigger_route": self.route.as_ref().map(|r| &r.records),
                }))?,
            )?;
            info!(
                "{SAMPLE_COUNT} quicksaves/loads: capture p95={:.2} ms; warm load p95={:.2} ms, max={:.2} ms",
                captures[P95_INDEX],
                sorted[P95_INDEX],
                sorted[SAMPLE_COUNT - 1]
            );
            self.completed.store(true, Ordering::Release);
            world.write_message(AppExit::Success);
            return Ok(());
        }
        if !self.written {
            let persistence = world.resource::<Persistence>();
            let bytes = persistence.store.read(Kind::Quicksave, &persistence.slot)?;
            resonance_persistence::decode::<FieldCheckpoint>(
                &bytes,
                &world.resource::<new_game::Session>().identity,
            )?;
            self.written = true;
        }
        // Change live state so a no-op restore cannot pass this probe.
        let mut session = world.resource_mut::<new_game::Session>();
        session.field.step(resonance_game::field::FieldInput {
            direction: [1., 0.],
            ..Default::default()
        })?;
        session.field.events.world.party.as_mut().unwrap().gald = baseline.progress.party.gald ^ 1;
        self.loaded = Some(Instant::now());
        load(world)?;
        Ok(())
    }
}

pub(super) fn capture(world: &mut World, path: PathBuf, done: &Arc<AtomicBool>) {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    let done = done.clone();
    done.store(false, Ordering::Release);
    let target = world.resource::<crate::Framebuffer>().0.clone();
    world.spawn(Screenshot(target)).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            if let Err(error) = crate::screenshot::write(&event.image, &path, None) {
                error!("Probe capture failed: {error:#}");
                exit.write(AppExit::error());
            }
            done.store(true, Ordering::Release);
        },
    );
}
