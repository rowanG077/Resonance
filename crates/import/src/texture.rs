//! Physical texture metadata and the shared lossless KTX2 cooking profile.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
mod encode;

#[derive(Serialize, Deserialize)]
pub(crate) struct Texture {
    pub(crate) dimensions: [u16; 2],
    pub(crate) format: crate::tpl::Format,
    pub(crate) sampler: crate::tpl::Sampler,
    pub(crate) palette: Option<crate::tpl::Palette>,
    pub(crate) images: Vec<String>,
}
impl Texture {
    pub(crate) fn image(&self, palette: usize) -> Result<resonance_content::font::UiTexture> {
        Ok(resonance_content::font::UiTexture {
            path: self
                .images
                .get(palette)
                .context("missing cooked texture palette")?
                .clone(),
            width: u32::from(self.dimensions[0]),
            height: u32::from(self.dimensions[1]),
        })
    }

    pub(crate) fn region(
        &self,
        palette: usize,
        rect: [u32; 4],
    ) -> Result<resonance_content::font::UiRegion> {
        let region = resonance_content::font::UiRegion {
            texture: self.image(palette)?,
            rect,
        };
        region.validate()?;
        Ok(region)
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Catalogue {
    pub(crate) textures: Vec<Option<Texture>>,
}

pub(crate) fn read(path: &Path) -> Result<Catalogue> {
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("invalid texture catalogue {}", path.display()))
}

/// Read the base image of our lossless cooking profile without re-decoding its source.
pub(crate) fn pixels(path: &Path) -> Result<image::RgbaImage> {
    decode_pixels(
        &fs::read(path).with_context(|| format!("reading cooked image {}", path.display()))?,
    )
    .with_context(|| format!("invalid cooked image {}", path.display()))
}

fn decode_pixels(bytes: &[u8]) -> Result<image::RgbaImage> {
    use ktx2::{Format, SupercompressionScheme, dfd};
    let reader = ktx2::Reader::new(bytes)?;
    let header = reader.header();
    ensure!(
        header.format == Some(Format::R8G8B8A8_UNORM)
            && header.type_size == 1
            && header.pixel_width != 0
            && header.pixel_height != 0
            && header.pixel_depth == 0
            && header.layer_count == 0
            && header.face_count == 1
            && (1..=header.pixel_width.max(header.pixel_height).ilog2() + 1)
                .contains(&header.level_count),
        "expected a 2D RGBA8 cooked image"
    );
    let (dfd, _) = dfd::Basic::from_format(Format::R8G8B8A8_UNORM)?;
    ensure!(
        reader.dfd_blocks() == [dfd::Block::Basic(dfd)]
            && reader.supercompression_global_data().is_empty(),
        "unsupported cooked image color metadata"
    );
    let mut metadata_length = 0;
    for (key, value) in reader.key_value_data() {
        // The crate skips malformed entries; account for every metadata byte.
        metadata_length += (4 + key.len() + 1 + value.len()).next_multiple_of(4);
        ensure!(
            match key {
                "KTXwriter" | "KTXwriterScParams" => true,
                "KTXorientation" => value == b"rd" || value == b"rd\0",
                "KTXswizzle" => value == b"rgba" || value == b"rgba\0",
                _ => false,
            },
            "unsupported cooked image metadata {key}"
        );
    }
    ensure!(
        metadata_length == header.index.kvd_byte_length as usize,
        "malformed cooked image metadata"
    );
    let length = (u64::from(header.pixel_width) * u64::from(header.pixel_height))
        .checked_mul(4)
        .and_then(|length| length.checked_add(1))
        .context("cooked image dimensions overflow")?
        - 1;
    let level = reader.levels().next().context("missing base image")?;
    ensure!(
        level.uncompressed_byte_length == length,
        "cooked image dimensions disagree with its data length"
    );
    let rgba = match header.supercompression_scheme {
        None => level.data.to_vec(),
        Some(SupercompressionScheme::Zstandard) => {
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(level.data)?;
            let mut rgba = Vec::new();
            (&mut decoder).take(length + 1).read_to_end(&mut rgba)?;
            ensure!(
                decoder.into_inner().is_empty(),
                "trailing cooked image data"
            );
            rgba
        }
        _ => anyhow::bail!("unsupported cooked image compression"),
    };
    ensure!(
        rgba.len() as u64 == length,
        "invalid cooked image data length"
    );
    image::RgbaImage::from_raw(header.pixel_width, header.pixel_height, rgba)
        .context("invalid cooked image dimensions")
}

/// Physical images already use root-relative paths; bind every authored palette page.
pub(crate) fn bind(root: &Path, directory: &str) -> Result<Vec<Texture>> {
    resonance_content::validate_asset_path(directory)?;
    let catalogue = read(&root.join(directory).join("textures.json"))?;
    ensure!(
        !catalogue.textures.is_empty(),
        "empty cooked texture catalogue"
    );
    catalogue
        .textures
        .into_iter()
        .enumerate()
        .map(|(index, texture)| {
            let texture = texture
                .with_context(|| format!("incomplete cooked texture {directory}/{index}"))?;
            texture.sampler.validate()?;
            let stride = match texture.format {
                crate::tpl::Format::Ci4 => Some(16),
                crate::tpl::Format::Ci8 => Some(256),
                crate::tpl::Format::Ci14 => Some(16384),
                _ => None,
            };
            let pages = if let Some(stride) = stride {
                let palette = texture
                    .palette
                    .as_ref()
                    .context("missing cooked texture palette")?;
                ensure!(!palette.colors.is_empty(), "empty cooked texture palette");
                palette.colors.len().div_ceil(stride)
            } else {
                1
            };
            ensure!(
                texture.dimensions.iter().all(|&size| size != 0) && texture.images.len() == pages,
                "incomplete cooked texture images at {directory}/{index}"
            );
            for image in &texture.images {
                resonance_content::validate_asset_path(image)?;
                ensure!(
                    image.starts_with(&format!("{directory}/")) && root.join(image).is_file(),
                    "missing or incorrectly owned cooked texture {image}"
                );
            }
            Ok(texture)
        })
        .collect()
}

// Bump when the encoding profile, encoder version, or compression settings change.
pub(crate) const RECIPE: &str = "rgba8-linear-ktx2-structured-zstd-0.0.54-level9-v1";

pub(crate) fn cook(width: u32, height: u32, pixels: &[u8], output: &Path) -> Result<()> {
    let bytes = encode::encode_rgba8(width, height, pixels)
        .with_context(|| format!("encode texture {}", output.display()))?;
    crate::write_atomic(output, &bytes)
}

/// Scene exports keep editable PNGs alongside their glTF files.
pub(crate) fn cook_png(png: &Path, output: &Path) -> Result<()> {
    let image = image::open(png)
        .with_context(|| format!("read texture {}", png.display()))?
        .into_rgba8();
    cook(image.width(), image.height(), image.as_raw(), output)
}

/// Supply original mip images in level order; never generate replacement levels.
pub(crate) fn cook_levels(images: &[impl AsRef<Path>], output: &Path) -> Result<()> {
    ensure!(!images.is_empty(), "texture needs at least one image");
    let images = images
        .iter()
        .map(|path| image::open(path.as_ref()).map(image::DynamicImage::into_rgba8))
        .collect::<Result<Vec<_>, _>>()?;
    let (width, height) = images[0].dimensions();
    for (level, image) in images.iter().enumerate() {
        ensure!(
            image.dimensions()
                == (
                    (width.checked_shr(level as u32).unwrap_or(0)).max(1),
                    (height.checked_shr(level as u32).unwrap_or(0)).max(1)
                ),
            "original mip dimensions disagree with the base image"
        );
    }
    let pixels: Vec<_> = images
        .iter()
        .map(|image| image.as_raw().as_slice())
        .collect();
    crate::write_atomic(output, &encode::encode_levels(width, height, &pixels)?)
}

#[cfg(test)]
pub(crate) fn fingerprint(width: u32, height: u32, pixels: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(RECIPE.as_bytes());
    hash.update(width.to_le_bytes());
    hash.update(height.to_le_bytes());
    hash.update(pixels);
    format!("{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_container(metadata: &[u8]) -> Vec<u8> {
        use ktx2::{Format, Header, Index, LevelIndex, dfd};
        let (dfd, type_size) = dfd::Basic::from_format(Format::R8G8B8A8_UNORM).unwrap();
        let dfd = dfd::Block::Basic(dfd).to_vec();
        let dfd_offset = Header::LENGTH + 2 * LevelIndex::LENGTH;
        let kvd_offset = dfd_offset + 4 + dfd.len();
        let data_offset = kvd_offset + metadata.len();
        let header = Header {
            format: Some(Format::R8G8B8A8_UNORM),
            type_size,
            pixel_width: 2,
            pixel_height: 1,
            pixel_depth: 0,
            layer_count: 0,
            face_count: 1,
            level_count: 2,
            supercompression_scheme: None,
            index: Index {
                dfd_byte_offset: dfd_offset as u32,
                dfd_byte_length: dfd.len() as u32 + 4,
                kvd_byte_offset: kvd_offset as u32,
                kvd_byte_length: metadata.len() as u32,
                sgd_byte_offset: 0,
                sgd_byte_length: 0,
            },
        };
        let mut bytes = header.as_bytes().to_vec();
        for (offset, length) in [(data_offset, 8), (data_offset + 8, 4)] {
            bytes.extend_from_slice(
                &LevelIndex {
                    byte_offset: offset as u64,
                    byte_length: length,
                    uncompressed_byte_length: length,
                }
                .as_bytes(),
            );
        }
        bytes.extend_from_slice(&header.index.dfd_byte_length.to_le_bytes());
        bytes.extend_from_slice(&dfd);
        bytes.extend_from_slice(metadata);
        bytes.extend_from_slice(&[1, 2, 3, 4, 50, 60, 70, 80, 9, 8, 7, 6]);
        bytes
    }

    #[test]
    fn cooked_pixels_preserve_base_level_and_reject_unsupported_profiles() -> Result<()> {
        let bytes = image_container(&[]);
        let image = decode_pixels(&bytes)?;
        assert_eq!(image.dimensions(), (2, 1));
        assert_eq!(image.as_raw(), &[1, 2, 3, 4, 50, 60, 70, 80]);
        for (offset, value) in [
            (12, 43u32), // sRGB
            (16, 2),     // component size
            (20, 0),     // zero width
            (24, 0),     // 1D
            (28, 1),     // 3D
            (32, 1),     // array
            (36, 6),     // cube
            (40, 0),     // generated mipmaps
            (44, 2),     // invalid zstd payload
            (44, 3),     // unsupported compression
            (96, 7),     // wrong uncompressed length
        ] {
            let mut invalid = bytes.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode_pixels(&invalid).is_err(), "header field {offset}");
        }
        for orientation in [b"rd", b"ru"] {
            let mut metadata = 18u32.to_le_bytes().to_vec();
            metadata.extend_from_slice(b"KTXorientation\0");
            metadata.extend_from_slice(orientation);
            metadata.push(0);
            metadata.resize(metadata.len().next_multiple_of(4), 0);
            assert_eq!(
                decode_pixels(&image_container(&metadata)).is_ok(),
                orientation == b"rd"
            );
        }
        assert!(decode_pixels(&image_container(&[255; 4])).is_err());
        Ok(())
    }

