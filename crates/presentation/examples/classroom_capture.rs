//! Silent development checkpoint; this is not the New Game acceptance route.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .unwrap_or_else(|| "local/native/classroom.png".into());
    let tick = args.next().map(|a| a.parse()).transpose()?;
    resonance_presentation::capture_classroom(
        std::path::Path::new("local/cooked"),
        std::path::Path::new(&output),
        tick,
    )
}
