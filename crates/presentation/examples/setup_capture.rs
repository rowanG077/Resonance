//! A silent setup-prompt checkpoint for equivalent-state oracle comparisons.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .unwrap_or_else(|| "local/native/setup.png".into());
    let tick = args.next().map(|a| a.parse()).transpose()?.unwrap_or(300);
    resonance_presentation::capture_setup(
        std::path::Path::new("local/cooked"),
        std::path::Path::new(&output),
        tick,
    )
}
