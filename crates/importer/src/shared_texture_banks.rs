//! Original texture handles select named, editable image banks.
use crate::{dol, ending, menu_sprites, native_archive, text, title_screen, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    physical_scene::{TextureCatalogue, TextureImage},
    skit::document::portraits::{self, Portraits},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn image_owner(id: u32, image: usize, portraits: &Portraits) -> Option<String> {
    let fixed = match (id, image) {
        (35, 1) => Some("ui/title/continue-alternate.ktx2"),
        (35, 14) => Some("ui/game-over/background.ktx2"),
        (36 | 39, 0) => Some("ui/location-titles/iselia-japanese-caption.ktx2"),
        (37, 0) => Some("ui/location-titles/iselia-japanese-plaque.ktx2"),
        (38, 0) => Some("ui/location-titles/iselia-japanese.ktx2"),
        (49, 0) => Some("ui/ending/party-with-kratos.ktx2"),
        (50, 0) => Some("ui/ending/party-with-zelos.ktx2"),
        (68611, 0) => Some("effects/overlays/radial-burst.ktx2"),
        (68614, 0) => Some("effects/overlays/ornate-ring.ktx2"),
        (68864, 2) => Some("ui/location-titles/japanese/transparent-caption.ktx2"),
        (68864..=68872 | 68874..=68887, 0) => Some("ui/location-titles/frame-line.ktx2"),
        _ => None,
    };
    if let Some(path) = fixed {
        return Some(path.to_owned());
    }
    match id {
        35 => title_screen::IMAGES
            .iter()
            .find(|(index, _)| *index == image)
            .map(|(_, name)| format!("ui/title/{name}.ktx2")),
        51 => ending::FIN.get(image).map(|path| (*path).to_owned()),
        _ => portraits
            .mappings
            .get(&id)
            .and_then(|name| portraits.images.get(name))
            .and_then(|images| images.get(image))
            .cloned(),
    }
}

fn bank_name(id: u32, portraits: &Portraits) -> String {
    match id {
        35 => "title".into(),
        36 | 39 => "iselia-japanese-caption".into(),
        37 => "iselia-japanese-plaque".into(),
        38 => "iselia-japanese".into(),
        49 => "ending-with-kratos".into(),
        50 => "ending-with-zelos".into(),
        51 => "fin".into(),
        68611 => "radial-burst".into(),
        68614 => "ornate-ring".into(),
        68864..=68887 => format!("japanese-location-title-{:02}", id - 68863),
        _ => portraits
            .mappings
            .get(&id)
            .map(|name| format!("skit-portraits/{name}"))
            .unwrap_or_else(|| format!("unidentified-bank-{id}")),
    }
}

fn extra_artwork(extracted: &Path, library: &Path) -> Result<BTreeMap<String, TextureCatalogue>> {
    let mut banks = BTreeMap::new();
    for (source, name, directory) in [
        ("change_disc.tpl", "disc-change", "ui/disc-change"),
        ("end_roll_wall.tpl", "credits-backgrounds", "ui/credits"),
    ] {
        let tpl = native_archive::read_named(&extracted.join("disc1/files"), source)?;
        let mut catalogue = TextureCatalogue::default();
        for index in 0..dol::u32(&tpl, 4)? as usize {
            let path = if source == "change_disc.tpl" && index == 3 {
                let path = "ui/title/lloyd-in-a-field.ktx2".to_owned();
                ensure!(
                    library.join(&path).is_file(),
                    "missing shared artwork {path}"
                );
                path
            } else {
                let path = format!("{directory}/background-{}.ktx2", index + 1);
                crate::output::write(
                    &library.join(&path),
                    &world_texture::tpl_texture(&tpl, index)?.0,
                )?;
                path
            };
            catalogue.textures.push(Some(TextureImage {
                sampler: world_texture::sampler(
                    &tpl,
                    dol::u32(&tpl, dol::u32(&tpl, 8)? as usize + index * 8)? as usize,
                )?,
                images: vec![path],
            }));
        }
        banks.insert(name.to_owned(), catalogue);
    }
    Ok(banks)
}

/// Publish only the additional artwork into an existing review library.
pub fn cook_extra_artwork(extracted: &Path, library: &Path) -> Result<()> {
    let path = library.join("textures/banks.json");
    let mut banks: BTreeMap<String, TextureCatalogue> = serde_json::from_slice(&fs::read(&path)?)?;
    banks.extend(extra_artwork(extracted, library)?);
    fs::write(path, resonance_asset_writer::json::encode(&banks)?)?;
    Ok(())
}

/// Run after the ordinary artwork owners, reusing their physical images.
pub fn cook_shared_texture_banks(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let files = extracted.join("disc1/files");
    let available = fs::read_dir(&files)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().to_ascii_lowercase()))
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    let portraits: Portraits = serde_json::from_slice(&fs::read(library.join(portraits::PATH))?)?;
    let mut native = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for id in 0..53 {
        let pointer = dol::u32(dol::slice(&exe, 0x801f85e4 + id * 4, 4)?, 0)?;
        if pointer == 0 {
            continue;
        }
        let name = text::string(&exe, pointer)?;
        if name.to_ascii_lowercase().ends_with(".tpl")
            && available.contains(&name.to_ascii_lowercase())
        {
            native.insert(id, native_archive::read_named(&files, &name)?);
        }
    }
    for group in [1, 13] {
        let name = text::pointer(&exe, 0x801f86b8 + (group - 1) * 12)?;
        let bank = native_archive::read_named(&files, &name)?;
        let count = dol::u32(&bank, 0)? as usize;
        for index in 0..count {
            let start = dol::u32(&bank, 4 + index * 8)? as usize;
            let size = dol::u32(&bank, 8 + index * 8)? as usize;
            if size == 0 {
                if group == 13 {
                    aliases.insert(group << 16 | index as u32, group << 16);
                }
                continue;
            }
            let bytes = bank
                .get(start..start + size)
                .context("texture exceeds bank")?;
            if bytes.starts_with(&0x0020af30u32.to_be_bytes()) {
                native.insert(group << 16 | index as u32, bytes.to_vec());
            }
        }
    }
    let mut mappings = BTreeMap::new();
    let mut banks = BTreeMap::new();
    let mut unique = BTreeMap::<(u32, u32, Vec<u8>), String>::new();
    for (id, tpl) in native {
        ensure!(
            dol::u32(&tpl, 0)? == 0x0020af30,
            "invalid texture bank {id}"
        );
        let name = bank_name(id, &portraits);
        let mut catalogue = TextureCatalogue::default();
        for index in 0..dol::u32(&tpl, 4)? as usize {
            let (width, height, rgba) = menu_sprites::decode_image(&tpl, index)?;
            let key = (width, height, rgba);
            let path = if let Some(path) = unique.get(&key) {
                path.clone()
            } else {
                let path =
                    image_owner(id, index, &portraits).unwrap_or_else(|| match (id, index) {
                        (35, 16 | 17) => format!("ui/title/disc-{}.ktx2", index - 15),
                        (68864..=68887, 0) => format!(
                            "ui/location-titles/japanese/title-{:02}-frame-line.ktx2",
                            id - 68863
                        ),
                        (68864, 1) => "ui/location-titles/japanese/frame-ornaments.ktx2".into(),
                        (68864..=68887, 1) => format!(
                            "ui/location-titles/japanese/title-{:02}-frame-ornaments.ktx2",
                            id - 68863
                        ),
                        (68864..=68887, _) => format!(
                            "ui/location-titles/japanese/title-{:02}-glyph-{index:02}.ktx2",
                            id - 68863
                        ),
                        _ => format!("textures/{name}-image-{index}.ktx2"),
                    });
                if !library.join(&path).is_file() {
                    crate::output::write(
                        &library.join(&path),
                        &world_texture::tpl_texture(&tpl, index)?.0,
                    )?;
                }
                unique.insert(key, path.clone());
                path
            };
            catalogue.textures.push(Some(TextureImage {
                sampler: world_texture::sampler(
                    &tpl,
                    dol::u32(&tpl, dol::u32(&tpl, 8)? as usize + index * 8)? as usize,
                )?,
                images: vec![path],
            }));
        }
        mappings.insert(id, name.clone());
        banks.insert(name, catalogue);
    }
    for (id, target) in aliases {
        let name = mappings
            .get(&target)
            .context("portrait fallback bank missing")?
            .clone();
        mappings.insert(id, name);
    }
    banks.extend(extra_artwork(extracted, library)?);
    crate::output::json(&library.join("textures/mappings.json"), &mappings)?;
    crate::output::json(&library.join("textures/banks.json"), &banks)
}
