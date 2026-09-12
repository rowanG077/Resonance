//! Branch/resource validation through the field runtime and silent presentation audio.
use anyhow::{Context, Result};
use std::path::Path;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let map = args
        .next()
        .context("MAP STORY OUTPUT.json [COOKED_ROOT]")?
        .parse()?;
    let story = args.next().context("STORY")?.parse()?;
    let output = args.next().context("OUTPUT.json")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    resonance_presentation::check_field_events(Path::new(&root), map, story, Path::new(&output))
}
