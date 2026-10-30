//! Field scenery from its four native model slots into GLB, KTX2 and scene.json.
use crate::{
    dol, field_actors, field_core, field_scene_banks, shared_animations, ui_emotions,
    ui_location_titles, world_cinematics, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_asset_writer::gltf;
use resonance_content::{SceneClip, field::DrawStage};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const LAYERS: [usize; 4] = [0, 10, 12, 2];

// These six fields reuse Altessa's house's native idle scene motion. Keep one
// editable GLB when the cooked clip really is identical; other releases may
// supply different motion and retain their field-local file instead.
fn shared_idle_owner(id: u32, section: usize) -> Option<&'static str> {
    (section == 0 && matches!(id, 144 | 145 | 146 | 290 | 397 | 446))
        .then_some("fields/altessas-house-0141/animations/motion-0.glb")
}

/// Debug menus resolve user-selected model IDs at runtime. Their scenery only
/// selects these fixed banks (544's Lloyd bank is a native call at PC 0x134c).
fn debug_scene_bank_ids(id: u32) -> Option<&'static [u32]> {
    match id {
        544 => Some(&[196608]),
        545 => Some(&[]),
        _ => None,
    }
}

fn annotate_doors(mesh: Vec<u8>) -> Result<Vec<u8>> {
    let word = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(
            mesh.get(at..at + 4)
                .context("truncated field GLB")?
                .try_into()?,
        ) as usize)
    };
    ensure!(
        word(0)? == 0x4654_6c67 && word(4)? == 2,
        "invalid field GLB"
    );
    let json_len = word(12)?;
    ensure!(word(16)? == 0x4e4f_534a, "field GLB lacks JSON");
    let binary_at = 20 + json_len;
    let binary_len = word(binary_at)?;
    ensure!(
        word(binary_at + 4)? == 0x004e_4942,
        "field GLB lacks geometry"
    );
    let binary = mesh
        .get(binary_at + 8..binary_at + 8 + binary_len)
        .context("truncated field geometry")?;
    let mut gltf: Value = serde_json::from_slice(&mesh[20..binary_at])?;
    for node in gltf["nodes"]
        .as_array_mut()
        .context("field GLB lacks nodes")?
    {
        if node.get("mesh").is_some() || node["extras"]["resonance_bone"].is_null() {
            continue;
        }
        let Some(name) = node["name"].as_str() else {
            continue;
        };
        if !name.starts_with("DOOR") || !name.contains("_AUTO") {
            continue;
        }
        let (name, suffix) = name.rsplit_once('-').unwrap_or((name, ""));
        let name = if suffix.parse::<u16>().is_ok_and(|n| n >= 2)
            && name
                .split_once("_W")
                .is_some_and(|(_, width)| !width.is_empty())
        {
            name
        } else {
            node["name"].as_str().unwrap()
        };
        let width = name.split_once("_W").map_or(Ok(200.), |(_, value)| {
            value
                .split('_')
                .next()
                .unwrap()
                .replace('e', ".")
                .parse::<f32>()
        })?;
        ensure!(
            width.is_finite() && width.abs() <= 2000.,
            "invalid door width"
        );
        let pull = name.contains("_PULL");
        ensure!(
            pull != name.contains("_PUSH"),
            "door lacks push/pull direction"
        );
        let distance = if pull && width < 0. {
            -45.287_f32
        } else {
            -55.287_f32
        };
        let angle = if name.contains("_AUTO_L") { 45. } else { 30. }
            * if pull { 1. } else { -1. }
            * if width < 0. { -1. } else { 1. };
        node["extras"]["door_approach"] = json!([-0.7 * width, distance, 0.]);
        node["extras"]["door_action"] = json!(if pull { "pull" } else { "push" });
        node["extras"]["door_angle_degrees"] = json!(angle);
    }
    gltf::pack_glb(&gltf, binary)
}

fn texture_animations(id: u32) -> Result<Option<Value>> {
    // UV timing is authored gameplay behavior, absent from native GPL geometry.
    // Identical rules share one entry in this maintained source catalogue.
    #[derive(serde::Deserialize)]
    struct Profile {
        fields: Vec<u32>,
        tracks: Vec<Value>,
    }
    let profiles: Vec<Profile> =
        serde_json::from_str(include_str!("field-texture-animations.json"))?;
    let mut selected = None;
    for profile in profiles {
        if profile.fields.contains(&id) {
            ensure!(selected.is_none(), "field {id} has duplicate UV recipes");
            selected = Some(Value::Array(profile.tracks));
        }
    }
    Ok(selected)
}

