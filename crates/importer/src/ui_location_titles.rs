//! Field-local location title artwork as named editable KTX2 images.
use crate::{dol, menu_sprites, ui_controls};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs, path::Path};

// Native title handles address field section 16 plus the low handle number.
const TITLES: [(usize, usize, &str); 24] = [
    (11, 22, "altamira"),
    (138, 18, "altessas-house"),
    (29, 16, "asgard"),
    (128, 16, "derris-kharlan"),
    (372, 20, "dirks-house"),
    (189, 16, "exire"),
    (310, 16, "flanoir"),
    (123, 16, "hakonesia-peak"),
    (282, 16, "heimdall"),
    (300, 16, "hima"),
    (332, 17, "iselia"),
    (353, 16, "izoold"),
    (419, 16, "katz-village"),
    (447, 20, "luin-devastation"),
    (452, 16, "luin-hope"),
    (447, 21, "luin-rebirth"),
    (376, 17, "meltokio"),
    (326, 17, "mizuho"),
    (425, 17, "ozette"),
    (55, 18, "palmacosta"),
    (434, 16, "sybak"),
    (82, 16, "thoda-island-boat-dock"),
    (485, 16, "triet"),
    (501, 24, "welgaia"),
];

/// Resolve a field-local title handle to the one named artwork owner.
pub(crate) fn title_path(field: u32, section: usize) -> Option<String> {
    let shared = match (field, section) {
        (190, 16) => Some("exire"),
        (448, 17) => Some("luin-devastation"),
        (448, 18) => Some("luin-rebirth"),
        (453, 16) => Some("luin-hope"),
        _ => None,
    };
    let name = shared.or_else(|| {
        TITLES
            .iter()
            .find(|&&(owner, slot, _)| owner == field as usize && slot == section)
            .map(|&(_, _, name)| name)
    })?;
    Some(format!("ui/location-titles/{name}.json"))
}

fn image_names(name: &str) -> [String; 4] {
    let subtitle = match name {
        "thoda-island-boat-dock" => "thoda-island".to_owned(),
        _ => format!("{name}-subtitle"),
    };
    let title = match name {
        "luin-devastation" | "luin-hope" | "luin-rebirth" => "luin",
        "thoda-island-boat-dock" => "sightseeing-boat-dock",
        _ => name,
    };
    [
        "frame-line".to_owned(),
        "frame-ornaments".to_owned(),
        subtitle,
        title.to_owned(),
    ]
}

fn cook_title(
    extracted: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    field: usize,
    section: usize,
    name: &str,
) -> Result<()> {
    let archive = ui_controls::field_archive(extracted, field)?;
    let tpl = ui_controls::local_texture(&archive, section)?;
    ensure!(
        dol::u32(&tpl, 4)? >= 4,
        "location title {name} has too few images"
    );
    let names = image_names(name);
    let mut document = Vec::new();
    for (index, image) in names.iter().enumerate() {
        let (width, height, rgba) = menu_sprites::decode_image(&tpl, index)
            .with_context(|| format!("location title {name} image {index}"))?;
        if index == 2 && width <= 8 && rgba.chunks_exact(4).all(|pixel| pixel[3] == 0) {
            document.push(None);
            continue;
        }
        let path = format!("ui/location-titles/{image}.ktx2");
        let encoded = resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?;
        if let Some(existing) = files.get(&path) {
            ensure!(
                *existing == encoded,
                "conflicting location title image {path}"
            );
        } else {
            files.insert(path.clone(), encoded);
        }
        document.push(Some(path));
    }
    files.insert(
        format!("ui/location-titles/{name}.json"),
        resonance_asset_writer::json::encode(&document)?,
    );
    Ok(())
}

fn japanese_iselia(extracted: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
    // Standalone resource 38 names pic002.tpl in the GameCube executable.
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let pointer = dol::u32(dol::slice(&exe, 0x801f85e4 + 38 * 4, 4)?, 0)?;
    let source = dol::slice(&exe, pointer, 64)?;
    let end = source
        .iter()
        .position(|&byte| byte == 0)
        .context("resource 38 path")?;
    let source = std::str::from_utf8(&source[..end])?;
    let tpl = fs::read(extracted.join("disc1/files").join(source))?;
    ensure!(
        dol::u32(&tpl, 4)? == 1,
        "Japanese Iselia image count differs"
    );
    let (width, height, rgba) = menu_sprites::decode_image(&tpl, 0)?;
    files.insert(
        "ui/location-titles/iselia-japanese.ktx2".to_owned(),
        resonance_asset_writer::ktx2::encode_rgba8(width, height, &rgba)?,
    );
    Ok(())
}

/// `selected` converts one named title for a focused check.
pub fn cook_ui_location_titles(
    extracted: &Path,
    library: &Path,
    selected: Option<&str>,
) -> Result<()> {
    let mut files = BTreeMap::new();
    let mut found = false;
    for (field, section, name) in TITLES {
        if selected.is_some_and(|wanted| wanted != name) {
            continue;
        }
        found = true;
        cook_title(extracted, &mut files, field, section, name)?;
    }
    if selected.is_none_or(|wanted| wanted == "iselia-japanese") {
        found = true;
        japanese_iselia(extracted, &mut files)?;
    }
    ensure!(found, "unknown location title");
    for (path, bytes) in files {
        crate::output::write(&library.join(path), &bytes)?;
    }
    Ok(())
}
