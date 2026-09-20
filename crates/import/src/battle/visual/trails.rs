//! Blade ribbons use image zero of the battle atlas, with palette variants resolved once.
use crate::{compression, digest, read::u16 as half, tpl};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle::{effect_program::Blend, visual::TrailStyle},
    font::UiTexture,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub(super) enum Recipe<'a> {
    PartyWeapon(&'a [u8]),
    EnemyWeapon(&'a [u8]),
    EnemyBody(&'a [u8]),
}

/// The caller resolves actor-dependent texture bindings to an identified TPL resource.
pub(super) struct Atlas<'a> {
    pub kind: i8,
    pub key: &'a str,
    pub bytes: &'a [u8],
}

pub(super) struct Cooker {
    common: [Vec<u8>; 2],
    output: PathBuf,
    sources: BTreeMap<String, String>,
    textures: BTreeMap<(String, u8), (UiTexture, PathBuf)>,
}

impl Cooker {
    pub fn new(usual: &[u8], output: &Path) -> Result<Self> {
        let member = super::super::actions::member;
        let bank = member(usual, 4)?;
        Ok(Self {
            common: [
                compression::decode(member(bank, 1)?)?,
                member(bank, 4)?.to_vec(),
            ],
            output: output.into(),
            sources: BTreeMap::new(),
            textures: BTreeMap::new(),
        })
    }

    pub fn set_output(&mut self, output: &Path) {
        self.output = output.into();
    }

    pub fn cook(&mut self, recipe: Recipe<'_>, context: Option<Atlas<'_>>) -> Result<TrailStyle> {
        let (data, color, uv, kind, dynamic) = match recipe {
            Recipe::PartyWeapon(data) => (data, 0x10, 0x14, 0x0d, true),
            Recipe::EnemyWeapon(data) => (data, 0x10, 0x14, 0x0d, false),
            Recipe::EnemyBody(data) => (data, 0x10c, 0x110, 0x118, false),
        };
        let texture = *data.get(kind).context("truncated trail texture binding")? as i8;
        let palette = *data.get(kind + 1).context("truncated trail palette")?;
        let flags = *data.get(kind + 2).context("truncated trail render flags")?;
        ensure!(
            flags & !3 == 0,
            "unsupported trail depth or culling flags {flags:#x}"
        );
        let mut style = TrailStyle {
            textures: BTreeMap::new(),
            palette,
            rgb: data
                .get(color..color + 3)
                .context("truncated trail color")?
                .try_into()?,
            uv: [
                half(data, uv)? as i16,
                half(data, uv + 2)? as i16,
                half(data, uv + 4)? as i16,
                half(data, uv + 6)? as i16,
            ],
            blend: match flags & 3 {
                0 => Blend::Alpha,
                2 => Blend::Subtractive,
                _ => Blend::Additive,
            },
        };
        let mut palettes = BTreeSet::from([palette]);
        if dynamic {
            palettes.extend([1, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        }
        if texture == -1 {
            style.textures = palettes.into_iter().map(|p| (p, None)).collect();
        } else {
            let (key, bytes) = match texture {
                0 => ("common-0", self.common[0].as_slice()),
                1 => ("common-1", self.common[1].as_slice()),
                _ => {
                    let atlas = context.context("trail requires an actor texture binding")?;
                    ensure!(
                        atlas.kind == texture,
                        "trail actor texture binding has the wrong kind"
                    );
                    (atlas.key, atlas.bytes)
                }
            };
            ensure!(
                !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                "invalid trail atlas key"
            );
            let source = digest(bytes);
            if let Some(previous) = self.sources.insert(key.to_owned(), source.clone()) {
                ensure!(
                    previous == source,
                    "trail atlas key {key} names different source resources"
                );
            }
            let image = tpl::parse_tpl(bytes)?
                .into_iter()
                .next()
                .context("empty trail atlas")?;
            for palette in palettes {
                let cache_key = (key.to_owned(), palette);
                let texture = if let Some((texture, source)) = self.textures.get(&cache_key) {
                    let destination = self.output.join(&texture.path);
                    if &destination != source {
                        share_texture(source, &destination)?;
                    }
                    texture.clone()
                } else {
                    let texture = cook_palette(bytes, &image, key, palette, &self.output)?;
                    self.textures.insert(
                        cache_key,
                        (texture.clone(), self.output.join(&texture.path)),
                    );
                    texture
                };
                style.textures.insert(palette, Some(texture));
            }
        }
        style.validate()?;
        Ok(style)
    }
}

fn share_texture(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination.parent().context("texture has no parent")?)?;
    let temporary = loop {
        let path = crate::temporary_path(destination);
        match fs::hard_link(source, &path) {
            Ok(()) => break path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("share trail texture"),
        }
    };
    let result = fs::rename(&temporary, destination);
    // Renaming two links to the same inode succeeds without removing either.
    if temporary.exists() {
        fs::remove_file(temporary)?;
    }
    result.context("publish shared trail texture")
}

#[test]
fn repeated_texture_publication_replaces_stale_data_without_leaving_links() -> Result<()> {
    let root = crate::temporary_path(&std::env::temp_dir().join("shared-trail-texture"));
    fs::create_dir_all(&root)?;
    let result = (|| -> Result<()> {
        let source = root.join("source.ktx2");
        let destination = root.join("target.ktx2");
        fs::write(&source, b"current")?;
        fs::write(&destination, b"stale")?;
        for _ in 0..3 {
            share_texture(&source, &destination)?;
            assert_eq!(fs::read(&destination)?, b"current");
            assert_eq!(fs::read_dir(&root)?.count(), 2);
        }
        Ok(())
    })();
    fs::remove_dir_all(root)?;
    result
}

fn cook_palette(
    bytes: &[u8],
    image: &tpl::TplTexture,
    key: &str,
    palette: u8,
    output: &Path,
) -> Result<UiTexture> {
    let mut image = image.clone();
    let stride = if image.format == 9 { 256 } else { 16 };
    let skip = usize::from(palette) * stride;
    image.palette_offset =
        Some(image.palette_offset.context("trail atlas has no palette")? + skip * 2);
    image.palette_entries = image
        .palette_entries
        .checked_sub(skip)
        .filter(|&count| count > 0)
        .context("trail palette exceeds atlas")?;
    let rgba = tpl::decode_texture(bytes, &image)?;
    let png = crate::temporary_path(
        &output.join(format!("intermediate/battle/trails/{key}-{palette}.png")),
    );
    let path = format!("battle/trails/{key}-{palette}.ktx2");
    fs::create_dir_all(png.parent().unwrap())?;
    fs::create_dir_all(output.join("battle/trails"))?;
    let width = u32::from(image.width);
    let height = u32::from(image.height);
    image::save_buffer(&png, &rgba, width, height, image::ColorType::Rgba8)?;
    crate::texture::cook_png(&png, &output.join(&path))?;
    fs::remove_file(png)?;
    Ok(UiTexture {
        path,
        width,
        height,
    })
}
