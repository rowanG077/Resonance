//! Editable image resources use the same retained source as their owning scene.
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::{AssetLoader, AssetPath, LoadContext, io::Reader},
    image::{CompressedImageFormats, ImageFormatSetting, ImageLoaderSettings, ImageType},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use resonance_content::texture::{ColorSpace, Definition, TEXTURE_LIMIT};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Default, TypePath)]
struct Loader;
impl AssetLoader for Loader {
    type Asset = Image;
    type Settings = ImageLoaderSettings;
    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &ImageLoaderSettings,
        context: &mut LoadContext<'_>,
    ) -> Result<Image> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let definition = definition(&bytes)?;
        let source = context.path().source().clone_owned();
        let mut images = BTreeMap::new();
        let mut total = 0usize;
        for path in definition.images() {
            if images.contains_key(path) {
                continue;
            }
            let asset = AssetPath::from(path.to_owned()).with_source(source.clone());
            let bytes = context.read_asset_bytes(asset).await?;
            total += bytes.len();
            ensure!(
                bytes.len() <= resonance_content::texture::PNG_LIMIT && total <= TEXTURE_LIMIT,
                "texture exceeds its read budget: {path}"
            );
            images.insert(path.to_owned(), bytes);
        }
        prepare(&definition, settings, |path| {
            images
                .get(path)
                .map(Vec::as_slice)
                .context("missing captured texture level")
        })
    }

    fn extensions(&self) -> &[&str] {
        &["texture.json"]
    }
}

pub(super) fn install(app: &mut App) {
    app.init_asset_loader::<Loader>();
}

fn definition(bytes: &[u8]) -> Result<Definition> {
    ensure!(
        bytes.len() <= 1024 * 1024,
        "texture definition exceeds read budget"
    );
    let definition: Definition = serde_json::from_slice(bytes)?;
    definition.validate()?;
    Ok(definition)
}

fn prepare<B: AsRef<[u8]>>(
    definition: &Definition,
    settings: &ImageLoaderSettings,
    read: impl FnMut(&str) -> Result<B>,
) -> Result<Image> {
    ensure!(
        settings.array_layout.is_none()
            && matches!(settings.format, ImageFormatSetting::FromExtension),
        "texture definitions describe ordinary two-dimensional images"
    );
    let format = match definition.color_space {
        ColorSpace::Linear => TextureFormat::Rgba8Unorm,
        ColorSpace::Srgb => TextureFormat::Rgba8UnormSrgb,
    };
    ensure!(
        settings
            .texture_format
            .is_none_or(|requested| requested == format),
        "image settings contradict the texture definition's color space"
    );
    let levels = definition.decode(read)?;
    let mut image = Image::new_uninit(
        Extent3d {
            width: levels[0].width,
            height: levels[0].height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        settings.asset_usage,
    );
    image.texture_descriptor.mip_level_count = levels.len().try_into()?;
    image.data = Some(levels.into_iter().flat_map(|level| level.rgba).collect());
    image.sampler = settings.sampler.clone();
    Ok(image)
}

/// Synchronous UI preparation follows exactly the same descriptor and PNG rules
/// as asynchronous scene loading, using the caller's captured file set.
pub(super) fn decode(
    path: &str,
    settings: &ImageLoaderSettings,
    read: &impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<Image> {
    let bytes = read(path)?;
    if path.ends_with(".texture.json") {
        return prepare(&definition(&bytes)?, settings, read);
    }
    let extension = path.rsplit_once('.').context("image has no extension")?.1;
    Ok(Image::from_buffer(
        &bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        settings.is_srgb,
        settings.sampler.clone(),
        settings.asset_usage,
    )?)
}
