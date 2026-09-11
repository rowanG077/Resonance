//! Load a previous process's menu save through the title screen, without audio.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::path::Path;
    let mut args = std::env::args().skip(1);
    let directory = args
        .next()
        .context("SAVE_DIRECTORY OUTPUT_DIRECTORY [COOKED_ROOT]")?;
    let output = args.next().context("OUTPUT_DIRECTORY")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    resonance_presentation::run_title_load_probe(
        Path::new(&root),
        Path::new(&directory),
        Path::new(&output),
    )
}
