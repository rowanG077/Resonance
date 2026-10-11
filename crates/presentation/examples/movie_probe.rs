fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args.next().expect("output directory");
    let stalls = args.next().is_some_and(|arg| arg == "stalls");
    resonance_presentation::run_movie_probe(
        std::path::Path::new("local/cooked"),
        std::path::Path::new(&output),
        stalls,
    )
}
