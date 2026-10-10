//! Shared command-line adapter; execution and readback belong to the application.
pub fn run(sequence: bool) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::{fs, path::Path};
    let mut args = std::env::args().skip(1);
    let input = args.next().context("CAPTURE.json OUTPUT [ASSETS]")?;
    let output = args.next().context("OUTPUT")?;
    let root = args.next().unwrap_or_else(|| "local/all-assets".into());
    ensure!(args.next().is_none(), "unexpected arguments");
    let spec = serde_json::from_slice(&fs::read(input)?)?;
    let capture = if sequence {
        resonance_presentation::capture_field_sequence
    } else {
        resonance_presentation::capture_field
    };
    capture(Path::new(&root), Path::new(&output), &spec)
}
