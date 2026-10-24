//! Physical texture metadata and the shared lossless KTX2 cooking profile.
#[cfg(test)]
use anyhow::ensure;
use anyhow::{Context, Result};
use resonance_asset_writer::ktx2::encode_rgba8;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::io::Read;
use std::path::Path;

use crate::tpl::FormatDecode;
pub(crate) use resonance_content::physical_scene::{
    TextureCatalogue as Catalogue, TextureImage as Texture,
};

/// Decoded pixels remain typed inputs until a terminal writer encodes them.
pub(crate) struct Decoded {
    pub source_sha256: String,
    pub catalogue: Catalogue,
    images: Vec<Image>,
}

struct Image {
    name: String,
    size: [u32; 2],
    levels: Result<Vec<Vec<u8>>>,
    published: Option<crate::publication::File>,
}

pub(crate) fn decode(bytes: &[u8], name: &str) -> Result<Decoded> {
    decode_with_palette_step(bytes, name, 0)
}

fn decode_with_palette_step(bytes: &[u8], name: &str, page_step: u16) -> Result<Decoded> {
    let mut images = Vec::new();
    let catalogue = decode_pages(bytes, name, page_step, |image| images.push(image))?;
    Ok(Decoded {
        source_sha256: crate::digest(bytes),
        catalogue,
        images,
    })
}

/// Original TPL banks and their CAB wrappers share the uncompressed bank identity.
pub(crate) fn decode_source(bytes: &[u8]) -> Result<Decoded> {
    decode_source_with_palette_step(bytes, 0)
}

/// Publish an explicitly selected effect palette without changing its index width.
pub(crate) fn publish_palette_window(
    bytes: &[u8],
    texture: usize,
    first_color: usize,
    output: &Path,
) -> Result<resonance_content::font::UiTexture> {
    let descriptor = crate::tpl::parse_tpl(bytes)?
        .get(texture)
        .context("missing effect palette texture")?
        .clone();
    let name = format!(
        "textures/{}/texture-{texture}-palette-offset-{first_color}",
        crate::digest(bytes)
    );
    let size = [u32::from(descriptor.width), u32::from(descriptor.height)];
    Image {
        name: name.clone(),
        size,
        levels: descriptor
            .levels(bytes)?
            .iter()
            .map(|level| {
                Ok(crate::tpl::decode_palette_window(
                    bytes,
                    level,
                    first_color,
                )?)
            })
            .collect(),
        published: None,
    }
    .publish(output)?;
    Ok(resonance_content::font::UiTexture {
        path: format!("{name}.ktx2"),
        width: size[0],
        height: size[1],
    })
}

/// Effect banks can select overlapping TLUT windows within a shared palette.
pub(crate) fn decode_source_with_palette_step(bytes: &[u8], page_step: u16) -> Result<Decoded> {
    let expanded;
    let bytes = if bytes.starts_with(b"MSCF") {
        expanded = crate::compression::cabinet(bytes)?.1;
        &expanded
    } else {
        bytes
    };
    let decoded = decode_with_palette_step(
        bytes,
        &format!("textures/{}", crate::digest(bytes)),
        page_step,
    )?;
    decoded.validate()?;
    Ok(decoded)
}

fn decode_pages(
    bytes: &[u8],
    name: &str,
    page_step: u16,
    mut emit: impl FnMut(Image),
) -> Result<Catalogue> {
    let mut catalogue = Catalogue {
        textures: Vec::new(),
    };
    for (index, texture) in crate::tpl::parse_tpl(bytes)?.into_iter().enumerate() {
        let stride = match texture.format {
            8 => 16,
            9 => 256,
            10 => 16384,
            _ => 0,
        };
        let step = palette_page_step(stride, page_step);
        let overlapping = step != stride;
        let pages = if stride == 0 {
            1
        } else if overlapping {
            // Publish complete windows within the declared source palette.
            // Do not fill missing colors or alias a neighboring palette page.
            texture.palette_entries.saturating_sub(stride) / step + 1
        } else {
            texture.palette_entries.div_ceil(stride).max(1)
        };
        let metadata = (|| -> Result<_> {
            Ok((
                Texture {
                    dimensions: [texture.width, texture.height],
                    format: crate::tpl::Format::from_code(texture.format)?,
                    sampler: texture.sampler()?,
                    palette: crate::tpl::palette(bytes, &texture)?,
                    images: Vec::new(),
                },
                texture.levels(bytes)?,
            ))
        })();
        let size = [u32::from(texture.width), u32::from(texture.height)];
        let (mut metadata, levels) = match metadata {
            Ok(value) => value,
            Err(error) => {
                emit(Image {
                    name: format!("{name}/texture-{index}"),
                    size,
                    levels: Err(error),
                    published: None,
                });
                catalogue.textures.push(None);
                continue;
            }
        };
        for page in 0..pages {
            let name = if stride == 0 {
                format!("{name}/texture-{index}")
            } else if overlapping {
                format!("{name}/texture-{index}-palette-offset-{}", page * step)
            } else {
                format!("{name}/texture-{index}-palette-{page}")
            };
            metadata.images.push(format!("{name}.ktx2"));
            let levels = levels
                .iter()
                .cloned()
                .map(|mut image| {
                    if overlapping {
                        return Ok(crate::tpl::decode_palette_window(
                            bytes,
                            &image,
                            page * step,
                        )?);
                    }
                    if stride != 0 {
                        image.palette_offset = Some(
                            image
                                .palette_offset
                                .context("indexed texture has no palette")?
                                + page * stride * 2,
                        );
                        image.palette_entries = image
                            .palette_entries
                            .saturating_sub(page * stride)
                            .min(stride);
                    }
                    Ok(crate::tpl::decode_texture(bytes, &image)?)
                })
                .collect();
            emit(Image {
                name,
                size,
                levels,
                published: None,
            });
        }
        catalogue.textures.push(Some(metadata));
    }
    Ok(catalogue)
}

