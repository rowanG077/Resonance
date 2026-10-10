fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args.next().expect("output directory");
    let mut stalls = false;
    let mut known_content = false;
    for arg in args {
        match arg.as_str() {
            "stalls" => stalls = true,
            "known-content" => known_content = true,
            _ => anyhow::bail!("unknown movie probe option {arg}"),
        }
    }
    resonance_presentation::run_movie_probe(
        std::path::Path::new("local/all-assets"),
        std::path::Path::new(&output),
        stalls,
        known_content,
    )
}
