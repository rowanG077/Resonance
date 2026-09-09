//! Silent consecutive-frame rendering diagnostic, including uncapped redraws.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::{fs, path::Path};
    let mut args = std::env::args().skip(1);
    let spec = args
        .next()
        .context("SEQUENCE.json OUTPUT_DIR [COOKED_ROOT]")?;
    let output = args.next().context("OUTPUT_DIR")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    let sequence = serde_json::from_slice(&fs::read(spec)?)?;
    resonance_presentation::capture_field_sequence(Path::new(&root), Path::new(&output), &sequence)
}
