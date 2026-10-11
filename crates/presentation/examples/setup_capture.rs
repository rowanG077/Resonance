//! A silent setup-prompt checkpoint for equivalent-state oracle comparisons.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .unwrap_or_else(|| "local/native/setup.png".into());
    let tick = args.next().map(|a| a.parse()).transpose()?.unwrap_or(300);
    let preferences = args
        .next()
        .map(|path| -> anyhow::Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
        .transpose()?;
    anyhow::ensure!(args.next().is_none(), "unexpected arguments");
    let assets = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map_or_else(|| "local/all-assets".into(), std::path::PathBuf::from);
    resonance_presentation::capture_setup(
        &assets,
        std::path::Path::new(&output),
        tick,
        preferences.as_ref(),
    )
}
