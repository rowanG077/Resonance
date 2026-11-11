//! Driver for tools/residency_probe.py; diagnostics stay outside the regression.
use super::scenario::Scenario;
use anyhow::{Result, ensure};
use bevy::{prelude::*, time::TimeUpdateStrategy};
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

#[test]
#[ignore = "diagnostic tool; run through tools/residency_probe.py"]
fn residency_probe() -> Result<()> {
    let output = std::env::var_os("RESONANCE_RESIDENCY_CAPTURE").map(std::path::PathBuf::from);
    if let Some(output) = &output {
        std::fs::create_dir_all(output)?;
    }
    let resolution = if let Ok(resolution) = std::env::var("RESONANCE_RESIDENCY_RESOLUTION") {
        resolution.parse().map_err(anyhow::Error::msg)?
    } else if output.is_some() {
        "1920x1080"
            .parse::<crate::Resolution>()
            .map_err(anyhow::Error::msg)?
    } else {
        Default::default()
    };
    let audit = std::env::var_os("RESONANCE_PIPELINE_AUDIT").is_some();
    let benchmark_rounds = std::env::var("RESONANCE_BENCHMARK_ROUNDS")
        .ok()
        .map(|rounds| rounds.parse::<u32>())
        .transpose()?;
    let samples = std::env::var("RESONANCE_RESIDENCY_MSAA")
        .ok()
        .map(|samples| {
            use resonance_content::menu_data::AntiAliasing;
            match samples.as_str() {
                "2" => Ok(AntiAliasing::Msaa2),
                "4" => Ok(AntiAliasing::Msaa4),
                "8" => Ok(AntiAliasing::Msaa8),
                _ => anyhow::bail!("unsupported residency MSAA probe mode"),
            }
        })
        .transpose()?;
    let mut scenario = Scenario::boot(resolution, |app| {
        crate::preparation_profile::install(app);
        if benchmark_rounds.is_some() {
            crate::battle::skip_shatter_duration_for_benchmark(app);
        }
        if audit {
            crate::model_asset::pipeline_test::install(app);
        }
        if let Some(mode) = samples {
            app.add_systems(
                PostUpdate,
                move |mut aa: ResMut<crate::anti_aliasing::AntiAliasingSettings>,
                      caps: Res<crate::anti_aliasing::AntiAliasingCapabilities>| {
                    aa.0 = caps.effective(mode);
                },
            );
        }
    })?;
    eprintln!(
        "Residency boot: {:.1} ms; pipeline preparation overlapped construction: {}",
        scenario.boot_ms, scenario.preparation_overlapped
    );
    if let Some(rounds) = benchmark_rounds {
        return benchmark(&mut scenario, resolution, rounds);
    }
    for map in [332, 340, 341, 376, 520, 374, 332, 3000, 332, 3000] {
        // Build the initial save fixture before timing production admission.
        scenario.request_scene(map)?;
        scenario.wait_for_scene(map)?;
        eprintln!(
            "Scene {map}: {:.1} ms",
            scenario
                .app
                .world()
                .resource::<crate::transition_metrics::Trace>()
                .last
                .as_ref()
                .unwrap()
                .total_ms
        );
        if let Some(output) = &output {
            capture_scene(&mut scenario, output, map)?;
        }
    }
    let request = scenario.battle()?;
    eprintln!(
        "Battle entry: {:.1} ms",
        scenario
            .app
            .world()
            .resource::<crate::transition_metrics::Trace>()
            .last
            .as_ref()
            .unwrap()
            .total_ms
    );
    if let Some(output) = &output {
        let capture_tick = scenario
            .app
            .world()
            .resource::<crate::battle::Owner>()
            .frame_update()
            .unwrap()
            + 16;
        scenario
            .app
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
        scenario.wait("battle capture did not advance", |app| {
            app.world()
                .resource::<crate::battle::Owner>()
                .frame_update()
                .is_some_and(|tick| {
                    assert!(tick <= capture_tick, "battle passed capture tick: {tick}");
                    tick == capture_tick
                })
        })?;
        capture(&mut scenario.app, &output.join("battle.png"))?;
    }
    scenario.return_from_battle(request)?;
    if audit {
        crate::model_asset::pipeline_test::check(scenario.app.world());
    }
    let profile = crate::preparation_profile::take();
    if !profile.is_null() {
        eprintln!("PREPARATION_PROFILE {profile}");
    }
    Ok(())
}

