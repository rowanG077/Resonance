//! Figurine previews directly from the native catalogue and NPC model archive.
use crate::{
    character_models, compression, dol, field_core, native_archive, text, world_gpl, world_motion,
    world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace, SceneClip,
    model_preview::{ModelPreview, PreviewPart},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::{FigurineId, PartyCharacterId};

const RECORDS: u32 = 0x8022_80c0;
const ARCHIVE: u32 = 0x801a_aa18;
const COUNT: usize = 288;
const STRIDE: usize = 80;

#[derive(Deserialize)]
struct Costume {
    name: String,
}

fn party_variant(id: u16) -> Option<(u8, u8)> {
    match id {
        0..=8 => Some((id as u8 + 1, 0)),
        10..=17 => Some(((id - 10) as u8 + 1, 1)),
        18..=25 => Some(((id - 18) as u8 + 1, 2)),
        26 => Some((9, 1)),
        27..=34 => Some(((id - 27) as u8 + 1, 4)),
        _ => None,
    }
}

fn party_idle(library: &Path, character: u8, variant: u8) -> Result<Option<String>> {
    let name = PartyCharacterId::try_from(character)?
        .resource_name()
        .context("party figurine has no character name")?;
    let costumes: BTreeMap<u8, BTreeMap<u8, Costume>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    let model = if variant == 0 {
        format!("meshes/characters/{name}/model.json")
    } else {
        let costume = &costumes[&character][&variant].name;
        format!("meshes/characters/{name}/{costume}/model.json")
    };
    let file = library.join(model);
    if !file.is_file() {
        return Ok(None);
    }
    let model: resonance_content::field::document::Model =
        serde_json::from_slice(&fs::read(file)?)?;
    Ok(model.parts.first().and_then(|part| {
        part.clips
            .iter()
            .find(|clip| clip.resource_slot == 12)
            .map(|clip| clip.motion.clone())
    }))
}

fn native_archive(extracted: &Path, exe: &[u8]) -> Result<Vec<u8>> {
    let name = dol::slice(exe, ARCHIVE, 12)?;
    let end = name
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated figurine archive name")?;
    let name = std::str::from_utf8(&name[..end])?;
    native_archive::read_named(&extracted.join("disc1/files"), name)
}

fn native_model<'a>(bank: &'a [u8], selector: u32) -> Result<&'a [u8]> {
    let index = (selector & 0xffff) as usize;
    ensure!(
        selector < 0x40000,
        "invalid figurine model selector {selector}"
    );
    let count = dol::u32(bank, 0)? as usize;
    ensure!(
        index < count && 4 + count * 8 <= bank.len(),
        "missing figurine model {index}"
    );
    let at = 4 + index * 8;
    let start = dol::u32(bank, at)? as usize;
    let size = dol::u32(bank, at + 4)? as usize;
    ensure!(
        start >= 4 + count * 8 && size > 0,
        "empty figurine model {index}"
    );
    bank.get(start..start + size)
        .context("figurine model exceeds bank")
}

fn payload(mut bytes: Vec<u8>) -> Result<Vec<u8>> {
    for _ in 0..16 {
        if bytes.starts_with(b"MSCF") {
            bytes = compression::cabinet(&bytes)?;
            continue;
        }
        let wrapped = bytes.get(..9).and_then(|head| {
            if !matches!(head[0], 0 | 1 | 3) {
                return None;
            }
            let packed = u32::from_le_bytes(head[1..5].try_into().ok()?) as usize;
            let size = u32::from_le_bytes(head[5..9].try_into().ok()?) as usize;
            let end = 9usize.checked_add(packed)?;
            (size > 0
                && size <= 16 * 1024 * 1024
                && end <= bytes.len()
                && bytes.len() - end < 32
                && bytes[end..].iter().all(|&value| value == 0))
            .then_some(end)
        });
        if let Some(end) = wrapped {
            bytes = compression::decode(&bytes[..end])?;
        } else {
            return Ok(bytes);
        }
    }
    anyhow::bail!("figurine model compression nesting exceeds 16 levels")
}

fn palette(model: &[u8]) -> Result<&[u8]> {
    let start = dol::u32(model, 0)? as usize;
    let end = dol::u32(model, 4)? as usize;
    let tpl = model
        .get(start..end)
        .context("figurine palette exceeds model")?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "figurine model lacks TPL");
    Ok(tpl)
}

