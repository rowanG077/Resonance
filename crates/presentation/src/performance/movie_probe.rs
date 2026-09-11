//! Muted real-window evidence for fullscreen movie deadlines and render stalls.
use anyhow::{Result, ensure};
use bevy::prelude::*;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub fn run_movie_probe(root: &Path, output: &Path, stalls: bool) -> Result<()> {
    ensure!(!output.exists(), "movie probe output already exists");
    std::fs::create_dir_all(output)?;
    let (mut app, _) = crate::build_app_with_display(
        crate::RunOptions {
            saves: Default::default(),
            assets: root.into(),
            tick: None,
            presentation_start: None,
            capture: None,
            reveal: false,
            selected: 0,
            silent: true,
            replay: None,
            movie_frame: None,
            boot_frame: None,
            skip_intro: false,
            record_playthrough: None,
            record_title_ticks: 1000,
        },
        "1920x1080".parse().map_err(anyhow::Error::msg)?,
    )?;
    super::install(
        &mut app,
        super::PerformanceOptions {
            overlay: false,
            dump: Some(output.join("frames.jsonl")),
        },
        false,
    )?;
    app.insert_resource(Probe {
        output: output.into(),
        stalls,
        began: Instant::now(),
        first: None,
        updates: 0,
        selected: 0,
        last: None,
        lateness: Vec::new(),
        title: None,
    });
    app.add_systems(PreUpdate, stall.before(crate::gather_input));
    app.add_systems(Last, observe);
    ensure!(app.run() == AppExit::Success, "movie probe failed");
    ensure!(
        output.join("movie.json").is_file(),
        "movie probe did not complete"
    );
    Ok(())
}
#[derive(Resource)]
struct Probe {
    output: PathBuf,
    stalls: bool,
    began: Instant,
    first: Option<Instant>,
    updates: u64,
    selected: u32,
    last: Option<u32>,
    lateness: Vec<f64>,
    title: Option<Instant>,
}
fn stall(probe: Res<Probe>, movie: Res<crate::movie::Playback>) {
    if probe.stalls
        && movie.is_presenting()
        && probe.updates > 0
        && probe.updates.is_multiple_of(30)
    {
        std::thread::sleep(Duration::from_millis(200));
    }
}
fn observe(world: &mut World) {
    let movie = world.resource::<crate::movie::Playback>();
    let (active, completed, frame, timestamp, drops, position) = (
        movie.active,
        movie.completed_naturally,
        movie.presented_frame,
        movie.presented_timestamp,
        movie.dropped_frames,
        movie.position(world),
    );
    let mut probe = world.resource_mut::<Probe>();
    if probe.began.elapsed() > Duration::from_secs(240) {
        error!("Movie window probe timed out");
        world.write_message(AppExit::error());
        return;
    }
    if active && frame.is_some() {
        probe.first.get_or_insert_with(Instant::now);
        probe.updates += 1;
        if probe.last != frame {
            probe.last = frame;
            probe.selected += 1;
            if let (Some(position), Some(timestamp)) = (position, timestamp) {
                probe
                    .lateness
                    .push(position.saturating_sub(timestamp).as_secs_f64() * 1000.);
            }
        }
    } else if completed {
        if probe.title.is_none() {
            probe.title = Some(Instant::now());
            let mut lateness = probe.lateness.clone();
            lateness.sort_by(f64::total_cmp);
            let elapsed = probe.first.unwrap().elapsed().as_secs_f64();
            let result = serde_json::json!({
                "silent":true, "stalls_ms":if probe.stalls {200}else{0},
                "seconds":elapsed, "app_updates":probe.updates,
                "updates_per_second":probe.updates as f64 / elapsed,
                "selected_and_uploaded":probe.selected, "skipped_due_frames":drops,
                "cpu_selection_lateness_p99_ms":lateness[(lateness.len() * 99 / 100).min(lateness.len()-1)],
                "cpu_selection_lateness_max_ms":lateness.last(),
                "timing":"relative to estimated audible clock; excludes physical display latency",
            });
            std::fs::write(
                probe.output.join("movie.json"),
                serde_json::to_vec_pretty(&result).unwrap(),
            )
            .unwrap();
            info!("Movie window probe: {result}");
        } else if probe
            .title
            .is_some_and(|time| time.elapsed() > Duration::from_secs(5))
        {
            world.write_message(AppExit::Success);
        }
    }
}
