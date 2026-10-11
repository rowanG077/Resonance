//! Silent fully revealed dialogue checkpoint; does not replay the movie audio.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .expect("OUTPUT and dialogue text prefix are required");
    let prefix = args.next().expect("dialogue text prefix is required");
    let hold_ticks = args.next().map(|s| s.parse()).transpose()?.unwrap_or(0);
    let preferences = args
        .next()
        .map(|path| -> anyhow::Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
        .transpose()?;
    anyhow::ensure!(args.next().is_none(), "unexpected arguments");
    let assets = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map_or_else(|| "local/all-assets".into(), std::path::PathBuf::from);
    resonance_presentation::capture_dialogue(
        &assets,
        std::path::Path::new(&output),
        &prefix,
        hold_ticks,
        preferences.as_ref(),
    )
}
