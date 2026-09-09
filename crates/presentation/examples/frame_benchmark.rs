fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args.next().expect("output directory");
    let size = args
        .next()
        .unwrap_or_else(|| "1920x1080".into())
        .parse()
        .map_err(anyhow::Error::msg)?;
    let seconds = args.next().unwrap_or_else(|| "120".into()).parse()?;
    let profile = args.next().is_some_and(|arg| arg == "profile");
    resonance_presentation::run_frame_benchmark(
        std::path::Path::new("local/cooked"),
        std::path::Path::new(&output),
        size,
        seconds,
        profile,
    )
}
