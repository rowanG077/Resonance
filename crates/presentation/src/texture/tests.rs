use super::*;
use bevy::{asset::RenderAssetUsages, image::ImageSampler, render::render_resource::TextureFormat};
use resonance_asset_writer::{gpu, texture};

#[test]
fn bc7_uploads_as_blocks_or_decodes_where_the_adapter_lacks_them() -> Result<()> {
    // Mips below one block are the ones an adapter is most likely to reject.
    let levels = [(12, 8), (6, 4), (3, 2), (1, 1)]
        .into_iter()
        .map(|(width, height)| texture::Pixels {
            width,
            height,
            rgba: [143, 78, 25, 255].repeat((width * height) as usize),
        })
        .collect::<Vec<_>>();
    let bytes = gpu::encode(&levels)?;
    for (formats, expected) in [
        (CompressedImageFormats::BC, TextureFormat::Bc7RgbaUnorm),
        (CompressedImageFormats::NONE, TextureFormat::Rgba8Unorm),
    ] {
        let prepared = prepare(bytes.clone(), formats)?;
        let image = Image::from_buffer(
            &prepared,
            ImageType::Extension("ktx2"),
            formats,
            false,
            ImageSampler::linear(),
            RenderAssetUsages::MAIN_WORLD,
        )?;
        assert_eq!(image.texture_descriptor.format, expected);
        assert_eq!(image.texture_descriptor.mip_level_count, 4);
        assert_eq!(texture::ktx2(&prepared)?, texture::ktx2(&bytes)?);
    }
    // Blocks that overhang the image cannot be uploaded.
    let mut overhanging = bytes;
    overhanging[20..24].copy_from_slice(&10u32.to_le_bytes());
    assert!(prepare(overhanging, CompressedImageFormats::BC).is_err());
    Ok(())
}
