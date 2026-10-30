use super::*;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    render::render_resource::TextureFormat,
};

#[test]
fn ktx_preserves_authored_mips_and_sampler() -> Result<()> {
    let base = vec![37; 16];
    let mip = vec![17, 31, 59, 127];
    let bytes = resonance_asset_writer::ktx2::encode_levels(2, 2, &[&base, &mip])?;
    let settings = ImageLoaderSettings {
        is_srgb: false,
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::nearest()
        }),
        asset_usage: RenderAssetUsages::MAIN_WORLD,
        ..default()
    };
    let image = decode("wood.ktx2", &settings, &|_| Ok(bytes.clone()))?;
    assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
    assert_eq!(image.texture_descriptor.mip_level_count, 2);
    assert_eq!([image.width(), image.height()], [2, 2]);
    assert_eq!(image.data.as_ref().unwrap(), &[base, mip].concat());
    assert!(matches!(&image.sampler, ImageSampler::Descriptor(sampler)
        if sampler.address_mode_u == ImageAddressMode::Repeat));
    Ok(())
}