fn texture_paths(
    tpl: &[u8],
    base: &str,
    layer: &str,
    write: bool,
    library: &Path,
) -> Result<(Vec<String>, Vec<bool>, Vec<u32>)> {
    let count = dol::u32(tpl, 4)? as usize;
    let mut paths = Vec::with_capacity(count);
    let mut alpha = Vec::with_capacity(count);
    let mut heights = Vec::with_capacity(count);
    for index in 0..count {
        let (ktx, transparent) = world_texture::tpl_texture(tpl, index)?;
        let path = format!("{base}/textures/{layer}-texture-{index}.ktx2");
        heights.push(u32::from_le_bytes(ktx[24..28].try_into()?));
        if write {
            crate::output::write(&library.join(&path), &ktx)?;
        }
        paths.push(path);
        alpha.push(transparent);
    }
    Ok((paths, alpha, heights))
}

fn first_motion(package: &[u8]) -> Result<Option<Vec<u8>>> {
    let count = dol::u32(package, 0)? as usize;
    for index in 2..count.saturating_sub(1) {
        if let Some(source) = field_core::optional_section(package, index)? {
            return Ok(Some(if dol::u32(source, 0)? == 0x007b_7960 {
                source.to_vec()
            } else {
                compression::decode(source)?
            }));
        }
    }
    Ok(None)
}

fn publish_motion(
    library: &Path,
    base: &str,
    glb: Vec<u8>,
    motions: &mut BTreeMap<Vec<u8>, String>,
    preferred: Option<&str>,
    filename: &str,
) -> Result<String> {
    if let Some(path) = preferred {
        if path.starts_with(&format!("{base}/animations/")) {
            crate::output::write(&library.join(path), &glb)?;
        } else {
            ensure!(
                library.join(path).is_file(),
                "figurine idle {path} is missing"
            );
        }
        motions.entry(glb).or_insert_with(|| path.to_owned());
        return Ok(path.to_owned());
    }
    if let Some(path) = motions.get(&glb) {
        return Ok(path.clone());
    }
    let path = format!("{base}/animations/{filename}.glb");
    crate::output::write(&library.join(&path), &glb)?;
    motions.insert(glb, path.clone());
    Ok(path)
}

