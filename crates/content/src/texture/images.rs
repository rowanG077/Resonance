//! Atlas pages are ordinary image paths. Dimensions come from captured headers.
use crate::font::UiTexture;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S: Serializer>(images: &[UiTexture], serializer: S) -> Result<S::Ok, S::Error> {
    images
        .iter()
        .map(|image| &image.path)
        .collect::<Vec<_>>()
        .serialize(serializer)
}
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<UiTexture>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|path| {
            crate::validate_asset_path(&path).map_err(serde::de::Error::custom)?;
            Ok(UiTexture {
                path,
                width: 0,
                height: 0,
            })
        })
        .collect()
}
pub fn prepare<B: AsRef<[u8]>>(
    image: &mut UiTexture,
    read: &mut impl FnMut(&str) -> Result<B>,
) -> Result<()> {
    let [width, height] = resonance_asset_writer::texture::dimensions(read(&image.path)?.as_ref())
        .with_context(|| image.path.clone())?;
    image.width = width;
    image.height = height;
    image.validate()
}