/// A zero authored stride leaves ordinary format-sized publication unchanged.
pub(crate) fn palette_page_step(capacity: usize, authored: u16) -> usize {
    if capacity == 0 || authored == 0 {
        return capacity;
    }
    let (mut a, mut b) = (capacity, usize::from(authored));
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Decoded {
    /// Select original pixels and all authored mip levels for a terminal writer.
    pub(crate) fn image_levels(
        &self,
        index: usize,
        page: usize,
    ) -> Result<Vec<resonance_asset_writer::texture::Pixels>> {
        let path = self
            .catalogue
            .textures
            .get(index)
            .and_then(Option::as_ref)
            .context("missing decoded texture")?
            .images
            .get(page)
            .context("missing decoded palette page")?;
        let image = self
            .images
            .iter()
            .find(|image| path == &format!("{}.ktx2", image.name))
            .context("missing decoded image")?;
        let levels = image
            .levels
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        Ok(levels
            .iter()
            .enumerate()
            .map(|(index, rgba)| resonance_asset_writer::texture::Pixels {
                width: (image.size[0] >> index).max(1),
                height: (image.size[1] >> index).max(1),
                rgba: rgba.clone(),
            })
            .collect())
    }

    pub(crate) fn base_images(&self) -> impl Iterator<Item = Result<([u32; 2], &[u8])>> {
        let mut images = self.images.iter();
        self.catalogue.textures.iter().map(move |texture| {
            let texture = texture.as_ref().context("incomplete decoded texture")?;
            let image = images.next().context("missing base texture image")?;
            for _ in 1..texture.images.len() {
                images.next().context("missing texture palette image")?;
            }
            let levels = image
                .levels
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{error:#}"))?;
            Ok((
                image.size,
                levels
                    .first()
                    .context("missing base texture level")?
                    .as_slice(),
            ))
        })
    }

    pub(crate) fn base_alpha(&self) -> Result<Vec<bool>> {
        self.base_images()
            .map(|image| Ok(image?.1.chunks_exact(4).any(|pixel| pixel[3] != 255)))
            .collect()
    }

    /// Terminal publication; downstream computations should use the typed catalogue.
    pub(crate) fn write(self, output: &Path) -> Result<Catalogue> {
        self.validate()?;
        for image in &self.images {
            image.publish(output)?;
        }
        Ok(self.catalogue)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        for image in &self.images {
            image
                .levels
                .as_ref()
                .map_err(|error| anyhow::anyhow!("{}: {error:#}", image.name))?;
        }
        Ok(())
    }

    pub(crate) fn publish(&self, output: &Path, mut report: impl FnMut(&str, Result<()>)) {
        for image in &self.images {
            report(&image.name, image.publish(output));
        }
    }
}

impl Image {
    fn publish(&self, output: &Path) -> Result<()> {
        self.publish_at(&output.join(format!("{}.ktx2", self.name)))
            .map(|_| ())
    }

    fn publish_at(&self, path: &Path) -> Result<crate::publication::File> {
        if let Some(published) = &self.published {
            return published.share(path);
        }
        let levels = self
            .levels
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        crate::publication::File::write(
            path,
            &resonance_asset_writer::ktx2::encode_levels(
                self.size[0],
                self.size[1],
                &levels.iter().map(Vec::as_slice).collect::<Vec<_>>(),
            )?,
        )
    }
}

/// Read the base image of our lossless cooking profile without re-decoding its source.
#[cfg(test)]
pub(crate) fn pixels(path: &Path) -> Result<image::RgbaImage> {
    if path.extension().is_some_and(|extension| extension == "png") {
        let pixels = resonance_asset_writer::texture::png(&fs::read(path)?)?;
        return image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba)
            .context("invalid PNG dimensions");
    }
    decode_pixels(
        &fs::read(path).with_context(|| format!("reading cooked image {}", path.display()))?,
    )
    .with_context(|| format!("invalid cooked image {}", path.display()))
}

#[cfg(test)]
pub(crate) fn decode_pixels(bytes: &[u8]) -> Result<image::RgbaImage> {
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

pub(crate) fn cook(width: u32, height: u32, pixels: &[u8], output: &Path) -> Result<()> {
    let bytes = encode_rgba8(width, height, pixels)
        .with_context(|| format!("encode texture {}", output.display()))?;
    crate::write_atomic(output, &bytes)
}

#[cfg(test)]
pub(crate) fn image_levels(
    root: &Path,
    path: &str,
) -> Result<(
    resonance_content::texture::ColorSpace,
    Vec<resonance_asset_writer::texture::Pixels>,
)> {
    use resonance_asset_writer::texture;
    use resonance_content::texture::{ColorSpace, Definition};
    let bytes = fs::read(root.join(path))?;
    if path.ends_with(".texture.json") {
        let definition: Definition = serde_json::from_slice(&bytes)?;
        return Ok((
            definition.color_space,
            definition.decode(|image| Ok(fs::read(root.join(image))?))?,
        ));
    }
    let levels = if path.ends_with(".png") {
        vec![texture::png(&bytes)?]
    } else {
        ensure!(path.ends_with(".ktx2"), "unsupported image fixture {path}");
        texture::ktx2(&bytes)?
    };
    Ok((ColorSpace::Linear, levels))
}

#[cfg(test)]
pub(crate) fn image_pixels(root: &Path, path: &str) -> Result<image::RgbaImage> {
    let (_, mut levels) = image_levels(root, path)?;
    let base = levels.remove(0);
    image::RgbaImage::from_raw(base.width, base.height, base.rgba)
        .context("invalid image dimensions")
}

#[cfg(test)]
pub(crate) fn compare_images(
    actual_root: &Path,
    actual: &str,
    expected_root: &Path,
    expected: &str,
) -> Result<()> {
    ensure!(
        image_levels(actual_root, actual)? == image_levels(expected_root, expected)?,
        "texture color space, dimensions, mip count or pixels changed: {actual} / {expected}"
    );
    Ok(())
}

#[cfg(test)]
fn compare_encoded(actual: &[u8], expected: &[u8]) -> Result<()> {
    let actual = ktx2::Reader::new(actual)?;
    let expected = ktx2::Reader::new(expected)?;
    let header = actual.header();
    let mut other = expected.header();
    other.index = header.index; // Container offsets change with metadata and compression.
    ensure!(
        header == other,
        "texture format, dimensions, or mip count changed"
    );
    ensure!(
        actual.dfd_blocks() == expected.dfd_blocks(),
        "texture color metadata changed"
    );
    ensure!(
        actual.supercompression_global_data() == expected.supercompression_global_data(),
        "texture compression metadata changed"
    );
    for reader in [&actual, &expected] {
        let parsed: usize = reader
            .key_value_data()
            .map(|(key, value)| (4 + key.len() + 1 + value.len()).next_multiple_of(4))
            .sum();
        ensure!(
            parsed == reader.header().index.kvd_byte_length as usize,
            "malformed texture metadata"
        );
    }
    let semantic = |key: &str| !matches!(key, "KTXwriter" | "KTXwriterScParams");
    ensure!(
        actual
            .key_value_data()
            .filter(|(key, _)| semantic(key))
            .eq(expected.key_value_data().filter(|(key, _)| semantic(key))),
        "texture orientation, swizzle, or other metadata changed"
    );
    let pixels = |level: ktx2::Level<'_>| -> Result<Vec<u8>> {
        let bytes = match header.supercompression_scheme {
            None => level.data.to_vec(),
            Some(ktx2::SupercompressionScheme::Zstandard) => {
                let mut decoder = ruzstd::decoding::StreamingDecoder::new(level.data)?;
                let mut bytes = Vec::new();
                (&mut decoder)
                    .take(level.uncompressed_byte_length + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(decoder.into_inner().is_empty(), "trailing mip data");
                bytes
            }
            _ => anyhow::bail!("unsupported texture compression"),
        };
        ensure!(
            bytes.len() as u64 == level.uncompressed_byte_length,
            "invalid mip length"
        );
        Ok(bytes)
    };
    for (index, (actual, expected)) in actual.levels().zip(expected.levels()).enumerate() {
        ensure!(
            pixels(actual)? == pixels(expected)?,
            "mip {index} pixels changed"
        );
    }
    Ok(())
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
    fn frozen_comparison_checks_every_mip_and_color_metadata() -> Result<()> {
        let pixels = [vec![17; 32], vec![29; 8], vec![43; 4]];
        let encode = |pixels: &[Vec<u8>]| {
            resonance_asset_writer::ktx2::encode_levels(
                4,
                2,
                &pixels.iter().map(Vec::as_slice).collect::<Vec<_>>(),
            )
        };
        let original = encode(&pixels)?;
        compare_encoded(&original, &original)?;
        let mut changed = pixels.clone();
        changed[2][0] += 1;
        assert!(compare_encoded(&original, &encode(&changed)?).is_err());
        let mut changed = original.clone();
        let descriptor = ktx2::Reader::new(&changed)?.header().index.dfd_byte_offset as usize;
        changed[descriptor + 14] ^= 1; // Transfer function, before sample information.
        assert!(compare_encoded(&original, &changed).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires extracted discs and current RESONANCE_TEST_ASSETS; title and toon pixels"]
    fn original_cooked_pixels_match_source_tpls() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .context("set RESONANCE_TEST_ASSETS")?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let reports = std::env::var_os("RESONANCE_TEST_IMPORT_REPORTS")
            .map(std::path::PathBuf::from)
            .context("set RESONANCE_TEST_IMPORT_REPORTS to the external reports directory")?;
        let sources: std::collections::BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&fs::read(reports.join("import-sources.json"))?)?;
        let output = tempfile::tempdir()?;
        let mut count = 0;
        for disc in [1, 2] {
            for path in ["title.tpl", "toon.tpl"] {
                let source = fs::read(local.join(format!("extracted/disc{disc}/files/{path}")))?;
                let original = crate::tpl::parse_tpl(&source)?;
                let outputs = sources
                    .get(&format!("disc{disc}/{path}"))
                    .context("missing current texture provenance")?;
                let paths = outputs
                    .iter()
                    .filter(|path| {
                        path.starts_with("resources/physical/") && path.ends_with("/textures.json")
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    paths.len() == 1,
                    "expected one physical texture bank for {path}"
                );
                let mut snapshot = library.snapshot();
                let catalogue: Catalogue = snapshot.json(paths[0], 64 * 1024 * 1024)?;
                catalogue.validate()?;
                let cooked = catalogue
                    .textures
                    .into_iter()
                    .enumerate()
                    .map(|(index, texture)| {
                        texture.with_context(|| format!("missing original texture {index}"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let fresh = decode_source(&source)?
                    .write(output.path())?
                    .textures
                    .into_iter()
                    .map(|texture| texture.context("native texture was not decoded"))
                    .collect::<Result<Vec<_>>>()?;
                assert_eq!(original.len(), cooked.len());
                assert_eq!(fresh.len(), cooked.len());
                for ((texture, cooked), fresh) in original.iter().zip(cooked).zip(fresh) {
                    assert_eq!(fresh.images.len(), cooked.images.len());
                    let stride = match texture.format {
                        8 => 16,
                        9 => 256,
                        10 => 16384,
                        _ => 0,
                    };
                    for (page, image) in cooked.images.iter().enumerate() {
                        compare_images(output.path(), &fresh.images[page], &root, image)?;
                        let mut texture = texture.clone();
                        if stride != 0 {
                            texture.palette_offset =
                                texture.palette_offset.map(|at| at + page * stride * 2);
                            texture.palette_entries = texture
                                .palette_entries
                                .saturating_sub(page * stride)
                                .min(stride);
                        }
                        let pixels = image_pixels(&root, image)?;
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
        assert!(!output.path().join("intermediate").exists());
        eprintln!("{count} cooked base images exactly match original TPL pixels");
        Ok(())
    }

    #[test]
    #[ignore = "requires complete current RESONANCE_TEST_ASSETS; physical texture schema check"]
    fn original_texture_catalogues_roundtrip_without_schema_changes() -> Result<()> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .context("set RESONANCE_TEST_ASSETS")?;
        let _library = resonance_content::library::Library::open(&root, &[])?;
        let root = root.join("resources/physical");
        let mut pending = vec![root];
        let mut count = 0;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    pending.push(entry.path());
                } else if entry.file_name() == "textures.json" {
                    let bytes = fs::read(entry.path())?;
                    let parsed: Catalogue = serde_json::from_slice(&bytes)?;
                    parsed.validate()?;
                    ensure!(
                        serde_json::to_value(&parsed)?
                            == serde_json::from_slice::<serde_json::Value>(&bytes)?,
                        "texture schema changed at {}",
                        entry.path().display()
                    );
                    count += 1;
                }
            }
        }
        ensure!(count > 1000, "incomplete original texture corpus");
        eprintln!("{count} physical texture catalogues retain every current schema field");
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "texture/tests.rs"]
mod encode_tests;
