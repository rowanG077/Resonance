//! Silent original-asset world observer through the production renderer.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| "local/all-assets".into());
    let probe = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("provide a world probe JSON"))?;
    let output = args
        .next()
        .unwrap_or_else(|| "local/native/overworld.png".into());
    resonance_presentation::capture_overworld(
        std::path::Path::new(&root),
        std::path::Path::new(&output),
        &serde_json::from_slice(&std::fs::read(probe)?)?,
    )
}
