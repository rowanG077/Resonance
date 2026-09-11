//! Silent live menu/save/load validation and screenshots.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::path::Path;
    let mut args = std::env::args().skip(1);
    let checkpoint = args
        .next()
        .context("SAVE.json OUTPUT_DIRECTORY [COOKED_ROOT]")?;
    let output = args.next().context("OUTPUT_DIRECTORY")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    resonance_presentation::run_menu_probe(
        Path::new(&root),
        Path::new(&checkpoint),
        Path::new(&output),
    )
}