    #[test]
    fn cooked_pixels_reject_truncated_containers() {
        let bytes = image_container(&[]);
        for length in 0..bytes.len() {
            assert!(decode_pixels(&bytes[..length]).is_err(), "length {length}");
        }
    }

    #[test]
    #[ignore = "requires extracted discs and cook-all; reads existing images without conversion"]
    fn original_cooked_pixels_match_source_tpls() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = local.join("all-assets");
        let mut count = 0;
        for disc in [1, 2] {
            for path in ["title.tpl", "toon.tpl"] {
                let source = fs::read(local.join(format!("extracted/disc{disc}/files/{path}")))?;
                let original = crate::tpl::parse_tpl(&source)?;
                let cooked =
                    crate::cooked::Source::open(&root, disc, path)?.standalone_textures()?;
                assert_eq!(original.len(), cooked.len());
                for (texture, cooked) in original.iter().zip(cooked) {
                    let stride = match texture.format {
                        8 => 16,
                        9 => 256,
                        10 => 16384,
                        _ => 0,
                    };
                    for (page, image) in cooked.images.iter().enumerate() {
                        let mut texture = texture.clone();
                        if stride != 0 {
                            texture.palette_offset =
                                texture.palette_offset.map(|at| at + page * stride * 2);
                            texture.palette_entries = texture
                                .palette_entries
                                .saturating_sub(page * stride)
                                .min(stride);
                        }
                        let pixels = pixels(&root.join(image))?;
                        assert_eq!(
                            pixels.dimensions(),
                            (u32::from(texture.width), u32::from(texture.height))
                        );
                        assert_eq!(
                            pixels.as_raw(),
                            &crate::tpl::decode_texture(&source, &texture)?,
                            "{image}"
                        );
                        count += 1;
                    }
                }
            }
        }
        eprintln!("{count} cooked base images exactly match original TPL pixels");
        Ok(())
    }

    #[test]
    #[ignore = "requires complete cook-all records; read-only catalogue schema check"]
    fn original_texture_catalogues_roundtrip_without_schema_changes() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets/assets");
        let mut pending = vec![root];
        let mut count = 0;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    pending.push(entry.path());
                } else if entry.file_name() == "textures.json" {
                    let bytes = fs::read(entry.path())?;
                    let parsed = read(&entry.path())?;
                    ensure!(
                        serde_json::to_vec(&parsed)? == bytes,
                        "texture schema changed at {}",
                        entry.path().display()
                    );
                    count += 1;
                }
            }
        }
        ensure!(count > 1000, "incomplete original texture corpus");
        eprintln!("{count} physical texture catalogues roundtrip byte-for-byte");
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "texture/tests.rs"]
mod encode_tests;
