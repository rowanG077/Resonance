//! KTX2 files carry their dimensions, color space and mip levels together.
use anyhow::{Result, ensure};
use bevy::{
    asset::io::{AssetReader, AssetReaderError, ErasedAssetReader, PathStream, Reader, VecReader},
    image::{CompressedImageFormatSupport, CompressedImageFormats, ImageLoaderSettings, ImageType},
    prelude::*,
};
use std::{
    path::Path,
    sync::{Arc, OnceLock},
};
#[cfg(test)]
mod tests;

/// Block formats the adapter accepts, published once the renderer is ready.
/// Until then textures are served as plain RGBA.
#[derive(Resource, Clone, Default)]
pub(super) struct Supported(Arc<OnceLock<CompressedImageFormats>>);

pub(super) struct TexturePlugin;
impl Plugin for TexturePlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let world = app.world();
        let Some(supported) = world.get_resource::<Supported>() else {
            return;
        };
        match world.get_resource::<CompressedImageFormatSupport>() {
            Some(formats) => {
                let _ = supported.0.set(formats.0);
            }
            // Without it every texture would silently be expanded to RGBA.
            None => warn!("texture format support is unknown; BC7 textures will be decoded"),
        }
    }
}

/// Serves every texture in a form the stock image loader can use, so the
/// loader itself needs no replacement.
pub(super) struct Textures {
    pub reader: Box<dyn ErasedAssetReader>,
    pub supported: Supported,
}
impl AssetReader for Textures {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let mut reader = self.reader.read(path).await?;
        if path.extension().is_none_or(|extension| extension != "ktx2") {
            return Ok(reader);
        }
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let formats = self
            .supported
            .0
            .get()
            .copied()
            .unwrap_or(CompressedImageFormats::NONE);
        let bytes = prepare(bytes, formats)
            .map_err(|error| std::io::Error::other(format!("{}: {error:#}", path.display())))?;
        Ok(Box::new(VecReader::new(bytes)) as Box<dyn Reader>)
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.reader.read_meta(path).await
    }
    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.reader.read_directory(path).await
    }
    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.reader.is_directory(path).await
    }
}

/// Plain files with textures decoded to RGBA, for tests without a renderer.
#[cfg(test)]
pub(super) fn decoded_files(app: &mut App, root: impl AsRef<Path>) {
    use bevy::asset::io::{AssetSource, AssetSourceBuilder, AssetSourceId};
    let root = root.as_ref().to_string_lossy().into_owned();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || {
            Box::new(Textures {
                reader: AssetSource::get_default_reader(root.clone())(),
                supported: Supported::default(),
            })
        }),
    );
}

/// Cooked textures are BC7; plain RGBA8 files are accepted as they are.
fn prepare(bytes: Vec<u8>, formats: CompressedImageFormats) -> Result<Vec<u8>> {
    if resonance_asset_writer::gpu::is_native_compressed(&bytes)?
        && !formats.contains(CompressedImageFormats::BC)
    {
        resonance_asset_writer::gpu::rgba8(&bytes)
    } else {
        Ok(bytes)
    }
}

pub(super) fn decode(
    path: &str,
    settings: &ImageLoaderSettings,
    formats: CompressedImageFormats,
    bytes: Vec<u8>,
) -> Result<Image> {
    ensure!(path.ends_with(".ktx2"), "textures must use KTX2: {path}");
    let bytes = prepare(bytes, formats)?;
    Ok(Image::from_buffer(
        &bytes,
        ImageType::Extension("ktx2"),
        formats,
        settings.is_srgb,
        settings.sampler.clone(),
        settings.asset_usage,
    )?)
}
