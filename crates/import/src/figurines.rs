//! Prepare catalogue previews from shared physical models, curves and menu records.
use crate::{
    all_assets::PhysicalDirectory,
    animation::AuthoredAnimation,
    cooked::Source,
    menu::{BoneRule, FigurineModel, FigurineRow},
    scene::binding::{self, read},
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace, SceneClip,
    figurine::{FIGURINE_COUNT, FIGURINE_VERSION, Figurine, FigurineBook},
    model_preview::{ModelPreview, PreviewPart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn book(output: &Path, disc: u8) -> Result<FigurineBook> {
    Ok(FigurineBook {
        title: crate::menu::figurine_catalogue(output, disc)?.title,
        records: (0..FIGURINE_COUNT)
            .map(|id| read(output, &format!("figurines/{id:03}.json")))
            .collect::<Result<_>>()?,
    })
}

pub fn cook(extracted: &Path, output: &Path, selected: &[u16]) -> Result<()> {
    ensure!(
        selected.iter().all(|&id| usize::from(id) < FIGURINE_COUNT),
        "unknown figurine"
    );
    let disc = crate::disc_number(extracted)?;
    let catalogue = crate::menu::figurine_catalogue(output, disc)?;
    let files = extracted.join("files");
    let path = crate::all_assets::roles::declared_path(&files, &catalogue.archive)?;
    let source = Source::open(output, disc, &path)?;
    let (directory, bytes) = source.resolve("archive.json")?;
    ensure!(
        directory == format!("assets/{}", crate::media::hash_file(&files.join(path))?),
        "cooked figurine archive differs from source"
    );
    let archive: PhysicalDirectory = serde_json::from_slice(&bytes)?;
    archive.validate()?;
    let mut models = BTreeMap::new();
    for (id, row) in catalogue
        .records
        .into_iter()
        .take(FIGURINE_COUNT)
        .enumerate()
    {
        ensure!(row.slot == id, "unordered cooked figurine catalogue");
        if !selected.is_empty() && !selected.contains(&(id as u16)) {
            continue;
        }
        let FigurineModel::Npc { entry } = row.model else {
            anyhow::bail!("figurine {id} has no model");
        };
        // Empty entries have no preview; unlike field resources they never alias entry zero.
        let canonical = archive
            .members
            .get(entry as usize)
            .copied()
            .flatten()
            .with_context(|| format!("figurine {id} has no archive entry {entry}"))?;
        if let std::collections::btree_map::Entry::Vacant(entry) = models.entry(canonical) {
            entry.insert(
                model(output, &format!("{directory}/{canonical}"))
                    .with_context(|| format!("figurine {id} model {canonical}"))?,
            );
        }
        let model = &models[&canonical];
        let record = Figurine {
            version: FIGURINE_VERSION,
            id: id as u16,
            name: row.name.clone().context("unnamed figurine")?,
            preview: preview(model, &row, &catalogue.hidden_prefix),
        };
        record
            .validate()
            .with_context(|| format!("figurine {id}"))?;
        write_atomic(
            &output.join(format!("figurines/{id:03}.json")),
            &serde_json::to_vec_pretty(&record)?,
        )?;
    }
    Ok(())
}

struct Model {
    parts: Vec<PreviewPart>,
    appearance: Vec<Option<usize>>,
    rows: u32,
}

fn preview(model: &Model, row: &FigurineRow, hidden_prefix: &str) -> ModelPreview {
    let mut parts = model.parts.clone();
    let bones = &parts[0].scene.bone_names;
    let mut hidden: BTreeSet<_> = bones
        .iter()
        .filter(|name| name.starts_with(hidden_prefix))
        .cloned()
        .collect();
    for rule in row.bone_rules.iter().flatten() {
        let (prefix, hide) = match rule {
            BoneRule::Hide { prefix } => (prefix, true),
            BoneRule::Show { prefix } => (prefix, false),
        };
        for bone in bones.iter().filter(|name| name.starts_with(prefix)) {
            if hide {
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
    ModelPreview {
        scale: 1.,
        elevation: row.elevation,
        parts,
        hidden_geometry: hidden.into_iter().collect(),
        node_scales: Vec::new(),
    }
}

fn model(root: &Path, directory: &str) -> Result<Model> {
    let package: PhysicalDirectory = read(root, &format!("{directory}/members.json"))?;
    package.validate()?;
    ensure!(package.count >= 2, "figurine package needs two layer slots");
    let member = |index: usize| package.members[index].map(|id| format!("{directory}/{id}"));
    let primary = member(0).context("missing figurine mesh")?;
    // The final package member is not part of the native idle search.
    let animation = (2..package.members.len().saturating_sub(1))
        .find_map(member)
        .map(|path| read::<AuthoredAnimation>(root, &format!("{path}/animation.json")))
        .transpose()?;
    let mut model = Model {
        parts: Vec::new(),
        appearance: Vec::new(),
        rows: 0,
    };
    for (index, directory) in [Some(primary), member(1)].into_iter().enumerate() {
        let Some(directory) = directory else {
            continue;
        };
        let (mut scene, mut glb) = binding::model(root, &directory)?;
        scene.resource = index as u16;
        for material in &mut scene.materials {
            material.draw_order += index as u32 * 65536;
        }
        #[derive(serde::Deserialize)]
        struct Identity {
            name: Option<String>,
        }
        let identity: Identity = read(root, &format!("{directory}/model.json"))?;
        scene.secondary_motion = crate::secondary_motion::bind(
            identity.name.as_deref().unwrap_or_default(),
            &glb.json,
            &scene.bone_names,
        )?;
        if let Some(animation) = &animation {
            let bindings = binding::bindings(root, &directory, &scene)?;
            let duration_seconds = crate::animation::bake_motion(
                &animation.motion(&bindings)?,
                &mut glb.json,
                &mut glb.binary,
                "figurine-idle",
            )?;
            scene.clips.push(SceneClip {
                resource_slot: 0,
                duration_seconds,
                animation_resource: None,
                secondary_pose_nodes: serde_json::from_value(
                    glb.json["animations"][0]["extras"]["secondary_pose_nodes"].clone(),
                )?,
            });
            scene.autoplay = true;
            let bytes = crate::scene::pack_glb(&glb.json, &mut glb.binary)?;
            scene.mesh = format!("figurines/scenes/{}.glb", crate::digest(&bytes));
            write_atomic(&root.join(&scene.mesh), &bytes)?;
        }
        if index == 1 {
            scene.outline_color = Some([0, 0, 0, 127]);
            for material in &mut scene.materials {
                material.cull = CullFace::Front;
                material.blend = true;
            }
        }
        let selector = selector(root, &directory)?;
        if index == 0
            && let Some(texture) = selector
        {
            let palette =
                crate::texture::read(&root.join(&directory).join("palettes/textures.json"))?;
            let texture = palette
                .textures
                .get(texture)
                .and_then(Option::as_ref)
                .context("invalid figurine appearance texture")?;
            model.rows = match texture.dimensions[1] {
                512 => 2,
                1024 => 4,
                _ => 0,
            };
        }
        model.appearance.push(selector);
        model.parts.push(PreviewPart {
            animation: animation.as_ref().map(|_| 0),
            scene,
            attached_to: None,
            additive: false,
            uv_offsets: Vec::new(),
        });
    }
    Ok(model)
}

fn selector(root: &Path, directory: &str) -> Result<Option<usize>> {
    #[derive(serde::Deserialize)]
    struct Selectors {
        texture_indices_by_slot: [Option<u8>; 14],
    }
    let path = format!("{directory}/texture-selectors.json");
    if !root.join(&path).try_exists()? {
        return Ok(None);
    }
    Ok(read::<Selectors>(root, &path)?.texture_indices_by_slot[2].map(usize::from))
}

#[cfg(test)]
mod tests;
