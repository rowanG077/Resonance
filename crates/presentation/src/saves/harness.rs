//! Shared device-free application setup for save menus and scenario replay.
use super::*;
use std::{fs, path::Path};

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
    app_with_target(root, output, saves, resolution, paranoid, false)
}

pub(super) fn app_with_target(
    root: &Path,
    output: &Path,
    saves: SaveOptions,
    resolution: crate::Resolution,
    paranoid: bool,
    window: bool,
) -> Result<App> {
    ensure!(!output.exists(), "probe output must be a fresh directory");
    fs::create_dir_all(output)?;
    let (mut app, _) = crate::build_app_with_display(
        crate::RunOptions {
            mods: Vec::new(),
            script_root: None,
            assets: root.into(),
            reveal: saves.load.is_none(),
            saves,
            presentation_start: None,
            replay: None,
            capture_at: None,
            capture: (!window).then(|| output.join("unused.png")),
            silent: true,
            paranoid,
            skip_intro: true,
            record_playthrough: None,
            record_title_ticks: 0,
            skip_battles: false,
            allow_incomplete_scripts: false,
        },
        resolution,
    )?;
    if !window {
        bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO).build(&mut app);
    }
    Ok(app)
}
