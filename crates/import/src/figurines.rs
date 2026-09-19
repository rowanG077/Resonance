//! Prepare catalogue previews directly from original NPC models and menu records.
use crate::{
    all_assets::figurine_catalogue::{Catalogue, Record, Resource},
    scene::source::Models,
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace,
    figurine::{FIGURINE_COUNT, FIGURINE_VERSION, Figurine, FigurineBook},
    model_preview::{ModelPreview, PreviewPart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) fn prepare(extracted: &Path, output: &Path, executable: &[u8]) -> Result<FigurineBook> {
    let catalogue = crate::all_assets::figurine_catalogue::read(executable)?;
    ensure!(
        catalogue.records.len() >= FIGURINE_COUNT,
        "incomplete figurine catalogue"
    );
    let files = extracted.join("files");
    let path =
        crate::all_assets::roles::declared_path(&files, catalogue.text(catalogue.archive.text))?;
    let bytes = fs::read(files.join(path))?;
    let archive = crate::all_assets::archive_entries(&bytes).context("invalid NPC archive")?;
    let mut models = BTreeMap::new();
    let mut records = Vec::new();
    let mut behaviors = crate::model_behavior::Bindings::new()?;
    for (id, row) in catalogue.records.iter().take(FIGURINE_COUNT).enumerate() {
        ensure!(!row.is_null(), "figurine {id} has no model");
        let (Resource::DirectNpc(entry) | Resource::TaggedNpc(entry)) = row.resource else {
            anyhow::bail!("figurine {id} has no model");
        };
        // Empty entries have no preview; unlike field resources they never alias entry zero.
        let range = archive
            .get(entry as usize)
            .and_then(Option::as_ref)
            .with_context(|| format!("figurine {id} has no archive entry {entry}"))?;
        let canonical = (range.start, range.end);
        if let std::collections::btree_map::Entry::Vacant(entry) = models.entry(canonical) {
            entry.insert(
                model(
                    output,
                    &crate::compression::payload(bytes[range.clone()].to_vec())?,
                )
                .with_context(|| format!("figurine {id} model {canonical:?}"))?,
            );
        }
        let model = &models[&canonical];
        let record = Figurine {
            version: FIGURINE_VERSION,
            id: id as u16,
            name: catalogue.required_text(row.name)?.into(),
            preview: preview(model, row, &catalogue, &mut behaviors)?,
        };
        record
            .validate()
            .with_context(|| format!("figurine {id}"))?;
        write_atomic(
            &output.join(format!("figurines/{id:03}.json")),
            &serde_json::to_vec_pretty(&record)?,
        )?;
        records.push(record);
    }
    behaviors.finish(crate::model_behavior::Catalogue::Figurines)?;
    Ok(FigurineBook {
        title: catalogue.required_text(catalogue.title)?.into(),
        records,
    })
}

struct Model {
    parts: Vec<PreviewPart>,
    appearance: Vec<Option<usize>>,
    rows: u32,
}

fn preview(
    model: &Model,
    row: &Record,
    catalogue: &Catalogue,
    behaviors: &mut crate::model_behavior::Bindings,
) -> Result<ModelPreview> {
    let hidden_prefix = catalogue.text(catalogue.preview.hidden_prefix.text);
    let mut parts = model.parts.clone();
    let bones = &parts[0].scene.bone_names;
    let mut hidden: BTreeSet<_> = bones
        .iter()
        .filter(|name| name.starts_with(hidden_prefix))
        .cloned()
        .collect();
    for &rule in row.bone_rules.iter().flatten() {
        let rule = catalogue.text(rule);
        let prefix = rule.strip_prefix('-').unwrap_or(rule);
        for bone in bones.iter().filter(|name| name.starts_with(prefix)) {
            if rule.starts_with('-') {
                hidden.insert(bone.clone());
            } else {
                hidden.remove(bone);
            }
        }
    }
    if row.appearance_row > 0 && row.appearance_row < model.rows {
        // Both layers use the primary atlas height, but each has its own texture selector.
        let shift = row.appearance_row as f32 / model.rows as f32;
        for (part, texture) in parts.iter_mut().zip(&model.appearance) {
            let Some(texture) = texture else {
                continue;
            };
            part.uv_offsets = part
                .scene
                .materials
                .iter()
                .map(|material| {
                    let y = |binding: &Option<resonance_content::TextureBinding>| {
                        if binding.as_ref().is_some_and(|b| b.texture == *texture) {
                            shift
                        } else {
                            0.
                        }
                    };
                    [0., y(&material.color), 0., y(&material.multiply)]
                })
                .collect();
        }
    }
    let mut preview = ModelPreview {
        scale: 1.,
        elevation: catalogue.preview.default_elevation,
        behavior: None,
        parts,
        hidden_geometry: hidden.into_iter().collect(),
    };
    preview.behavior = behaviors.bind(crate::model_behavior::Subject::Figurine(row.resource));
    Ok(preview)
}

fn model(root: &Path, package: &[u8]) -> Result<Model> {
    let sections = crate::field::sections(package)?;
    ensure!(
        sections.len() >= 2,
        "figurine package needs two layer slots"
    );
    let member = |index: usize| {
        sections[index]
            .as_ref()
            .map(|range| &package[range.clone()])
    };
    let primary = member(0).context("missing figurine mesh")?;
    // The final package member is not part of the native idle search.
    let animation = (2..sections.len().saturating_sub(1))
        .find_map(member)
        .map(crate::animation::read_member)
        .transpose()?;
    let mut model = Model {
        parts: Vec::new(),
        appearance: Vec::new(),
        rows: 0,
    };
    let mut models = Models::new(root);
    for (index, source) in [Some(primary), member(1)].into_iter().enumerate() {
        let Some(source) = source else {
            continue;
        };
        let (selector, rows) = appearance(primary, source, index == 0)?;
        model.appearance.push(selector);
        if index == 0 {
            model.rows = rows;
        }
        let animation = animation.as_ref();
        models.add(
            &format!("figurine/{index}"),
            source,
            primary,
            move |geometry, _, scene, glb| {
                scene.resource = index as u16;
                for material in &mut scene.materials {
                    material.draw_order += index as u32 * 65536;
                    if index == 1 {
                        material.cull = CullFace::Front;
                        material.blend = true;
                    }
                }
                scene.outline_color = (index == 1).then_some([0, 0, 0, 127]);
                if let Some(animation) = animation {
                    let bindings = geometry
                        .bindings
                        .as_ref()
                        .context("animated figurine has no skeleton")?;
                    scene.clips.push(glb.animate(animation.motion(bindings)?)?);
                    scene.autoplay = true;
                }
                Ok(())
            },
        )?;
    }
    model.parts = models
        .finish()?
        .into_iter()
        .map(|layer| PreviewPart {
            animation: animation.as_ref().map(|_| 0),
            scene: layer.part,
            attached_to: None,
            additive: false,
            uv_offsets: Vec::new(),
        })
        .collect();
    Ok(model)
}

fn appearance(primary: &[u8], source: &[u8], primary_layer: bool) -> Result<(Option<usize>, u32)> {
    let word = crate::read::u32;
    let palette = if word(source, 0)? == 0 {
        primary
    } else {
        source
    };
    let textures = if word(palette, 0)? == 0 {
        Vec::new()
    } else {
        crate::tpl::parse_tpl(
            palette
                .get(word(palette, 0)? as usize..word(palette, 4)? as usize)
                .context("invalid figurine palette range")?,
        )?
    };
    let selector =
        crate::all_assets::geometry::model_selectors(source, textures.len().try_into()?)?[2]
            .map(usize::from);
    let rows = if primary_layer && let Some(index) = selector {
        match textures
            .get(index)
            .context("invalid figurine appearance texture")?
            .height
        {
            512 => 2,
            1024 => 4,
            _ => 0,
        }
    } else {
        0
    };
    Ok((selector, rows))
}

#[cfg(test)]
pub(crate) mod tests;
