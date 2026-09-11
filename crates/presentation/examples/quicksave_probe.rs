//! Device-free live quicksave/quickload benchmark.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::path::Path;
    let mut args = std::env::args().skip(1);
    let checkpoint = args
        .next()
        .context("SAVE.json OUTPUT_DIRECTORY [COOKED_ROOT]")?;
    let output = args.next().context("OUTPUT_DIRECTORY")?;
    let rest: Vec<_> = args.collect();
    let root = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .map_or("local/cooked", String::as_str);
    resonance_presentation::run_quicksave_probe(
        Path::new(root),
        Path::new(&checkpoint),
        Path::new(&output),
        rest.iter().any(|a| a == "--route"),
    )
}
