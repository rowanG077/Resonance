//! Authoring helper: tightly packed RGBA8 pixels to the engine's lossless KTX2 profile.
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: rgba_to_ktx2 WIDTH HEIGHT INPUT.rgba OUTPUT.ktx2"
    );
    let width = args[0]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid width"))?
        .parse()?;
    let height = args[1]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid height"))?
        .parse()?;
    let pixels = std::fs::read(&args[2])?;
    std::fs::write(
        &args[3],
        resonance_asset_writer::ktx2::encode_rgba8(width, height, &pixels)?,
    )?;
    Ok(())
}
