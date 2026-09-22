//! Replay ordinary keyboard input from a free-control save without output devices.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::path::Path;
    let mut args = std::env::args().skip(1);
    let save = args
        .next()
        .context("SAVE REPLAY.json OUTPUT [COOKED_ROOT] [WIDTHxHEIGHT]")?;
    let spec = args.next().context("REPLAY.json")?;
    let output = args.next().context("OUTPUT")?;
    let root = args.next().unwrap_or_else(|| "local/all-assets".into());
    let resolution = args
        .next()
        .map(|s| s.parse())
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_default();
    anyhow::ensure!(args.next().is_none(), "unexpected replay argument");
    resonance_presentation::record_checkpoint_with_display(
        Path::new(&root),
        Path::new(&save),
        Path::new(&output),
        &serde_json::from_slice(&std::fs::read(spec)?)?,
        resolution,
    )
}
