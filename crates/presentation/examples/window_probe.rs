fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args.next().expect("output directory");
    let resolution = args
        .next()
        .unwrap_or_else(|| "1920x1080".into())
        .parse()
        .map_err(anyhow::Error::msg)?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    resonance_presentation::run_window_probe(
        std::path::Path::new(&root),
        std::path::Path::new(&output),
        resolution,
    )
}