fn camera_tracks(
    archive: &[u8],
    id: u32,
    root: &str,
    library: &Path,
) -> Result<(BTreeMap<u32, String>, Vec<(String, Vec<u8>)>)> {
    let count = dol::u32(archive, 0)? as usize;
    let mut cameras = Vec::new();
    for section in 16..count {
        if let Some(source) = field_core::optional_section(archive, section)?
            && source.starts_with(b"CAMM")
        {
            cameras.push((section, source));
        }
    }
    let mut paths = BTreeMap::new();
    let mut files = Vec::new();
    for (ordinal, &(section, source)) in cameras.iter().enumerate() {
        let title = matches!(id, 474 | 531);
        let owner = if title { "ui/title" } else { root };
        let path = if cameras.len() == 1 {
            format!("{owner}/camera.glb")
        } else {
            format!("{owner}/camera-{}.glb", ordinal + 1)
        };
        let encoded = world_cinematics::camera(source)
            .with_context(|| format!("field {id} camera section {section}"))?;
        if title {
            ensure!(
                fs::read(library.join(&path)).ok().as_deref() == Some(encoded.as_slice()),
                "field {id} needs matching title camera {path}"
            );
        } else {
            files.push((path.clone(), encoded));
        }
        paths.insert(0xffee_0000 + (section - 16) as u32, path);
    }
    Ok((paths, files))
}

fn panorama(archive: &[u8], id: u32, root: &str) -> Result<Option<(String, Vec<u8>)>> {
    let Some(source) = field_core::optional_section(archive, 15)? else {
        return Ok(None);
    };
    if dol::u32(source, 0)? != 0x0020_af30 {
        ensure!(
            matches!(id, 184 | 475),
            "field {id} has an unsupported section 15"
        );
        return Ok(None);
    }
    let path = format!("{root}/textures/panorama.ktx2");
    let (image, _) =
        world_texture::tpl_texture(source, 0).with_context(|| format!("field {id} panorama"))?;
    Ok(Some((path, image)))
}

fn environment(id: u32) -> Result<Option<Value>> {
    // Gameplay atmosphere is authored outside the native field archive. The
    // shared defaults remain in fields/settings.json; these are field choices.
    #[derive(serde::Deserialize)]
    struct Profile {
        fields: Vec<u32>,
        environment: Value,
    }
    let profiles: Vec<Profile> = serde_json::from_str(include_str!("field-environments.json"))?;
    let mut selected = None;
    for profile in profiles {
        if profile.fields.contains(&id) {
            ensure!(
                selected.is_none(),
                "field {id} has duplicate environment recipes"
            );
            selected = Some(profile.environment);
        }
    }
    Ok(selected)
}

fn water(id: u32) -> Option<Value> {
    if !(519..=521).contains(&id) {
        return None;
    }
    // Ymir's reflection plane and tint are authored gameplay parameters;
    // the original MAP contains no water record. Field 520 adds one actor.
    let mut reflected_actors = vec![999_997, 3002, 3];
    if id == 520 {
        reflected_actors.push(3500);
    }
    Some(json!({
        "height":-70.0,
        "bounds":[[-7000.0,-4500.0],[4100.0,7000.0]],
        "tint":[2.0078433,2.509804,2.0078433,0.2509804],
        "reflected_actors":reflected_actors,
        "reflection_only_actors":[999_980]
    }))
}

fn unbound_geometry(
    archive: &[u8],
    id: u32,
    root: &str,
) -> Result<Option<(Value, Vec<(String, Vec<u8>)>)>> {
    if !matches!(id, 125 | 499) {
        return Ok(None);
    }
    let section = field_core::optional_section(archive, 16)?
        .with_context(|| format!("field {id} has no caller-textured resource"))?;
    let body = if dol::u32(section, 0)? == 31 {
        field_core::optional_section(section, 0)?.context("unbound actor has no body")?
    } else {
        section
    };
    ensure!(dol::u32(body, 0)? == 0, "unbound model owns a palette");
    let end = match dol::u32(body, 4)? as usize {
        0 => body.len(),
        end => end,
    };
    ensure!(
        (0x20..=body.len()).contains(&end),
        "invalid unbound GPL boundary"
    );
    let mut normalized = body.to_vec();
    normalized[..4].copy_from_slice(&(end as u32).to_be_bytes());
    let base = format!("{root}/models/caller-textured");
    let mesh_path = format!("{base}/mesh.glb");
    let scene_path = format!("{base}/scene.json");
    let (mesh, scene) = world_gpl::unbound_model(&normalized, &mesh_path)
        .with_context(|| format!("field {id} caller-textured GPL"))?;
    let files = vec![
        (mesh_path, mesh),
        (
            scene_path.clone(),
            resonance_asset_writer::json::encode(&scene)?,
        ),
    ];
    Ok(Some((
        json!([{"resource":0xffee_0000_u32,"scenes":[scene_path]}]),
        files,
    )))
}