fn preview(
    id: u16,
    exe: &[u8],
    row: &[u8],
    package: &[u8],
    base: &str,
    shared: Option<&str>,
    library: &Path,
    motions: &mut BTreeMap<Vec<u8>, String>,
) -> Result<ModelPreview> {
    let body = field_core::optional_section(package, 0)?.context("figurine has no body")?;
    let outline = field_core::optional_section(package, 1)?;
    let motion = first_motion(package)?;
    let media = shared
        .map(|name| format!("meshes/{name}"))
        .unwrap_or_else(|| base.to_owned());
    let own_media = shared.is_none();
    let party_index = party_variant(id).map(|(character, _)| u16::from(character - 1));
    let party = party_index
        .map(|index| {
            PartyCharacterId::try_from((index + 1) as u8)?
                .resource_name()
                .context("party figurine has no character name")
        })
        .transpose()?;
    let party_idle = party_variant(id)
        .map(|(character, variant)| party_idle(library, character, variant))
        .transpose()?
        .flatten();
    let mut parts: Vec<PreviewPart> = Vec::new();
    let mut animation_path: Option<String> = None;
    let mut body_motion: Option<Vec<u8>> = None;
    let mut selector = Vec::new();
    let mut rows = 0;
    for (index, source) in [Some(body), outline].into_iter().enumerate() {
        let Some(source) = source else { continue };
        let own_palette = dol::u32(source, 0)? != 0;
        let normalized = if own_palette {
            source.to_vec()
        } else {
            world_gpl::with_primary_palette(body, source)?
        };
        let layer = if index == 0 { "body" } else { "outline" };
        let palette_source = if own_palette { source } else { body };
        let (paths, alpha, heights) = texture_paths(
            palette(palette_source)?,
            &media,
            if own_palette { layer } else { "body" },
            own_media && own_palette,
            library,
        )?;
        let paths = if shared.is_some() {
            paths
                .into_iter()
                .map(|path| {
                    if library.join(&path).is_file() {
                        path
                    } else {
                        let body = path.replace("/outline-texture-", "/body-texture-");
                        if library.join(&body).is_file() {
                            body
                        } else {
                            path
                        }
                    }
                })
                .collect()
        } else {
            paths
        };
        let chosen = source
            .get(0x0e)
            .copied()
            .and_then(|slot| slot.checked_sub(1))
            .map(usize::from);
        if let Some(chosen) = chosen {
            ensure!(
                chosen < paths.len(),
                "figurine appearance selector exceeds palette"
            );
        }
        if index == 0 {
            rows = chosen
                .map(|slot| match heights[slot] {
                    512 => 2,
                    1024 => 4,
                    _ => 0,
                })
                .unwrap_or(0);
        }
        selector.push(chosen);
        let (mesh, mut scene) = world_gpl::static_model(&normalized, paths, alpha)?;
        scene.mesh = format!("{media}/meshes/{layer}.glb");
        scene.resource = index as u16;
        if own_media {
            crate::output::write(&library.join(&scene.mesh), &mesh)?;
        }
        scene.prepare_bones(&mesh)?;
        if index == 0 {
            let translucent_accents = matches!(party_variant(id), Some((1, 2) | (7, 4)));
            for material in &mut scene.materials {
                if !translucent_accents && material.color.is_none() && material.multiply.is_none() {
                    material.blend = false;
                }
            }
            if let Some(name) = shared {
                let model: resonance_content::field::document::Model = serde_json::from_slice(
                    &fs::read(library.join(format!("meshes/{name}/model.json")))?,
                )?;
                scene.secondary_motion = model
                    .parts
                    .first()
                    .context("shared figurine body has no part")?
                    .secondary_motion
                    .clone();
            } else if let Some(label) = party {
                let json_len =
                    u32::from_le_bytes(mesh.get(12..16).context("short figurine GLB")?.try_into()?)
                        as usize;
                let gltf: serde_json::Value = serde_json::from_slice(
                    mesh.get(20..20 + json_len)
                        .context("short figurine GLB JSON")?,
                )?;
                let names = world_gpl::animation_bindings(body)?
                    .into_iter()
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>();
                let tag = if let Some(index) = party_index {
                    [
                        "llo00", "col00", "gen00", "ref00", "shi00", "zel00", "pre00", "reg00",
                        "kra00",
                    ][index as usize]
                } else {
                    label
                };
                scene.secondary_motion = character_models::character_dynamics::bind(
                    tag,
                    &gltf,
                    &names,
                    &scene.bone_names,
                )?;
            }
        } else {
            scene.secondary_motion = parts[0].scene.secondary_motion.clone();
        }
        if index == 1 {
            scene.outline_color = Some([0, 0, 0, 127]);
            for material in &mut scene.materials {
                material.cull = CullFace::Front;
                material.blend = true;
                material.draw_order += 65536;
            }
        }
        if let Some(motion) = &motion {
            let bindings = world_gpl::animation_bindings(&normalized)?;
            let pose_chains = if scene.secondary_motion.chains.is_empty() {
                Vec::new()
            } else {
                character_models::pose_chains(
                    motion,
                    &bindings,
                    &scene.bone_names,
                    &scene.secondary_motion,
                )?
            };
            let glb = world_motion::encode_bound(motion, &mesh, &bindings)?;
            let path = if index == 1 && body_motion.as_ref() == Some(&glb) {
                animation_path
                    .as_ref()
                    .context("figurine body has no motion path")?
                    .clone()
            } else {
                let preferred = if index == 0 {
                    party_idle.as_deref()
                } else {
                    None
                };
                let filename = if index == 0 { "idle" } else { "outline-idle" };
                publish_motion(library, base, glb.clone(), motions, preferred, filename)?
            };
            if index == 0 {
                body_motion = Some(glb);
                animation_path = Some(path.clone());
            }
            scene.clips.push(SceneClip {
                motion: path,
                joints: BTreeMap::new(),
                resource_slot: 0,
                duration_seconds: 0.,
                animation_resource: None,
                pose_chains,
            });
            scene.autoplay = true;
        }
        parts.push(PreviewPart {
            animation: motion.as_ref().map(|_| 0),
            scene,
            attached_to: None,
            additive: false,
            uv_offsets: Vec::new(),
        });
    }
    let appearance_row = dol::u32(row, 12)?;
    if rows > 0 && appearance_row > 0 && appearance_row < rows {
        let shift = appearance_row as f32 / rows as f32;
        for (part, chosen) in parts.iter_mut().zip(selector) {
            if let Some(chosen) = chosen {
                part.uv_offsets = part
                    .scene
                    .materials
                    .iter()
                    .map(|material| {
                        let y = |binding: &Option<resonance_content::TextureBinding>| {
                            binding
                                .as_ref()
                                .is_some_and(|b| b.texture == chosen)
                                .then_some(shift)
                                .unwrap_or(0.)
                        };
                        [0., y(&material.color), 0., y(&material.multiply)]
                    })
                    .collect();
            }
        }
    }
    let prefix = std::str::from_utf8(dol::slice(exe, 0x8035_d3b8, 2)?)?;
    let mut hidden: BTreeSet<String> = parts[0]
        .scene
        .bone_names
        .iter()
        .filter(|name| name.starts_with(prefix))
        .cloned()
        .collect();
    for pointer in row[16..].chunks_exact(4) {
        let pointer = dol::u32(pointer, 0)?;
        if pointer == 0 {
            continue;
        }
        let rule = text::string(exe, pointer)?;
        let target = rule.strip_prefix('-').unwrap_or(&rule);
        for name in parts[0]
            .scene
            .bone_names
            .iter()
            .filter(|name| name.starts_with(target))
        {
            if rule.starts_with('-') {
                hidden.insert(name.clone());
            } else {
                hidden.remove(name);
            }
        }
    }
    let elevation = f32::from_bits(dol::u32(dol::slice(exe, 0x8035_d350, 4)?, 0)?);
    let behavior = (id == 80).then(|| resonance_content::model_behavior::ModelBehaviorBinding {
        module: "preview::efreet".into(),
        function: "apply".into(),
    });
    let result = ModelPreview {
        document: None,
        scale: 1.,
        elevation,
        parts,
        hidden_geometry: hidden.into_iter().collect(),
        behavior,
    };
    result.validate_definition()?;
    Ok(result)
}

