//! Native portrait images and pose tracks use GC's named portrait document.
use crate::{output, package::Package, pe, resources::Resources, source::Source, ui_images};
use anyhow::{Context, Result, ensure};
use resonance_content::skit::{
    PortraitFrame, PortraitRecipe,
    document::portraits::{self, Animation, Portraits},
};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let names: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("../../importer/src/skit-portrait-names.json"))?;
    let mut document = Portraits::default();
    let mut groups = Vec::<(Vec<u16>, PortraitRecipe)>::new();
    for id in 0..230u16 {
        let recipe = recipe(&image, id)?;
        if let Some((ids, _)) = groups.iter_mut().find(|(_, previous)| *previous == recipe) {
            ids.push(id);
        } else {
            groups.push((vec![id], recipe));
        }
    }
    for (ids, recipe) in groups {
        let name = if recipe.tracks.iter().all(Vec::is_empty) {
            "still".to_owned()
        } else {
            ids.iter()
                .find_map(|id| names.get(&(0xd0000 | u32::from(*id))))
                .cloned()
                .unwrap_or_else(|| format!("animation-{}", ids[0]))
        };
        ensure!(
            document
                .animations
                .insert(name, Animation::from_recipe(ids, &recipe)?)
                .is_none(),
            "duplicate portrait animation name"
        );
    }
    for (id, filename) in Resources::read(source)?
        .entries
        .into_iter()
        .filter(|(id, _)| id >> 16 == 13)
    {
        let Some(filename) = filename else {
            continue;
        };
        let owner = names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("portrait-{:03}", id & 65535));
        let bytes = source.require(&filename)?;
        let package = Package::parse(&bytes)?;
        let mut images = Vec::new();
        for index in 0..package.len() {
            let name = package
                .reference(index)?
                .context("missing portrait image")?;
            let path = format!("skits/portraits/{owner}/image-{index}.ktx2");
            let image = ui_images::cook(source, library, name, &path, true)?;
            ensure!(
                image.width <= 1024
                    && image.height <= 1024
                    && image.width.is_multiple_of(8)
                    && image.height.is_multiple_of(8),
                "invalid portrait dimensions"
            );
            images.push(image);
        }
        ensure!(!images.is_empty(), "portrait {id} has no images");
        ensure!(
            document.images.insert(owner.clone(), images).is_none(),
            "duplicate portrait owner"
        );
        document.mappings.insert(id, owner);
    }
    output::json(&library.join(portraits::PATH), &document)
}

fn recipe(image: &pe::Image<'_>, id: u16) -> Result<PortraitRecipe> {
    let pointer = pe::word(image.slice(0x8b3908 + u32::from(id) * 4, 4)?, 0)?;
    let mut recipe = PortraitRecipe::default();
    if pointer == 0 {
        return Ok(recipe);
    }
    let mut at = pointer + 4;
    let mut seen = [false; 3];
    for _ in 0..4 {
        let header = image.slice(at, 4)?;
        at += 4;
        if pe::word(header, 0)? == 0xfefefefe {
            return Ok(recipe);
        }
        let channel = match pe::half(header, 0)? {
            0x7000 => 0,
            0x7003 => 1,
            0x7006 => 2,
            tag => anyhow::bail!("unknown portrait channel {tag:#x}"),
        };
        ensure!(!seen[channel], "duplicate portrait track");
        seen[channel] = true;
        loop {
            let row = image.slice(at, 8)?;
            at += 8;
            let duration = pe::half(row, 4)? as i16;
            match duration {
                253 => {
                    recipe.repeat[channel] = true;
                    break;
                }
                254 => break,
                _ => {}
            }
            let index = pe::half(row, 6)? as i16;
            ensure!(
                duration >= 0 && index >= -1 && recipe.tracks[channel].len() < 255,
                "invalid portrait frame"
            );
            recipe.tracks[channel].push(PortraitFrame {
                ticks: duration,
                image: (index != -1).then_some(index as u16),
                position: [pe::half(row, 0)? as i16, pe::half(row, 2)? as i16],
            });
        }
    }
    anyhow::bail!("unterminated portrait recipe")
}
