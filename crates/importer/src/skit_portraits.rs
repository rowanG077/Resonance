//! Native skit portrait archive and executable pose tracks into named KTX2.
use crate::{dol, text, world_texture};
use anyhow::{Context, Result, ensure};
use resonance_content::skit::{
    PortraitFrame, PortraitRecipe,
    document::portraits::{Animation, Portraits},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const GROUPS: u32 = 0x801f86b8;
const RECIPES: u32 = 0x8020f49c;
const RECIPE_COUNT: u16 = 230;

fn archive(root: &Path, exe: &[u8]) -> Result<Vec<u8>> {
    let name = text::pointer(exe, GROUPS + 12 * 12)?;
    ensure!(
        !name.is_empty() && !name.contains(['/', '\\']),
        "invalid native skit portrait archive name"
    );
    Ok(fs::read(
        root.join("disc1/files").join(name.to_ascii_lowercase()),
    )?)
}

fn member(archive: &[u8], index: u16) -> Result<Option<&[u8]>> {
    let count = dol::u32(archive, 0)? as usize;
    let end = count
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(4))
        .context("portrait directory size overflow")?;
    ensure!(end <= archive.len(), "truncated skit portrait directory");
    ensure!(usize::from(index) < count, "portrait index outside archive");
    let at = 4 + usize::from(index) * 8;
    let start = dol::u32(archive, at)? as usize;
    let size = dol::u32(archive, at + 4)? as usize;
    if size == 0 {
        return Ok(None);
    }
    ensure!(start >= end, "portrait overlaps directory");
    Ok(Some(
        archive
            .get(start..start.checked_add(size).context("portrait range overflow")?)
            .context("portrait exceeds archive")?,
    ))
}

fn recipe(exe: &[u8], id: u16) -> Result<PortraitRecipe> {
    let pointer = dol::u32(dol::slice(exe, RECIPES + u32::from(id) * 4, 4)?, 0)?;
    if pointer == 0 {
        return Ok(PortraitRecipe::default());
    }
    let mut result = PortraitRecipe::default();
    let mut seen = [false; 3];
    let mut at = pointer + 4; // Native flags and reserved word.
    loop {
        let header = dol::slice(exe, at, 4)?;
        at += 4;
        let tag = dol::u32(header, 0)?;
        if tag == 0xfefefefe {
            break;
        }
        let channel = match dol::u16(header, 0)? {
            0x7000 => 0,
            0x7003 => 1,
            0x7006 => 2,
            tag => anyhow::bail!("unknown portrait channel {tag:#x}"),
        };
        ensure!(!seen[channel], "duplicate portrait channel");
        seen[channel] = true;
        loop {
            let row = dol::slice(exe, at, 8)?;
            at += 8;
            let image = dol::u16(row, 6)? as i16;
            ensure!(image >= -1, "invalid portrait image {image}");
            let image = (image != -1).then_some(image as u16);
            match dol::u16(row, 4)? as i16 {
                0xfd => {
                    result.repeat[channel] = true;
                    break;
                }
                0xfe => break,
                ticks => result.tracks[channel].push(PortraitFrame {
                    ticks,
                    image,
                    position: [dol::u16(row, 0)? as i16, dol::u16(row, 2)? as i16],
                }),
            }
        }
    }
    Ok(result)
}

fn recipes(exe: &[u8], names: &BTreeMap<u32, String>) -> Result<BTreeMap<String, Animation>> {
    let mut groups = Vec::<(Vec<u16>, PortraitRecipe)>::new();
    for id in 0..RECIPE_COUNT {
        let parsed = recipe(exe, id)?;
        if let Some((ids, _)) = groups.iter_mut().find(|(_, previous)| *previous == parsed) {
            ids.push(id);
        } else {
            groups.push((vec![id], parsed));
        }
    }
    let mut animations = BTreeMap::new();
    for (ids, recipe) in groups {
        let name = if recipe.tracks.iter().all(Vec::is_empty) {
            "still".to_owned()
        } else {
            ids.iter()
                .find_map(|id| names.get(&(0xd0000 | u32::from(*id))))
                .with_context(|| format!("unnamed portrait animation {ids:?}"))?
                .clone()
        };
        ensure!(
            animations
                .insert(name, Animation::from_recipe(ids, &recipe)?)
                .is_none(),
            "duplicate portrait animation name"
        );
    }
    Ok(animations)
}