fn local_overlay_path(id: u32, section: usize) -> Option<String> {
    if let Some(path) = ui_location_titles::title_path(id, section) {
        return Some(path);
    }
    match (id, section) {
        (77, 16) => Some(resonance_content::input_prompts::FIELD_BANK_PATH.into()),
        (354, 20..=29) => ui_emotions::picture_path(section),
        (171, 23) => Some("effects/overlays/flames.json".into()),
        (474 | 531, 19) => Some("ui/title/title-screen.ktx2".into()),
        (511..=518, 17) => Some("effects/overlays/gold-glow.ktx2".into()),
        (511..=518, 22) => Some("effects/overlays/gold-light-strip.json".into()),
        _ => None,
    }
}

fn global_overlay_path(id: u32) -> Option<&'static str> {
    match id {
        38 => Some("ui/location-titles/iselia-japanese.ktx2"),
        49 => Some("ui/ending/party-with-kratos.ktx2"),
        50 => Some("ui/ending/party-with-zelos.ktx2"),
        51 => Some("ui/ending/fin.json"),
        68611 => Some("effects/overlays/radial-burst.ktx2"),
        68614 => Some("effects/overlays/ornate-ring.json"),
        _ => None,
    }
}

fn overlay_paths(extracted: &Path, archive: &[u8], id: u32) -> Result<BTreeMap<i32, String>> {
    let mut paths = BTreeMap::new();
    let count = dol::u32(archive, 0)? as usize;
    for section in 16..count {
        let Some(source) = field_core::optional_section(archive, section)? else {
            continue;
        };
        if source.len() < 4 || dol::u32(source, 0)? != 0x0020_af30 {
            continue;
        }
        let resource = 0xffee_0000u32 + u32::try_from(section - 16)?;
        let path = local_overlay_path(id, section)
            .with_context(|| format!("field {id} overlay {resource:#x} has no name"))?;
        paths.insert(resource as i32, path);
    }
    // Debug menus' unresolved selectors choose models interactively; their
    // statically declared calls select no global scene overlay pictures.
    let global = if debug_scene_bank_ids(id).is_some() {
        BTreeSet::new()
    } else {
        field_actors::declared_resources(extracted, id)?
    };
    for resource in global {
        let Some(path) = global_overlay_path(resource) else {
            continue;
        };
        let source = field_actors::read_shared_resource(extracted, resource)?;
        ensure!(
            dol::u32(&source, 0)? == 0x0020_af30,
            "field {id} overlay {resource} is not a TPL"
        );
        paths.insert(i32::try_from(resource)?, path.into());
    }
    Ok(paths)
}

pub fn cook_field_scene(extracted: &Path, library: &Path, id: u32) -> Result<()> {
    let (exe, archive) = field_core::read(extracted, id)?;
    cook_from_source(extracted, library, id, &exe, &archive)
}

pub(crate) fn cook_from_source(
    extracted: &Path,
    library: &Path,
    id: u32,
    exe: &[u8],
    archive: &[u8],
) -> Result<()> {
    publish(library, prepare(extracted, library, id, exe, archive)?)
}