/// Convert one native figurine or the complete 288-entry preview catalogue.
pub fn cook_figurine_media(extracted: &Path, library: &Path, selected: Option<u16>) -> Result<()> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let records = dol::slice(&exe, RECORDS, COUNT * STRIDE)?;
    let bank = native_archive(extracted, &exe)?;
    let shared: BTreeMap<u16, String> = serde_json::from_str(include_str!("figurine-bodies.json"))?;
    let mut owners = BTreeMap::<u32, String>::new();
    let mut motions = BTreeMap::<Vec<u8>, String>::new();
    if let Some(id) = selected {
        ensure!((id as usize) < COUNT, "figurine ID exceeds catalogue");
        for earlier in 0..id {
            let name = FigurineId::try_from(earlier)?
                .resource_name()
                .context("unnamed figurine")?;
            for filename in ["idle", "outline-idle"] {
                let path = format!("meshes/figurines/{name}/animations/{filename}.glb");
                if library.join(&path).is_file() {
                    motions
                        .entry(fs::read(library.join(&path))?)
                        .or_insert(path);
                }
            }
        }
    }
    let ids: Box<dyn Iterator<Item = usize>> = match selected {
        Some(id) => Box::new(std::iter::once(id as usize)),
        None => Box::new(0..COUNT),
    };
    for id in ids {
        let row = &records[id * STRIDE..][..STRIDE];
        let selector = dol::u32(row, 8)?;
        let native = native_model(&bank, selector)?;
        let key = selector & 0xffff;
        let name = FigurineId::try_from(id as u16)?
            .resource_name()
            .context("unnamed figurine")?;
        let base = format!("meshes/figurines/{name}");
        let owner = shared
            .get(&(id as u16))
            .map(String::as_str)
            .or_else(|| owners.get(&key).map(String::as_str));
        let physical = owner
            .map(str::to_owned)
            .unwrap_or_else(|| format!("figurines/{name}"));
        let package = payload(native.to_vec())?;
        let model = preview(
            id as u16,
            &exe,
            row,
            &package,
            &base,
            owner,
            library,
            &mut motions,
        )
        .with_context(|| format!("figurine {id} {name}"))?;
        crate::output::json(&library.join(format!("{base}/model.json")), &model)?;
        owners.entry(key).or_insert(physical);
    }
    Ok(())
}
