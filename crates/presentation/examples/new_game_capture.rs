//! Run a semantic scenario from the normal New Game entry without output devices.
use anyhow::{Context, Result, ensure};
use resonance_presentation::{CheckpointRecordingOptions, record_new_game};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut positional = Vec::new();
    let mut options = CheckpointRecordingOptions::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--gamepad" => options.gamepad = true,
            "--paranoid" => options.paranoid = true,
            _ => {
                ensure!(!arg.starts_with("--"), "unknown option {arg}");
                positional.push(arg);
            }
        }
    }
    let mut args = positional.into_iter();
    let spec = args
        .next()
        .context("REPLAY.json OUTPUT [COOKED_ROOT] [WIDTHxHEIGHT] [--gamepad] [--paranoid]")?;
    let output = PathBuf::from(args.next().context("OUTPUT")?);
    let assets = PathBuf::from(args.next().unwrap_or_else(|| "local/all-assets".into()));
    if let Some(resolution) = args.next() {
        options.resolution = resolution.parse().map_err(anyhow::Error::msg)?;
    }
    ensure!(args.next().is_none(), "unexpected scenario argument");
    record_new_game(
        &assets,
        &output,
        &serde_json::from_slice(&std::fs::read(spec)?)?,
        options,
    )
}