fn prepare(
    extracted: &Path,
    library: &Path,
    id: u32,
    exe: &[u8],
    archive: &[u8],
) -> Result<Vec<(String, Vec<u8>)>> {
    let texture_animations = texture_animations(id)?;
    let environment = environment(id)?;
    let water = water(id);
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let (cameras, camera_files) = camera_tracks(&archive, id, &root, library)?;
    let panorama = panorama(&archive, id, &root)?;
    let unbound = unbound_geometry(&archive, id, &root)?;
    let overlays = overlay_paths(extracted, &archive, id)?;
    let banks = if let Some(ids) = debug_scene_bank_ids(id) {
        let names: resonance_content::field::document::AnimationMappings = if ids.is_empty() {
            resonance_content::field::document::AnimationMappings(BTreeMap::new())
        } else {
            serde_json::from_slice(&fs::read(library.join("animations/mappings.json"))?)?
        };
        ids.iter()
            .map(|&bank| {
                Ok((
                    bank,
                    names
                        .target(bank)
                        .with_context(|| format!("debug field {id} lacks bank {bank}"))?
                        .to_owned(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?
    } else {
        field_actors::animation_banks(extracted, exe, archive)?
    };
    let scene_banks = if !banks.is_empty() {
        ensure!(
            library.join("animations/mappings.json").is_file(),
            "field {id} scene lacks animation mappings"
        );
        for &bank in banks.keys() {
            let path = shared_animations::mapped_motion_path(library, bank)?;
            ensure!(
                library.join(&path).is_file(),
                "field {id} scene lacks {path}"
            );
        }
        Some(banks)
    } else {
        None
    };
    let mut files = camera_files;
    if let Some((path, image)) = &panorama {
        files.push((path.clone(), image.clone()));
    }
    if let Some((_, unbound_files)) = &unbound {
        files.extend(unbound_files.clone());
    }
    let mut textures = Vec::<(Vec<u8>, String)>::new();
    let mut parts = Vec::new();
    for section in LAYERS {
        let Some(model) = field_core::optional_section(&archive, section)? else {
            continue;
        };
        let index = parts.len();
        let start = dol::u32(model, 0)? as usize;
        let end = dol::u32(model, 4)? as usize;
        let palette = model
            .get(start..end)
            .context("field palette exceeds model")?;
        // Some scenery GPLs have no palette at all (the flying dragon's
        // middle layer is one). Its zero palette pointer is native data.
        let count = if start == 0 {
            0
        } else {
            ensure!(dol::u32(palette, 0)? == 0x0020_af30, "missing field TPL");
            dol::u32(palette, 4)? as usize
        };
        let mut paths = Vec::with_capacity(count);
        let mut alpha = Vec::with_capacity(count);
        for texture in 0..count {
            let (bytes, blend) = world_texture::tpl_texture(palette, texture)
                .with_context(|| format!("field {id} section {section} texture {texture}"))?;
            let path = if let Some((_, path)) = textures
                .iter()
                .find(|(prior, _)| prior.as_slice() == bytes.as_slice())
            {
                path.clone()
            } else {
                let path = format!("{root}/textures/part-{index}-texture-{texture}.ktx2");
                textures.push((bytes.clone(), path.clone()));
                path
            };
            paths.push(path);
            alpha.push(blend);
        }
        let (mesh, mut part) = world_gpl::static_model(model, paths, alpha)
            .with_context(|| format!("field {id} scenery section {section}"))?;
        let mesh = if section == 0 {
            annotate_doors(mesh)?
        } else {
            mesh
        };
        let mesh_path = format!("{root}/meshes/part-{index}.glb");
        part.mesh = mesh_path.clone();
        part.resource = section as u16;
        let stage = DrawStage::scenery(section as u16).context("unknown field scenery layer")?;
        for material in &mut part.materials {
            material.draw_order += stage.offset();
            material.depth_write = section != 2;
        }
        if let Some(motion) = field_core::optional_section(&archive, section + 1)? {
            let bindings = world_gpl::animation_bindings(model)?;
            let encoded = world_motion::encode_bound(motion, &mesh, &bindings)
                .with_context(|| format!("field {id} scenery motion {section}"))?;
            let local_path = if index == 0 {
                format!("{root}/animations/motion-0.glb")
            } else {
                format!("{root}/animations/part-{index}-motion-0.glb")
            };
            let motion_path = shared_idle_owner(id, section)
                .filter(|owner| {
                    fs::read(library.join(owner)).is_ok_and(|existing| existing == encoded)
                })
                .map(str::to_owned)
                .unwrap_or_else(|| local_path.clone());
            part.clips.push(SceneClip {
                motion: motion_path.clone(),
                joints: Default::default(),
                resource_slot: 0,
                duration_seconds: 0.,
                animation_resource: None,
                pose_chains: Vec::new(),
            });
            part.autoplay = true;
            if motion_path == local_path {
                files.push((motion_path, encoded));
            }
        }
        files.push((mesh_path, mesh));
        parts.push(part);
    }
    for (bytes, path) in textures {
        files.push((path, bytes));
    }
    if let Some(banks) = scene_banks {
        field_scene_banks::append_shared_scenery_clips(
            extracted, library, id, &archive, &mut parts, &banks,
        )
        .with_context(|| format!("field {id} shared scenery clips"))?;
    }
    field_scene_banks::append_local_scenery_clips(&archive, &root, &mut parts)?;
    // The field archive's trailing animation members are reusable across
    // actors. Their script resource ID is based on the member slot.
    let section_count = dol::u32(&archive, 0)? as usize;
    for section in 16..section_count {
        let Some(motion) = field_core::optional_section(&archive, section)? else {
            continue;
        };
        if motion.len() < 36 || dol::u32(motion, 0)? != 0x007b_7960 {
            continue;
        }
        let names = world_motion::indexed_track_names(motion)?;
        let glb = world_motion::encode_named_indexed(motion, &names)
            .with_context(|| format!("field {id} local animation section {section}"))?;
        files.push((
            format!("{root}/animations/local-motion-{}.glb", section - 16),
            glb,
        ));
    }
    let mut scene = json!({"parts":parts,"settings":"fields/settings.json"});
    if let Some(environment) = environment {
        scene["environment"] = environment;
    }
    if let Some(water) = water {
        scene["water"] = water;
    }
    if let Some((bindings, _)) = unbound {
        scene["unbound_geometry"] = bindings;
    }
    if let Some(animation) = texture_animations {
        scene["texture_animations"] = animation;
    }
    if !cameras.is_empty() {
        scene["camera_tracks"] = json!(cameras);
    }
    if let Some((path, _)) = panorama {
        scene["panorama"] = json!(path);
    }
    if matches!(id, 184 | 475)
        && overlays
            .values()
            .any(|path| path == "effects/overlays/radial-burst.ktx2")
    {
        let native = field_actors::read_shared_resource(extracted, 68611)?;
        let (texture, _) = world_texture::tpl_texture(&native, 0)?;
        let path = "effects/overlays/radial-burst.ktx2";
        files.push((path.into(), texture));
    }
    if !overlays.is_empty() {
        for path in overlays.values() {
            ensure!(
                library.join(path).is_file() || files.iter().any(|(owned, _)| owned == path),
                "field {id} overlay lacks {path}"
            );
        }
        scene["overlays"] = json!(overlays);
    }
    files.push((
        format!("{root}/scene.json"),
        resonance_asset_writer::json::encode(&scene)?,
    ));
    Ok(files)
}

fn publish(library: &Path, files: Vec<(String, Vec<u8>)>) -> Result<()> {
    for (path, bytes) in files {
        let target = library.join(&path);
        if target.exists() {
            ensure!(
                fs::read(&target)? == bytes,
                "conflicting field asset {path}"
            );
        } else {
            crate::output::write(&target, &bytes)?;
        }
    }
    Ok(())
}

/// Convert field sources/scenery in parallel, then publish and finalize actors in order.
/// Only one CPU-sized batch is retained; each actor reuses its opened native archive.
pub fn cook_fields(extracted: &Path, library: &Path, fields: &[u32]) -> Result<()> {
    // Field 141 owns the idle scenery motion reused by six later fields. Finish
    // its batch before preparing those consumers, retaining the original alias.
    let boundary = fields
        .iter()
        .position(|&id| id == 141)
        .map_or(fields.len(), |i| i + 1);
    let mut completed = 0;
    for section in [&fields[..boundary], &fields[boundary..]] {
        for batch in section.chunks(crate::parallel::workers()) {
            let prepared = crate::parallel::map(batch, |index, &id| {
                eprintln!(
                    "  [{}/{} jobs] Fields: {} ({id})",
                    completed + index + 1,
                    fields.len(),
                    field_core::field_name(id as usize)
                );
                let (exe, archive) = field_core::read(extracted, id)?;
                let files = prepare(extracted, library, id, &exe, &archive)
                    .with_context(|| format!("field {id} scene"))?;
                Ok((id, exe, archive, files))
            })?;
            for (id, exe, archive, files) in prepared {
                publish(library, files)?;
                eprintln!(
                    "    Finalizing actors: {} ({id})",
                    field_core::field_name(id as usize)
                );
                field_actors::cook_from_source(extracted, library, id, &exe, &archive)
                    .with_context(|| format!("field {id} actors"))?;
            }
            completed += batch.len();
        }
    }
    Ok(())
}
