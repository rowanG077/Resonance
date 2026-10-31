//! Party field models and their reusable editable motions.
#[path = "character_dynamics/mod.rs"]
pub(crate) mod character_dynamics;

use crate::{
    compression, dol, field_core,
    native_archive::{member, read_named},
    text, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, AtlasChannel, CullFace, SceneClip, ScenePart,
    field::{
        FIELD_SERVICE_MOTION_RESOURCE_BASE, ServiceMotion,
        document::{Model, ModelAlias, ModelTarget},
    },
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_constants::PartyCharacterId;

#[derive(Deserialize)]
struct Costume {
    name: String,
    body: Option<String>,
}

pub(crate) fn party_body_name(character: u8, variant: u8) -> Result<String> {
    let costumes: BTreeMap<u8, BTreeMap<u8, Costume>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    let costume = costumes
        .get(&character)
        .and_then(|row| row.get(&variant))
        .context("unnamed party costume")?;
    Ok(costume.body.as_deref().unwrap_or(&costume.name).to_owned())
}

pub(crate) fn body_materials(part: &mut ScenePart, character: u8, variant: u8) {
    // These two outfits blend their untextured accent draws.
    if matches!((character, variant), (1, 2) | (7, 4)) {
        return;
    }
    for material in &mut part.materials {
        if material.color.is_none() && material.multiply.is_none() {
            material.blend = false;
        }
    }
}

/// The native party body, used when a field animates that body as scenery.
pub(crate) fn party_package(extracted: &Path, character: u8, variant: u8) -> Result<Vec<u8>> {
    ensure!(
        (1..=9).contains(&character) && variant < 5,
        "party body outside native catalogue"
    );
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let pointer = dol::u32(
        dol::slice(
            &exe,
            0x801f_ace4 + u32::from(character - 1) * 20 + u32::from(variant) * 4,
            4,
        )?,
        0,
    )?;
    let filename = text::string(&exe, pointer)?;
    read_named(&extracted.join("disc1/files"), &filename)
}

pub(crate) fn party_body(extracted: &Path, character: u8, variant: u8) -> Result<Vec<u8>> {
    let package = party_package(extracted, character, variant)?;
    let body = member(&package, 0, 2)?.context("missing party body")?;
    Ok(body.to_vec())
}

/// Native joint IDs and source names for a party body.
pub(crate) fn party_joint_bindings(
    extracted: &Path,
    character: u8,
    variant: u8,
) -> Result<Vec<(u16, String)>> {
    world_gpl::animation_bindings(&party_body(extracted, character, variant)?)
}

/// Party IDs and costume aliases for the shared mesh mapping document.
pub fn party_model_mappings() -> Result<BTreeMap<u32, ModelTarget>> {
    let costumes: BTreeMap<u8, BTreeMap<u8, Costume>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    let mut mappings = BTreeMap::new();
    for id in 1_u8..=9 {
        let name = PartyCharacterId::try_from(id)?
            .resource_name()
            .context("unnamed party character")?;
        let base = format!("characters/{name}");
        mappings.insert(u32::from(id), ModelTarget::Model(base.clone()));
        let mut seen = BTreeSet::from(["default"]);
        for (&variant, costume) in &costumes[&id] {
            if variant == 0 {
                continue;
            }
            let owner = costume.body.as_deref().unwrap_or(&costume.name);
            let target = if owner == "default" {
                base.clone()
            } else {
                format!("{base}/{owner}")
            };
            let alias = variant == 3 && (costume.body.is_some() || seen.contains(owner));
            let model = if alias {
                ModelTarget::Alias(ModelAlias {
                    name: format!("{base}/story"),
                    model: target,
                })
            } else {
                ModelTarget::Model(target)
            };
            seen.insert(owner);
            mappings.insert(
                resonance_content::appearance::costume_resource(u32::from(id), variant),
                model,
            );
        }
    }
    Ok(mappings)
}

fn inline_name(exe: &[u8], address: u32, width: usize) -> Result<String> {
    let row = dol::slice(exe, address, width)?;
    let end = row
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated resource name")?;
    Ok(std::str::from_utf8(&row[..end])?.to_owned())
}

pub(crate) fn palette(model: &[u8]) -> Result<&[u8]> {
    let start = dol::u32(model, 0)? as usize;
    let end = dol::u32(model, 4)? as usize;
    let tpl = model
        .get(start..end)
        .context("character palette exceeds model")?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing character TPL");
    Ok(tpl)
}

pub(crate) fn texture_paths(
    tpl: &[u8],
    base: &str,
    layer: &str,
    mut emit: impl FnMut(String, Vec<u8>),
) -> Result<(Vec<String>, Vec<bool>, Vec<(u32, u32)>)> {
    let count = dol::u32(tpl, 4)? as usize;
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    let mut sizes = Vec::new();
    for index in 0..count {
        let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
        let path = format!("{base}/textures/{layer}-texture-{index}.ktx2");
        paths.push(path.clone());
        alpha.push(transparent);
        // KTX2 stores pixelWidth and pixelHeight in its fixed header.
        sizes.push((
            u32::from_le_bytes(texture[20..24].try_into()?),
            u32::from_le_bytes(texture[24..28].try_into()?),
        ));
        emit(path, texture);
    }
    Ok((paths, alpha, sizes))
}

pub(crate) fn appearance(model: &[u8], sizes: &[(u32, u32)]) -> Result<Option<AppearanceTextures>> {
    let mut slots = [None; 14];
    for (index, &value) in model
        .get(0x0c..0x19)
        .context("missing model selectors")?
        .iter()
        .enumerate()
    {
        slots[index + usize::from(index >= 3)] = value.checked_sub(1).map(usize::from);
    }
    ensure!(
        slots.iter().flatten().all(|&index| index < sizes.len()),
        "character selector exceeds texture palette"
    );
    let variant = slots[2].and_then(|index| {
        let (width, height) = sizes[index];
        let frames = match (width == 128, height) {
            (true, 256) | (false, 512) => 2,
            (true, 512) | (false, 1024) => 4,
            _ => return None,
        };
        Some(AtlasChannel {
            texture: index,
            frames,
        })
    });
    let appearance = AppearanceTextures {
        eyes: slots[0],
        mouth: slots[1],
        variant,
        costume: slots[4],
    };
    Ok((appearance.eyes.is_some()
        || appearance.mouth.is_some()
        || appearance.variant.is_some()
        || appearance.costume.is_some())
    .then_some(appearance))
}

fn clip(path: String, slot: u16, bank: Option<u32>) -> SceneClip {
    SceneClip {
        motion: path,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: bank,
        pose_chains: Vec::new(),
    }
}

fn motion_name(slot: u16, service: bool) -> String {
    match (slot, service) {
        (12, false) => "idle".into(),
        (36, false) => "walk".into(),
        (40, false) => "run".into(),
        (20, true) => "field-service-open-door".into(),
        (24, true) => "field-service-pull-door".into(),
        (32, true) => "field-service-hold-block".into(),
        (36, true) => "field-service-push-block".into(),
        (40, true) => "field-service-pull-block".into(),
        (52, true) => "field-service-cast-ring".into(),
        _ => format!("motion-{slot}"),
    }
}

/// Compare authored curves without the costume's slightly different bind pose.
pub(crate) fn motion_signature(glb: &[u8]) -> Result<Vec<u8>> {
    ensure!(glb.get(..4) == Some(b"glTF"), "invalid motion GLB");
    let json_len = u32::from_le_bytes(glb.get(12..16).context("short GLB")?.try_into()?) as usize;
    let json: serde_json::Value =
        serde_json::from_slice(glb.get(20..20 + json_len).context("short GLB JSON")?)?;
    let nodes = json["nodes"].as_array().context("motion has no nodes")?;
    let channels = json["animations"][0]["channels"]
        .as_array()
        .context("motion has no channels")?;
    let targets = channels
        .iter()
        .map(|channel| -> Result<&str> {
            let index = channel["target"]["node"]
                .as_u64()
                .context("motion channel lacks node")? as usize;
            nodes
                .get(index)
                .and_then(|node| node["name"].as_str())
                .context("unnamed motion target")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut signature = serde_json::to_vec(&serde_json::json!({
        "animations": json["animations"],
        "accessors": json["accessors"],
        "bufferViews": json["bufferViews"],
        "targets": targets,
    }))?;
    let chunk = 20 + json_len;
    let bin_len = u32::from_le_bytes(
        glb.get(chunk..chunk + 4)
            .context("short GLB binary")?
            .try_into()?,
    ) as usize;
    signature.extend_from_slice(
        glb.get(chunk + 8..chunk + 8 + bin_len)
            .context("short GLB binary data")?,
    );
    Ok(signature)
}

pub(crate) fn pose_chains(
    native: &[u8],
    bindings: &[(u16, String)],
    friendly: &[String],
    dynamics: &resonance_content::secondary_motion::Definition,
) -> Result<Vec<String>> {
    let count = usize::from(dol::u16(native, 12)?);
    let names = (|| -> Result<Vec<String>> {
        ensure!(
            dol::u32(native, 16)? != 0 && dol::u32(native, 20)? != 0,
            "motion has no name table"
        );
        let mut cursor = dol::u32(native, 20)? as usize;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            let tail = native
                .get(cursor..)
                .context("motion joint names exceed source")?;
            let end = tail
                .iter()
                .position(|&byte| byte == 0)
                .context("unterminated motion joint name")?;
            let (label, invalid) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(&tail[..end]);
            ensure!(!invalid, "invalid native motion joint name");
            names.push(label.into_owned());
            cursor += end + 1;
        }
        Ok(names)
    })()
    .ok();
    // A malformed nominal table does not invalidate the track IDs.
    let use_names = names.as_ref().is_some_and(|names| {
        names
            .iter()
            .any(|name| bindings.iter().any(|(_, joint)| joint == name))
    });
    let animated = (0..count)
        .filter(|&track| dol::u16(native, 36 + track * 16 + 8).is_ok_and(|keys| keys > 2))
        .filter_map(|track| {
            if use_names {
                let name = names.as_ref()?.get(track)?;
                bindings.iter().position(|(_, joint)| joint == name)
            } else {
                let id = dol::u16(native, 36 + track * 16 + 10).ok()?;
                bindings.iter().position(|(binding, _)| *binding == id)
            }
        })
        .filter_map(|index| friendly.get(index).map(String::as_str))
        .collect::<BTreeSet<_>>();
    let mut selected = dynamics
        .chains
        .iter()
        .filter(|chain| {
            chain
                .joints
                .first()
                .is_some_and(|root| animated.contains(root.as_str()))
        })
        .map(|chain| chain.name.clone())
        .collect::<Vec<_>>();
    selected.sort();
    Ok(selected)
}

fn write(files: BTreeMap<String, Vec<u8>>, library: &Path) -> Result<()> {
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}

/// One native body owner writes its meshes, textures, motions and model metadata.
pub fn cook_character_models(root: &Path, library: &Path, selected: Option<u8>) -> Result<()> {
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let costumes: BTreeMap<u8, BTreeMap<u8, Costume>> =
        serde_json::from_str(include_str!("costume-names.json"))?;
    if let Some(id) = selected {
        ensure!((1..=9).contains(&id), "invalid party character {id}");
    }
    for id in 1_u8..=9 {
        if selected.is_some_and(|wanted| wanted != id) {
            continue;
        }
        let mut files = BTreeMap::new();
        let name = PartyCharacterId::try_from(id)?
            .resource_name()
            .context("unnamed party character")?;
        let motions_name = inline_name(&exe, 0x8017_e4c0 + u32::from(id - 1) * 12, 12)?;
        let service_name = inline_name(&exe, 0x8017_a33c + u32::from(id - 1) * 16, 16)?;
        let motions = read_named(&root.join("disc1/files"), &motions_name)?;
        let services = read_named(&root.join("disc1/files"), &service_name)?;
        let mut seen_bodies = BTreeSet::new();
        let mut shared_motions: Vec<(Vec<u8>, String)> = Vec::new();
        for variant in 0..5_u8 {
            let Some(costume) = costumes.get(&id).and_then(|row| row.get(&variant)) else {
                continue;
            };
            let pointer = dol::u32(
                dol::slice(
                    &exe,
                    0x801f_ace4 + u32::from(id - 1) * 20 + u32::from(variant) * 4,
                    4,
                )?,
                0,
            )?;
            let filename = text::string(&exe, pointer)?;
            if !seen_bodies.insert(filename.clone()) {
                continue;
            }
            ensure!(
                costume.body.is_none(),
                "distinct costume aliases another body"
            );
            let package = read_named(&root.join("disc1/files"), &filename)?;
            let body = member(&package, 0, 2)?.context("missing character body")?;
            let outline = member(&package, 1, 2)?.context("missing character outline")?;
            let base = if variant == 0 {
                format!("meshes/characters/{name}")
            } else {
                format!("meshes/characters/{name}/{}", costume.name)
            };
            let (body_paths, body_alpha, sizes) =
                texture_paths(palette(body)?, &base, "body", |path, bytes| {
                    files.insert(path, bytes);
                })?;
            let (body_glb, mut body_part) =
                world_gpl::static_model(body, body_paths.clone(), body_alpha.clone())?;
            body_part.mesh = format!("{base}/meshes/body.glb");
            files.insert(body_part.mesh.clone(), body_glb.clone());
            body_materials(&mut body_part, id, variant);
            body_part.appearance = appearance(body, &sizes)?;
            body_part.prepare_bones(&body_glb)?;
            let bindings = world_gpl::animation_bindings(body)?;
            let source_names = bindings
                .iter()
                .map(|(_, name)| name.clone())
                .collect::<Vec<_>>();
            let json_len =
                u32::from_le_bytes(body_glb.get(12..16).context("short body GLB")?.try_into()?)
                    as usize;
            let body_json: serde_json::Value = serde_json::from_slice(
                body_glb
                    .get(20..20 + json_len)
                    .context("short body GLB JSON")?,
            )?;
            let tag = [
                "llo00", "col00", "gen00", "ref00", "shi00", "zel00", "pre00", "reg00", "kra00",
            ][usize::from(id - 1)];
            let dynamics =
                character_dynamics::bind(tag, &body_json, &source_names, &body_part.bone_names)?;
            body_part.secondary_motion = dynamics.clone();

            let normalized_outline = if dol::u32(outline, 0)? == 0 {
                world_gpl::with_primary_palette(body, outline)?
            } else {
                outline.to_vec()
            };
            let own_outline = dol::u32(outline, 0)? != 0;
            let (outline_paths, outline_alpha, _) = if own_outline {
                texture_paths(
                    palette(&normalized_outline)?,
                    &base,
                    "outline",
                    |path, bytes| {
                        files.insert(path, bytes);
                    },
                )?
            } else {
                (body_paths, body_alpha, sizes)
            };
            let (outline_glb, mut outline_part) =
                world_gpl::static_model(&normalized_outline, outline_paths, outline_alpha)?;
            outline_part.mesh = format!("{base}/meshes/outline.glb");
            files.insert(outline_part.mesh.clone(), outline_glb);
            outline_part.resource = 1;
            outline_part.outline_color = Some([0, 0, 0, 127]);
            outline_part.secondary_motion = dynamics.clone();
            for material in &mut outline_part.materials {
                material.cull = CullFace::Front;
                material.blend = true;
                material.draw_order += 65536;
            }

            let mut requests = Vec::new();
            for source_slot in 2..31 {
                if let Some(bytes) = member(&motions, source_slot, 31)? {
                    requests.push((4 + source_slot as u16 * 4, None, false, bytes));
                }
            }
            for motion in ServiceMotion::ALL {
                let slot = motion as u16;
                let source_slot = usize::from((slot - 4) / 4);
                if let Some(bytes) = field_core::optional_section(&services, source_slot)? {
                    requests.push((
                        slot,
                        Some(FIELD_SERVICE_MOTION_RESOURCE_BASE + u32::from(id)),
                        true,
                        bytes,
                    ));
                }
            }
            let mut used_slots = BTreeSet::new();
            let mut local_motions: Vec<(Vec<u8>, String)> = Vec::new();
            for (slot, bank, service, native) in requests {
                ensure!(
                    used_slots.insert((slot, bank)),
                    "duplicate party motion request"
                );
                let expanded = if dol::u32(native, 0)? == 0x007b_7960 {
                    native.to_vec()
                } else {
                    compression::decode(native)?
                };
                let encoded = world_motion::encode_bound(&expanded, &body_glb, &bindings)
                    .with_context(|| {
                        format!("convert {name} costume {} motion {slot}", costume.name)
                    })?;
                let selected_chains =
                    pose_chains(&expanded, &bindings, &body_part.bone_names, &dynamics)?;
                let signature = motion_signature(&encoded)?;
                let path = if let Some((_, path)) = local_motions
                    .iter()
                    .chain(shared_motions.iter())
                    .find(|(bytes, _)| bytes == &signature)
                {
                    path.clone()
                } else {
                    let label = motion_name(slot, service);
                    let mut path = format!("{base}/animations/{label}.glb");
                    let mut suffix = 2;
                    while files.contains_key(&path) {
                        path = format!("{base}/animations/{label}-variant-{suffix}.glb");
                        suffix += 1;
                    }
                    files.insert(path.clone(), encoded.clone());
                    local_motions.push((signature, path.clone()));
                    path
                };
                let mut binding = clip(path, slot, bank);
                binding.pose_chains = selected_chains;
                body_part.clips.push(binding.clone());
                outline_part.clips.push(binding);
            }
            shared_motions.extend(local_motions);
            let hidden_geometry = body_part
                .bone_names
                .iter()
                .filter(|joint| joint.starts_with("kk"))
                .cloned()
                .collect();
            let document = Model {
                parts: vec![body_part, outline_part],
                collision: None,
                hidden_geometry,
            };
            files.insert(
                format!("{base}/model.json"),
                resonance_asset_writer::json::encode(&document)?,
            );
        }
        write(files, library)?;
    }
    Ok(())
}
