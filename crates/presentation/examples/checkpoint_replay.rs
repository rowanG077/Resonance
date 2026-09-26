//! Replay ordinary keyboard input from a free-control save without output devices.
use anyhow::{Context, Result, ensure};
use std::path::PathBuf;

struct Args {
    save: PathBuf,
    spec: PathBuf,
    output: PathBuf,
    root: PathBuf,
    resolution: resonance_presentation::Resolution,
    save_directory: Option<PathBuf>,
    paranoid: bool,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut positionals = Vec::new();
        let mut save_directory = None;
        let mut paranoid = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--save-directory" => {
                    ensure!(save_directory.is_none(), "duplicate --save-directory");
                    let path = args.next().context("--save-directory PATH")?;
                    ensure!(!path.starts_with("--"), "--save-directory PATH");
                    save_directory = Some(PathBuf::from(path));
                }
                "--paranoid" => {
                    ensure!(!paranoid, "duplicate --paranoid");
                    paranoid = true;
                }
                _ => {
                    ensure!(!arg.starts_with("--"), "unknown option {arg}");
                    positionals.push(arg);
                }
            }
        }
        let mut args = positionals.into_iter();
        let save = args.next().context(
            "SAVE REPLAY.json OUTPUT [COOKED_ROOT] [WIDTHxHEIGHT] [--save-directory PATH] [--paranoid]",
        )?.into();
        let spec = args.next().context("REPLAY.json")?.into();
        let output = args.next().context("OUTPUT")?.into();
        let root = args
            .next()
            .unwrap_or_else(|| "local/all-assets".into())
            .into();
        let resolution = args
            .next()
            .map(|s| s.parse())
            .transpose()
            .map_err(anyhow::Error::msg)?
            .unwrap_or_default();
        ensure!(args.next().is_none(), "unexpected replay argument");
        Ok(Self {
            save,
            spec,
            output,
            root,
            resolution,
            save_directory,
            paranoid,
        })
    }
}

fn main() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    resonance_presentation::record_checkpoint_with_options(
        &args.root,
        &args.save,
        &args.output,
        &serde_json::from_slice(&std::fs::read(args.spec)?)?,
        resonance_presentation::CheckpointRecordingOptions {
            resolution: args.resolution,
            save_directory: args.save_directory.as_deref(),
            paranoid: args.paranoid,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args> {
        Args::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn default_tolerant_and_explicit_paranoid_share_other_options() {
        let normal = parse(&["save", "replay.json", "output"]).unwrap();
        assert!(!normal.paranoid);
        let strict = parse(&[
            "--paranoid",
            "save",
            "replay.json",
            "output",
            "assets",
            "640x480",
            "--save-directory",
            "slots",
        ])
        .unwrap();
        assert!(strict.paranoid);
        assert_eq!(strict.save_directory, Some(PathBuf::from("slots")));
        assert_eq!(
            strict.resolution,
            resonance_presentation::Resolution::default()
        );
    }

    #[test]
    fn invalid_options_are_errors_in_both_modes() {
        for mode in [vec![], vec!["--paranoid"]] {
            let base = ["save", "replay.json", "output"];
            for suffix in [
                vec!["--typo"],
                vec!["--save-directory"],
                vec!["--save-directory", "--paranoid"],
            ] {
                let args = base
                    .iter()
                    .copied()
                    .chain(mode.iter().copied())
                    .chain(suffix)
                    .collect::<Vec<_>>();
                assert!(parse(&args).is_err());
            }
        }
        assert!(parse(&["save", "replay.json", "output", "--paranoid", "--paranoid"]).is_err());
    }
}
