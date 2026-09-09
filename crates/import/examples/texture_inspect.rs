//! Inspect decoded source texels with nearest-neighbor enlargement, silently.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let source = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected source TPL"))?;
    let output = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected output directory"))?;
    std::fs::create_dir_all(&output)?;
    for (index, (width, height, rgba)) in resonance_import::tpl::decode(&std::fs::read(source)?)?
        .into_iter()
        .enumerate()
    {
        let image = image::RgbaImage::from_raw(width, height, rgba).unwrap();
        image::imageops::resize(
            &image,
            width * 4,
            height * 4,
            image::imageops::FilterType::Nearest,
        )
        .save(std::path::Path::new(&output).join(format!("{index}.png")))?;
    }
    Ok(())
}
