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
            .map(|image| image.path.clone()),
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
            let descriptor = dol::u32(&tpl, dol::u32(&tpl, 8)? as usize + index * 8)? as usize;
            let sampler = world_texture::sampler(&tpl, descriptor)?;
            let (ktx, size) = world_texture::tpl_interface_texture(&tpl, index, sampler.wrap)?;
            let path = if source == "change_disc.tpl" && index == 3 {
                let path = "ui/title/lloyd-in-a-field.ktx2".to_owned();
                ensure!(
                    library.join(&path).is_file(),
                    "missing shared artwork {path}"
                );
                path
            } else {
                let path = format!("{directory}/background-{}.ktx2", index + 1);
                crate::output::write(&library.join(&path), &ktx)?;
                path
            };
            catalogue.textures.push(Some(TextureImage {
                sampler,
                images: vec![path],
                size: Some(size),
            }));
        }
        banks.insert(name.to_owned(), catalogue);
    }
    Ok(banks)
}

/// Publish only the additional artwork into an existing review library.
pub fn cook_extra_artwork(extracted: &Path, library: &Path) -> Result<()> {
    let mut banks: BTreeMap<String, TextureCatalogue> = serde_json::from_slice(
        &crate::authoring::read_document(library, "textures/banks.json")?,
    )?;
    banks.extend(extra_artwork(extracted, library)?);
    crate::authoring::publish_document(
        library,
        "textures/banks.json",
        &serde_json::to_value(&banks)?,
    )
}

/// Run after the ordinary artwork owners, reusing their physical images.
pub fn cook_shared_texture_banks(extracted: &Path, library: &Path) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let files = extracted.join("disc1/files");
    let available = fs::read_dir(&files)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().to_ascii_lowercase()))
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    let portraits: Portraits =
        serde_json::from_slice(&crate::authoring::read_document(library, portraits::PATH)?)?;
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
            let (path, write_image) = if let Some(path) = unique.get(&key) {
                (path.clone(), false)
            } else {
                let owner = image_owner(id, index, &portraits);
                let owns_image = owner.is_none();
                let path = owner.unwrap_or_else(|| match (id, index) {
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
                let write_image = owns_image || !library.join(&path).is_file();
                unique.insert(key, path.clone());
                (path, write_image)
            };
            let sampler = world_texture::sampler(
                &tpl,
                dol::u32(&tpl, dol::u32(&tpl, 8)? as usize + index * 8)? as usize,
            )?;
            // Reuse other categories' artwork and images emitted in this pass.
            // Bank-owned images always replace output from an earlier cook.
            let (ktx, size) = world_texture::tpl_interface_texture(&tpl, index, sampler.wrap)
                .with_context(|| path.clone())?;
            if write_image {
                crate::output::write(&library.join(&path), &ktx)?;
            }
            catalogue.textures.push(Some(TextureImage {
                sampler,
                images: vec![path],
                size: Some(size),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn word(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn image(red: u8) -> Vec<u8> {
        let mut tpl = vec![0; 128];
        for (at, value) in [
            (0, 0x0020_af30),
            (4, 1),
            (8, 12),
            (12, 20),
            (24, 6),
            (28, 64),
        ] {
            word(&mut tpl, at, value);
        }
        // One clamped 4x4 RGBA8 image in the console's tiled channel layout.
        tpl[20..24].copy_from_slice(&[0, 4, 0, 4]);
        for pixel in 0..16 {
            tpl[64 + pixel * 2] = if pixel % 3 == 0 { 0 } else { 255 };
            tpl[65 + pixel * 2] = red;
            tpl[96 + pixel * 2] = 80;
            tpl[97 + pixel * 2] = 40;
        }
        tpl
    }

    #[test]
    fn recook_overwrites_bank_images_and_preserves_shared_artwork() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let extracted = directory.path().join("extracted");
        let files = extracted.join("disc1/files");
        let library = directory.path().join("library");
        fs::create_dir_all(&files)?;
        fs::create_dir_all(extracted.join("disc1/sys"))?;
        fs::create_dir_all(library.join("skits"))?;
        fs::write(
            library.join(portraits::PATH),
            serde_json::to_vec(&Portraits::default())?,
        )?;

        // A minimal executable names two identical bank-owned images and one
        // image supplied by the ordinary artwork categories.
        let base = 0x801f_8000;
        let mut exe = vec![0; 0x1100];
        word(&mut exe, 0, 0x100);
        word(&mut exe, 0x48, base);
        word(&mut exe, 0x90, 0x1000);
        for (pointer, address, name) in [
            (0x801f_85e4, 0x801f_8800, "owned.tpl"),
            (0x801f_85e8, 0x801f_8820, "duplicate.tpl"),
            (0x801f_8674, 0x801f_8840, "shared.tpl"),
            (0x801f_86b8, 0x801f_8860, "group1.bin"),
            (0x801f_8748, 0x801f_8880, "group13.bin"),
        ] {
            word(&mut exe, 0x100 + (pointer - base) as usize, address);
            let at = 0x100 + (address - base) as usize;
            exe[at..at + name.len()].copy_from_slice(name.as_bytes());
        }
        fs::write(extracted.join("disc1/sys/main.dol"), exe)?;
        for name in ["owned.tpl", "duplicate.tpl"] {
            fs::write(files.join(name), image(200))?;
        }
        fs::write(files.join("shared.tpl"), image(100))?;
        for name in ["group1.bin", "group13.bin"] {
            fs::write(files.join(name), 0u32.to_be_bytes())?;
        }
        for name in ["change_disc.tpl", "end_roll_wall.tpl"] {
            let mut tpl = vec![0; 12];
            word(&mut tpl, 0, 0x0020_af30);
            word(&mut tpl, 8, 12);
            fs::write(files.join(name), tpl)?;
        }
        let shared = library.join("ui/location-titles/iselia-japanese-caption.ktx2");
        crate::output::write(&shared, b"artwork from another category")?;
        let cook = || {
            crate::authoring::cook_incremental(&library, || {
                cook_shared_texture_banks(&extracted, &library)
            })
        };
        cook()?;
        let texture = library.join("textures/unidentified-bank-0-image-0.ktx2");
        let expected = fs::read(&texture)?;
        assert!(resonance_asset_writer::gpu::is_native_compressed(
            &expected
        )?);
        let banks: BTreeMap<String, TextureCatalogue> =
            crate::authoring::read_json(&library, "textures/banks.json")?;
        assert_eq!(
            banks["unidentified-bank-0"].textures[0]
                .as_ref()
                .unwrap()
                .images,
            banks["unidentified-bank-1"].textures[0]
                .as_ref()
                .unwrap()
                .images
        );
        fs::write(&texture, b"stale output")?;
        cook()?;
        assert_eq!(fs::read(texture)?, expected);
        assert_eq!(fs::read(shared)?, b"artwork from another category");
        Ok(())
    }
}
