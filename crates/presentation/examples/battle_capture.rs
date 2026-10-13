//! Silent battle scene capture through the ordinary scene owner.
use anyhow::{Context, Result, ensure};
use resonance_events::battle::{DefeatPolicy, Encounter, Setup};
use std::{fs, path::PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let save = PathBuf::from(
        args.next()
            .context("SAVE REPLAY OUTPUT ASSETS FORMATION ARENA [RESOLUTION [ROUTE [MUSIC [DEFEAT]]]]")?,
    );
    let replay = args.next().context("REPLAY")?;
    let output = PathBuf::from(args.next().context("OUTPUT")?);
    let root = PathBuf::from(args.next().context("ASSETS")?);
    let formation = args.next().context("FORMATION")?.parse()?;
    let arena = args.next().context("ARENA")?.parse()?;
    let resolution = args
        .next()
        .map(|s| s.parse())
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_default();
    let route = args
        .next()
        .map(|value| -> Result<[i32; 5]> {
            value
                .split(',')
                .map(str::parse)
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| anyhow::anyhow!("ROUTE requires five comma-separated integers"))
        })
        .transpose()?
        .unwrap_or([0; 5]);
    let music = args
        .next()
        .filter(|value| value != "none")
        .map(|value| value.parse())
        .transpose()?;
    let defeat = match args.next().as_deref() {
        None | Some("resume_event") => DefeatPolicy::ResumeEvent,
        Some("game_over") => DefeatPolicy::GameOver,
        Some(_) => anyhow::bail!("DEFEAT must be resume_event or game_over"),
    };
    ensure!(args.next().is_none(), "unexpected argument");
    resonance_presentation::record_battle(
        &root,
        &save,
        &output,
        Setup {
            encounter: Encounter::Formation(formation),
            arena,
            defeat,
            music,
            route,
        },
        &serde_json::from_slice(&fs::read(replay)?)?,
        resonance_presentation::CheckpointRecordingOptions {
            resolution,
            paranoid: true,
            ..Default::default()
        },
    )
}
