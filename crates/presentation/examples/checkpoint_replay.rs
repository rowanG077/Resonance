//! Replay ordinary keyboard input from a free-control save without output devices.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::path::Path;
    let mut args = std::env::args().skip(1);
    let save = args
        .next()
        .context("SAVE REPLAY.json OUTPUT [COOKED_ROOT]")?;
    let spec = args.next().context("REPLAY.json")?;
    let output = args.next().context("OUTPUT")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    resonance_presentation::record_checkpoint(
        Path::new(&root),
        Path::new(&save),
        Path::new(&output),
        &serde_json::from_slice(&std::fs::read(spec)?)?,
    )
}
