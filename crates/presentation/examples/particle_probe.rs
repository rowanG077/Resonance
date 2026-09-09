//! Silent seeded effect diagnosis; this is not a normal-route acceptance capture.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::{fs, path::Path};
    let mut args = std::env::args().skip(1);
    let spec = args.next().context("PROBE.json OUTPUT.png [COOKED_ROOT]")?;
    let output = args.next().context("OUTPUT.png")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    let probe = serde_json::from_slice(&fs::read(&spec)?)?;
    resonance_presentation::capture_classroom_particles(
        Path::new(&root),
        Path::new(&output),
        &probe,
    )?;
    fs::copy(spec, Path::new(&output).with_extension("probe.json"))?;
    Ok(())
}