fn benchmark(scenario: &mut Scenario, resolution: crate::Resolution, rounds: u32) -> Result<()> {
    ensure!(rounds > 0, "benchmark rounds must be positive");
    let path = std::env::var_os("RESONANCE_BENCHMARK_JSONL")
        .ok_or_else(|| anyhow::anyhow!("benchmark output path required"))?;
    let mut output = std::fs::File::create(path)?;
    let adapter = scenario
        .app
        .world()
        .resource::<bevy::render::renderer::RenderAdapterInfo>();
    let metadata = serde_json::json!({
        "kind": "metadata", "rounds": rounds, "resolution": resolution,
        "adapter": format!("{:?}", **adapter),
        "anti_aliasing": format!("{:?}", scenario.app.world().resource::<crate::anti_aliasing::AntiAliasingSettings>().0),
        "boot_ms": scenario.boot_ms, "formation": 1, "arena": 13,
        "simulation": "held field clock; realtime battle clock; shatter minimum duration disabled",
        "shatter_minimum_duration_disabled": true,
        "timing": "production admission through opening-frame GPU completion",
        "preparation_trace": crate::preparation_profile::enabled(),
    });
    writeln!(output, "{metadata}")?;
    eprintln!("BENCHMARK {metadata}");
    let _ = crate::preparation_profile::take();
    scenario.enter(332)?;
    record(scenario, &mut output, "initial_field", 0)?;
    settle_field(scenario)?;
    for round in 0..rounds {
        for map in [340, 341, 376, 520, 374, 332] {
            scenario.enter(map)?;
            record(scenario, &mut output, "field_transition", round)?;
            settle_field(scenario)?;
        }
        let request = scenario.battle_realtime()?;
        record(scenario, &mut output, "field_battle", round)?;
        scenario.return_from_battle(request)?;
        record(scenario, &mut output, "battle_return", round)?;
        scenario.enter(3000)?;
        record(scenario, &mut output, "field_to_world", round)?;
        let request = scenario.battle_realtime()?;
        record(scenario, &mut output, "world_battle", round)?;
        scenario.return_from_battle(request)?;
        record(scenario, &mut output, "battle_return", round)?;
        scenario.enter(332)?;
        record(scenario, &mut output, "world_to_field", round)?;
        settle_field(scenario)?;
        eprintln!("BENCHMARK round {}/{rounds} complete", round + 1);
    }
    Ok(())
}

fn settle_field(scenario: &mut Scenario) -> Result<()> {
    scenario
        .app
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
    scenario.wait("benchmark field entry fade did not open", |app| {
        let events = app.world().resource::<crate::new_game::Session>().events();
        events.world.brightness() >= 1.
    })?;
    let _ = crate::preparation_profile::take();
    Ok(())
}

fn record(scenario: &Scenario, output: &mut std::fs::File, kind: &str, round: u32) -> Result<()> {
    let trace = scenario
        .app
        .world()
        .resource::<crate::transition_metrics::Trace>();
    let transition = trace.last.as_ref().unwrap();
    ensure!(transition.outcome == "ready", "benchmark transition failed");
    let value = serde_json::json!({
        "kind": kind, "round": round, "transition": transition,
        "preparation_scopes": crate::preparation_profile::take(),
    });
    writeln!(output, "{value}")?;
    output.flush()?;
    eprintln!(
        "BENCHMARK {kind} round={round} {:.3} ms",
        transition.total_ms
    );
    Ok(())
}

fn capture_scene(scenario: &mut Scenario, output: &Path, map: u32) -> Result<()> {
    if map < 3000 {
        scenario
            .app
            .insert_resource(TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
        scenario.wait("field entry fade did not open", |app| {
            let events = app.world().resource::<crate::new_game::Session>().events();
            events
                .world
                .fade
                .as_ref()
                .is_none_or(|fade| fade.alpha(events.tick()) <= 0.)
        })?;
    }
    capture(
        &mut scenario.app,
        &output.join(if map >= 3000 {
            "world.png".into()
        } else {
            format!("field.{map}.png")
        }),
    )
}

fn capture(app: &mut App, path: &Path) -> Result<()> {
    let failed = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicU32::new(0));
    crate::new_game_capture::screenshot_held(app, path.into(), failed.clone(), written.clone())?;
    ensure!(
        !failed.load(Ordering::Acquire) && written.load(Ordering::Acquire) == 1,
        "scene readback failed"
    );
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    Ok(())
}
