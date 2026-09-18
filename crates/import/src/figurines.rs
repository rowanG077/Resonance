//! Convert figurine catalogue entries and shared NPC models into player assets.
use crate::{
    character::archive_entry,
    compression, dol,
    field::sections,
    model_preview::{Layer, layers},
    read::u32 as word,
    write_atomic,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    figurine::{FIGURINE_COUNT, FIGURINE_VERSION, Figurine, FigurineBook},
    model_preview::{ModelPreview, PreviewPart},
};
use std::{collections::BTreeMap, fs, path::Path};

pub(crate) fn book(executable: &[u8], output: &Path) -> Result<FigurineBook> {
    Ok(FigurineBook {
        title: dol::text(executable, word(dol::slice(executable, 0x8019d6f4, 4)?, 0)?)?,
        records: (0..FIGURINE_COUNT)
            .map(|id| {
                serde_json::from_slice(&fs::read(output.join(format!("figurines/{id:03}.json")))?)
                    .context("figurine assets need recooking; run cook-figurines")
            })
            .collect::<Result<_>>()?,
    })
}

pub fn cook(extracted: &Path, output: &Path, selected: &[u16]) -> Result<()> {
    ensure!(
        selected.iter().all(|&id| usize::from(id) < FIGURINE_COUNT),
        "unknown figurine"
    );
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let archive = fs::read(extracted.join("files/npc_all.bin"))?;
    let mut models: BTreeMap<u32, Vec<PreviewPart>> = BTreeMap::new();
    for id in 0..FIGURINE_COUNT as u16 {
        if !selected.is_empty() && !selected.contains(&id) {
            continue;
        }
        let row = dol::slice(&executable, 0x802280c0 + u32::from(id) * 80, 80)?;
        let resource = word(row, 8)?
            .checked_sub(0x20000)
            .context("invalid figurine model")?;
        let bytes = archive_entry(&archive, resource as usize)?;
        if let std::collections::btree_map::Entry::Vacant(entry) = models.entry(resource) {
            entry.insert(
                model(bytes, resource, output)
                    .with_context(|| format!("figurine {id} model {resource}"))?,
            );
        }
        let mut parts = models[&resource].clone();
        let bones = &parts[0].scene.bone_names;
        let mut hidden: std::collections::BTreeSet<_> = bones
            .iter()
            .filter(|name| name.starts_with("kk"))
            .cloned()
            .collect();
        for offset in (16..80).step_by(4) {
            let pointer = word(row, offset)?;
            if pointer == 0 {
                continue;
            }
            let rule = dol::text(&executable, pointer)?;
            let prefix = rule.strip_prefix('-').unwrap_or(&rule);
            for bone in bones.iter().filter(|b| b.starts_with(prefix)) {
                if rule.starts_with('-') {
                    hidden.insert(bone.clone());
                } else {
                    hidden.remove(bone);
                }
            }
        }
        let variant = word(row, 12)?;
        if variant != 0 {
            let ranges = sections(bytes)?;
            for (index, part) in parts.iter_mut().enumerate() {
                let data = &bytes[ranges[index].clone().context("missing figurine layer")?];
                let Some(texture) = data[14].checked_sub(1).map(usize::from) else {
                    continue;
                };
                let primary = &bytes[ranges[0].clone().unwrap()];
                let normalized = crate::character::texture_palette(primary, data)?;
                let palette = crate::tpl::parse_tpl(&normalized[word(&normalized, 0)? as usize..])?;
                let texture_info = palette
                    .get(texture)
                    .context("invalid figurine variant texture")?;
                let frames = match texture_info.height {
                    512 => 2,
                    1024 => 4,
                    _ => 0,
                };
                if variant >= frames {
                    continue;
                }
                let shift = variant as f32 / frames as f32;
                part.uv_offsets = part
                    .scene
                    .materials
                    .iter()
                    .map(|m| {
                        let y = |binding: &Option<resonance_content::TextureBinding>| {
                            if binding.as_ref().is_some_and(|b| b.texture == texture) {
                                shift
                            } else {
                                0.
                            }
                        };
                        [0., y(&m.color), 0., y(&m.multiply)]
                    })
                    .collect();
            }
        }
        let record = Figurine {
            version: FIGURINE_VERSION,
            id,
            name: dol::text(&executable, word(row, 0)?)?,
            preview: ModelPreview {
                scale: 1.,
                elevation: if resource == 73 { -80. } else { 0. },
                parts,
                hidden_geometry: hidden.into_iter().collect(),
                node_scales: Vec::new(),
            },
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

fn model(bytes: &[u8], id: u32, output: &Path) -> Result<Vec<PreviewPart>> {
    let ranges = sections(bytes)?;
    let model = &bytes[ranges[0].clone().context("missing figurine mesh")?];
    let outline = ranges
        .get(1)
        .and_then(Option::as_ref)
        .map(|r| &bytes[r.clone()]);
    let animation = ranges
        .iter()
        .skip(2)
        .flatten()
        .next()
        .map(|r| {
            let data = &bytes[r.clone()];
            if data.starts_with(&0x007b7960u32.to_be_bytes()) {
                Ok(data.to_vec())
            } else {
                compression::decode(data)
            }
        })
        .transpose()?;
    let mut parts = Vec::new();
    layers(
        Layer {
            model,
            outline,
            animation: animation.as_deref(),
            attached_to: None,
            additive: false,
        },
        &mut parts,
        &format!("figurines/models/{id:03}"),
        output,
    )?;
    Ok(parts)
}

#[test]
#[ignore = "requires locally cooked figurine assets"]
fn catalogue_uses_shared_converted_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked");
    let mut meshes = std::collections::BTreeSet::new();
    for id in 0..FIGURINE_COUNT {
        let record: Figurine = serde_json::from_slice(
            &fs::read(root.join(format!("figurines/{id:03}.json"))).unwrap(),
        )
        .unwrap();
        record.validate().unwrap();
        assert_eq!(usize::from(record.id), id);
        meshes.insert(record.preview.parts[0].scene.mesh.clone());
        for part in &record.preview.parts {
            assert_eq!(
                &fs::read(root.join(&part.scene.mesh)).unwrap()[..4],
                b"glTF"
            );
            for texture in &part.scene.textures {
                assert!(texture.ends_with(".ktx2") && root.join(texture).is_file());
            }
        }
        if id == 172 {
            assert_eq!(record.name, "Katz");
            assert_eq!(record.preview.parts[0].scene.clips[0].duration_ticks(), 120);
        }
    }
    assert_eq!(meshes.len(), 271, "model variants should share mesh files");
}
