//! Native collection entries reuse converted NPC bodies and shared preview parts.
use crate::{
    model, output,
    package::Package,
    pe, preview_model,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    figurine::FIGURINE_VERSION, menu_data::FIGURINES_PATH, model_preview::ModelPreview,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::FigurineId;

fn count(image: &pe::Image<'_>) -> Result<usize> {
    ensure!(
        image.slice(0x56d3b1, 2)? == [0x81, 0xfe],
        "unsupported figurine bound"
    );
    let count = pe::word(image.slice(0x56d3b3, 4)?, 0)? as usize;
    ensure!((1..=4096).contains(&count), "invalid figurine count");
    Ok(count)
}
fn symbol(id: u16) -> Result<String> {
    let added = [
        "lloyd-ancient-swordsman",
        "colette-lloyd",
        "genis-snowboarder",
        "raine-bunny",
        "sheena-biker",
        "zelos-biker",
        "presea-snowboarder",
        "regal-president",
        "kratos-ancient-swordsman",
    ];
    if (35..44).contains(&id) {
        return Ok(added[usize::from(id - 35)].into());
    }
    let original = if id >= 44 { id - 9 } else { id };
    Ok(FigurineId::try_from(original)?
        .resource_name()
        .context("unnamed figurine")?
        .to_owned())
}
pub(crate) fn catalogue(source: &Source, library: &Path) -> Result<()> {
    let text = source.require("TOS_TEXT.TXB")?;
    let menu = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&text)?,
        slot: source.language.slot(),
    };
    let captions = menu.directory(0x8b9628)?;
    let records = (0..count(&menu.image)?).map(|id| -> Result<_> {
        let row = menu.image.slice(0x8b9640 + id as u32*80,80)?;
        let name = menu.caption(captions,pe::word(row,0)?)?;
        ensure!(!name.is_empty(), "figurine {id} has no name");
        Ok(serde_json::json!({"id":id,"name":name,"version":FIGURINE_VERSION,"preview":format!("meshes/figurines/{}/model.json",symbol(id as u16)?)}))
    }).collect::<Result<Vec<_>>>()?;
    let title = menu.caption(menu.directory(0x8b943c)?, 4)?;
    output::json(
        &library.join(FIGURINES_PATH),
        &serde_json::json!({"title":title,"records":records}),
    )
}

struct Converted {
    preview: ModelPreview,
    materials: model::Materials,
    descriptor: String,
    appearance: Option<(u32, Vec<[f32; 4]>)>,
}
pub(crate) fn models(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let mut converted = BTreeMap::<String, Converted>::new();
    for id in 0..count(&image)? {
        let row = image.slice(0x8b9640 + id as u32 * 80, 80)?;
        let owner = format!("meshes/figurines/{}", symbol(id as u16)?);
        eprintln!("  Figurine model: {owner} ({id})");
        let resource = pe::word(row, 8)?;
        ensure!((resource as i32) >= 0, "figurine {id} has no model");
        let npc = if resource >= 0x20000 {
            resource - 0x20000
        } else {
            resource
        };
        let npc = npc - u32::from(npc > 255);
        let name = format!("npc_all_b{npc:03}.pac");
        if !converted.contains_key(&name) {
            let bytes = source.require(&name)?;
            let package = Package::parse(&bytes)?;
            ensure!(package.len() >= 3, "figurine model has no idle directory");
            let body = package.reference(0)?.context("figurine has no body")?;
            let outline = package.reference(1)?.unwrap_or("");
            let idle = (2..package.len() - 1)
                .map(|slot| package.reference(slot))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .next();
            let motions = idle
                .map(|name| (0, name.to_owned()))
                .into_iter()
                .collect::<Vec<_>>();
            let (preview, native) =
                preview_model::cook(source, library, [body, outline], &owner, &motions)?;
            converted.insert(
                name.clone(),
                Converted {
                    preview,
                    materials: native.materials,
                    descriptor: body.to_owned(),
                    appearance: None,
                },
            );
        }
        let base = converted.get_mut(&name).unwrap();
        let mut preview = base.preview.clone();
        let appearance = pe::word(row, 12)?;
        if appearance != 0 {
            if base.appearance.is_none() {
                base.appearance = Some(appearance_offsets(
                    source,
                    library,
                    &base.descriptor,
                    &base.materials,
                    &base.preview,
                )?);
            }
            let (rows, offsets) = base.appearance.as_ref().unwrap();
            if *rows > 1 {
                ensure!(appearance < *rows, "figurine {id} appearance exceeds atlas");
                preview.parts[0].uv_offsets = offsets
                    .iter()
                    .map(|uv| uv.map(|v| v * appearance as f32))
                    .collect();
            }
        }
        let mut hidden: BTreeSet<_> = preview.hidden_geometry.into_iter().collect();
        for pointer in row[16..].chunks_exact(4) {
            let pointer = pe::word(pointer, 0)?;
            if pointer == 0 {
                continue;
            }
            let rule = image.text(pointer)?;
            let prefix = rule.trim_start_matches('-').to_ascii_lowercase();
            for bone in preview.parts[0]
                .scene
                .bone_names
                .iter()
                .filter(|bone| bone.to_ascii_lowercase().starts_with(&prefix))
            {
                if rule.starts_with('-') {
                    hidden.insert(bone.clone());
                } else {
                    hidden.remove(bone);
                }
            }
        }
        preview.hidden_geometry = hidden.into_iter().collect();
        preview.validate_definition()?;
        output::json(&library.join(&owner).join("model.json"), &preview)?;
    }
    Ok(())
}
fn appearance_offsets(
    source: &Source,
    library: &Path,
    descriptor: &str,
    materials: &model::Materials,
    preview: &ModelPreview,
) -> Result<(u32, Vec<[f32; 4]>)> {
    let stem = descriptor
        .rsplit_once('.')
        .context("model descriptor has no extension")?
        .0;
    let (_, _, roles) =
        model::material_parameters(&source.require(&format!("{stem}.mxd"))?, materials)?;
    let mut selected = BTreeSet::new();
    for (material, role) in materials.records.iter().zip(roles) {
        if role == 3
            && let Some(texture) = material.textures.first()
        {
            selected.insert(usize::from(texture.texture));
        }
    }
    let body = &preview.parts[0].scene;
    let sizes = body
        .textures
        .iter()
        .map(|path| resonance_asset_writer::texture::dimensions(&fs::read(library.join(path))?))
        .collect::<Result<Vec<_>>>()?;
    if selected.is_empty() {
        selected.extend(
            sizes
                .iter()
                .enumerate()
                .filter_map(|(i, &size)| (size == [256, 512]).then_some(i)),
        );
    }
    // Bodies without vertical variants need no appearance binding.
    ensure!(selected.len() == 1, "ambiguous figurine body atlas");
    let selected = *selected.first().unwrap();
    let [width, height] = sizes[selected];
    ensure!(
        width == 256 && matches!(height, 256 | 512 | 1024),
        "unsupported figurine body atlas"
    );
    let rows = height / 256;
    let offsets = body
        .materials
        .iter()
        .map(|material| {
            let shift = |binding: &Option<resonance_content::TextureBinding>| {
                if binding.as_ref().is_some_and(|b| b.texture == selected) {
                    1. / rows as f32
                } else {
                    0.
                }
            };
            [0., shift(&material.color), 0., shift(&material.multiply)]
        })
        .collect();
    Ok((rows, offsets))
}
