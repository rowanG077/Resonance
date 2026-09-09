//! High-resolution diagnostic. This deliberately does not change oracle APIs.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args.next().expect("output directory");
    let resolution = args
        .next()
        .expect("WIDTHxHEIGHT")
        .parse()
        .map_err(anyhow::Error::msg)?;
    let endpoint = args.next().unwrap_or_else(|| "new-game-setup".into());
    let assets = args.next().unwrap_or_else(|| "local/cooked".into());
    resonance_presentation::record_new_game_display(
        std::path::Path::new(&assets),
        std::path::Path::new(&output),
        resolution,
        &endpoint,
    )
}
