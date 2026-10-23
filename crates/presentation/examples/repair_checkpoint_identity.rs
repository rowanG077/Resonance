//! Write a separate checkpoint copy after proving saved-state content equivalence.
use anyhow::{Context, Result, ensure};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .context("[--check] SAVE SOURCE_LIBRARY TARGET_LIBRARY [NEW_SAVE]")?;
    let check = first == "--check";
    let input = PathBuf::from(if check {
        args.next().context("SAVE")?
    } else {
        first
    });
    let source = PathBuf::from(args.next().context("SOURCE_LIBRARY")?);
    let target = PathBuf::from(args.next().context("TARGET_LIBRARY")?);
    let output = if check {
        None
    } else {
        Some(PathBuf::from(args.next().context("NEW_SAVE")?))
    };
    ensure!(args.next().is_none(), "unexpected argument");
    let report = match output {
        Some(output) => {
            resonance_presentation::repair_checkpoint_identity(&input, &source, &target, &output)?
        }
        None => resonance_presentation::check_checkpoint_identity(&input, &source, &target)?,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