/// Read only the executable's portrait tracks; no textures are decoded.
pub fn skit_portrait_animations(root: &Path) -> Result<BTreeMap<String, Animation>> {
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("skit-portrait-names.json"))?;
    recipes(&fs::read(root.join("disc1/sys/main.dol"))?, &names)
}

fn image_path(owner: &str, index: usize, recipe: &PortraitRecipe) -> String {
    let base = format!("skits/portraits/{owner}");
    if index == 0 {
        return format!("{base}.ktx2");
    }
    for (role, track) in ["eyes", "mouth", "extra"].into_iter().zip(&recipe.tracks) {
        let images: BTreeSet<_> = track
            .iter()
            .filter_map(|frame| frame.image)
            .filter(|&image| image != 0)
            .collect();
        if let Some(position) = images.iter().position(|&image| usize::from(image) == index) {
            return format!("{base}-{role}-{}.ktx2", position + 1);
        }
    }
    format!("{base}-patch-{index}.ktx2")
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::output::write(&path, bytes)?;
    Ok(())
}

fn portrait_images(
    exe: &[u8],
    archive: &[u8],
    index: u16,
    owner: &str,
) -> Result<Vec<(String, Vec<u8>)>> {
    let native = member(archive, index)?.context("named portrait has no archive member")?;
    ensure!(dol::u32(native, 0)? == 0x0020af30, "invalid portrait TPL");
    let count = dol::u32(native, 4)? as usize;
    ensure!(count > 0, "portrait has no images");
    let recipe = recipe(exe, index)?;
    (0..count)
        .map(|image| {
            Ok((
                image_path(owner, image, &recipe),
                world_texture::tpl_texture(native, image)?.0,
            ))
        })
        .collect()
}

/// One named native member, for a focused image review.
pub fn cook_skit_portrait(root: &Path, library: &Path, id: u32) -> Result<()> {
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("skit-portrait-names.json"))?;
    let owner = names.get(&id).context("unnamed skit portrait")?;
    let index = u16::try_from(id.checked_sub(0xd0000).context("invalid portrait ID")?)?;
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let archive = archive(root, &exe)?;
    for (path, bytes) in portrait_images(&exe, &archive, index, owner)? {
        write_new(&library.join(path), &bytes)?;
    }
    Ok(())
}

/// One-time direct archive conversion with shared image paths for identical KTX2 data.
pub fn cook_skit_portraits(root: &Path, library: &Path) -> Result<()> {
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("skit-portrait-names.json"))?;
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let archive = archive(root, &exe)?;
    let count = dol::u32(&archive, 0)?;
    let mut document = Portraits {
        mappings: BTreeMap::new(),
        images: BTreeMap::new(),
        animations: recipes(&exe, &names)?,
    };
    let mut unique = BTreeMap::<Vec<u8>, String>::new();
    let mut files = Vec::<(PathBuf, Vec<u8>)>::new();
    for index in 0..count {
        let Some(_) = member(&archive, u16::try_from(index)?)? else {
            continue;
        };
        let id = 0xd0000 | index;
        let owner = names
            .get(&id)
            .with_context(|| format!("unnamed portrait {id}"))?;
        let mut images = Vec::new();
        for (path, bytes) in portrait_images(&exe, &archive, index as u16, owner)? {
            let canonical = if let Some(existing) = unique.get(&bytes) {
                existing.clone()
            } else {
                unique.insert(bytes.clone(), path.clone());
                files.push((library.join(&path), bytes));
                path
            };
            images.push(canonical);
        }
        ensure!(
            document.images.insert(owner.clone(), images).is_none(),
            "duplicate portrait name"
        );
        document.mappings.insert(id, owner.clone());
    }
    ensure!(
        document.mappings.len() == names.len(),
        "portrait name inventory differs from archive"
    );
    for (path, bytes) in files {
        write_new(&path, &bytes)?;
    }
    write_new(
        &library.join(resonance_content::skit::document::portraits::PATH),
        &resonance_asset_writer::json::encode(&document)?,
    )?;
    Ok(())
}
