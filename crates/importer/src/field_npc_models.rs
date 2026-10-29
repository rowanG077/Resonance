//! Shared field actors selected by native tables, with readable known names.
use crate::{
    character_models, compression, dol, field_core, field_local_models, native_archive, text,
    world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, AtlasChannel, CullFace, SceneClip,
    field::{
        collision_mesh,
        document::{
            AnimationTarget, Model, ModelAlias, ModelMappings, ModelTarget, SharedAnimationBinding,
        },
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) fn native_bank(extracted: &Path, exe: &[u8]) -> Result<Vec<u8>> {
    grouped_bank(extracted, exe, 2)
}

pub(crate) fn member(bank: &[u8], id: u32) -> Result<&[u8]> {
    ensure!(id >> 16 == 2, "resource {id} is not in the NPC bank");
    let count = dol::u32(bank, 0)? as usize;
    let index = (id & 0xffff) as usize;
    ensure!(
        index < count && 4 + count * 8 <= bank.len(),
        "invalid NPC resource bank"
    );
    let at = 4 + index * 8;
    let start = dol::u32(bank, at)? as usize;
    let size = dol::u32(bank, at + 4)? as usize;
    ensure!(
        start >= 4 + count * 8 && size > 0,
        "missing NPC resource {id}"
    );
    bank.get(start..start + size)
        .context("NPC resource exceeds bank")
}

fn body_pair(package: &[u8]) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
    let Some(body) = field_core::optional_section(package, 0)? else {
        return Ok(None);
    };
    Ok(Some((
        body.to_vec(),
        field_core::optional_section(package, 1)?
            .unwrap_or_default()
            .to_vec(),
    )))
}

fn npc_geometry_owners(
    extracted: &Path,
    bank: &[u8],
    names: &BTreeMap<u32, String>,
) -> Result<BTreeMap<(Vec<u8>, Vec<u8>), String>> {
    let mut owners = BTreeMap::new();
    for (&id, name) in names.iter().filter(|(id, _)| **id >> 16 == 2) {
        if let Some(pair) = body_pair(member(bank, id)?)? {
            owners.entry(pair).or_insert_with(|| name.clone());
        }
    }
    for (id, target) in character_models::party_model_mappings()? {
        let package = character_models::party_package(extracted, id as u8, (id >> 8) as u8)?;
        let body = native_archive::member(&package, 0, 2)?.context("party has no body")?;
        let outline = native_archive::member(&package, 1, 2)?.unwrap_or_default();
        owners.insert((body.to_vec(), outline.to_vec()), target.model().to_owned());
    }
    Ok(owners)
}

/// Native NPC rows missing from the known-name catalogue still get request routes.
/// Full-package aliases share an owner; body-only matches keep distinct documents.
pub(crate) fn npc_mappings(extracted: &Path) -> Result<BTreeMap<u32, ModelTarget>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = native_bank(extracted, &exe)?;
    let names: BTreeMap<u32, String> = serde_json::from_str(include_str!("mesh-model-names.json"))?;
    let owners = npc_geometry_owners(extracted, &bank, &names)?;
    let mut packages = BTreeMap::<&[u8], String>::new();
    for (&id, name) in names.iter().filter(|(id, _)| **id >> 16 == 2) {
        packages
            .entry(member(&bank, id)?)
            .or_insert_with(|| name.clone());
    }
    let mut mappings = BTreeMap::new();
    for index in 0..dol::u32(&bank, 0)? {
        let id = 0x20000 + index;
        if names.contains_key(&id) || dol::u32(&bank, 8 + index as usize * 8)? == 0 {
            continue;
        }
        let package = member(&bank, id)?;
        let Some(pair) = body_pair(package)? else {
            continue;
        };
        let name = owners
            .get(&pair)
            .map(|owner| format!("{owner}/npc-{id}"))
            .unwrap_or_else(|| format!("unidentified/model-{id}"));
        let target = if let Some(owner) = packages.get(package) {
            ModelTarget::Alias(ModelAlias {
                name,
                model: owner.clone(),
            })
        } else {
            packages.insert(package, name.clone());
            ModelTarget::Model(name)
        };
        mappings.insert(id, target);
    }
    Ok(mappings)
}

/// The native Colette package has three motions but no body of its own.
pub(crate) fn cook_motion_only_npc(
    extracted: &Path,
    library: &Path,
) -> Result<(AnimationTarget, Vec<SharedAnimationBinding>)> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = native_bank(extracted, &exe)?;
    let package = member(&bank, 131328)?;
    let mesh = fs::read(library.join("meshes/characters/colette/meshes/body.glb"))?;
    let joints = character_models::party_joint_bindings(extracted, 2, 0)?;
    let mappings: ModelMappings =
        serde_json::from_slice(&fs::read(library.join("meshes/mappings.json"))?)?;
    let actors = mappings
        .0
        .values()
        .filter(|target| {
            target.name().starts_with("characters/colette") && !target.name().contains("/wings")
        })
        .map(|target| target.name().to_owned())
        .collect::<Vec<_>>();
    ensure!(!actors.is_empty(), "motion-only NPC has no Colette actors");
    let name = "characters/colette/package-131328";
    let mut shared = BTreeMap::<Vec<u8>, String>::new();
    let mut bindings = Vec::new();
    for (index, suffix) in [(2, ""), (8, "-walk"), (9, "-run")] {
        let bytes = field_core::optional_section(package, index)?
            .context("motion-only NPC lacks expected motion")?;
        let native = if dol::u32(bytes, 0)? == 0x007b_7960 {
            bytes.to_vec()
        } else {
            compression::decode(bytes)?
        };
        let bytes = world_motion::encode_bound(&native, &mesh, &joints)?;
        let motion = if let Some(path) = shared.get(&bytes) {
            path.clone()
        } else {
            let path = format!("animations/{name}{suffix}.glb");
            if !library.join(&path).is_file() {
                crate::output::write(&library.join(&path), &bytes)?;
            }
            shared.insert(bytes, path.clone());
            path
        };
        bindings.push(SharedAnimationBinding {
            actors: actors.clone(),
            parts: vec![0, 1],
            motion,
            joint_map: None,
            resource_slot: 4 + index as u16 * 4,
            pose_chains: Vec::new(),
        });
    }
    Ok((
        AnimationTarget::Shared {
            name: name.into(),
            motion: bindings[0].motion.clone(),
        },
        bindings,
    ))
}

/// Return selected model bodies for field-local motion remaps without cooking media.
pub(crate) fn npc_native_bodies(extracted: &Path, ids: &[u32]) -> Result<BTreeMap<u32, Vec<u8>>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = native_bank(extracted, &exe)?;
    ids.iter()
        .map(|&id| {
            let body = field_core::optional_section(member(&bank, id)?, 0)?
                .context("NPC model has no body")?;
            Ok((id, body.to_vec()))
        })
        .collect()
}

fn palette(model: &[u8]) -> Result<Option<&[u8]>> {
    if dol::u32(model, 0)? == 0 {
        return Ok(None);
    }
    let palette = model
        .get(dol::u32(model, 0)? as usize..dol::u32(model, 4)? as usize)
        .context("NPC palette exceeds model")?;
    ensure!(dol::u32(palette, 0)? == 0x0020_af30, "NPC model lacks TPL");
    Ok(Some(palette))
}

fn atlas_variant(model: &[u8]) -> Result<Option<AtlasChannel>> {
    let Some(index) = model.get(0x0e).and_then(|value| value.checked_sub(1)) else {
        return Ok(None);
    };
    let tpl = palette(model)?.context("variant channel has no TPL")?;
    let count = dol::u32(tpl, 4)? as usize;
    ensure!(
        usize::from(index) < count,
        "variant channel exceeds model textures"
    );
    let table = dol::u32(tpl, 8)? as usize;
    let descriptor = dol::u32(tpl, table + usize::from(index) * 8)? as usize;
    let width = dol::u16(tpl, descriptor + 2)?;
    let height = dol::u16(tpl, descriptor)?;
    let frames = match (width == 128, height) {
        (true, 256) | (false, 512) => 2,
        (true, 512) | (false, 1024) => 4,
        _ => return Ok(None),
    };
    Ok(Some(AtlasChannel {
        texture: usize::from(index),
        frames,
    }))
}

fn textures(
    library: &Path,
    base: &str,
    layer: &str,
    model: &[u8],
    shared: &mut BTreeMap<Vec<u8>, String>,
) -> Result<(Vec<String>, Vec<bool>)> {
    let Some(tpl) = palette(model)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    for index in 0..dol::u32(tpl, 4)? as usize {
        let (bytes, transparent) = world_texture::tpl_texture(tpl, index)?;
        let path = write_shared(
            library,
            format!("{base}/textures/{layer}-texture-{index}.ktx2"),
            bytes,
            shared,
        )?;
        paths.push(path);
        alpha.push(transparent);
    }
    Ok((paths, alpha))
}

fn write_shared(
    library: &Path,
    path: String,
    bytes: Vec<u8>,
    shared: &mut BTreeMap<Vec<u8>, String>,
) -> Result<String> {
    if let Some(owner) = shared.get(&bytes) {
        return Ok(owner.clone());
    }
    crate::output::write(&library.join(&path), &bytes)?;
    shared.insert(bytes, path.clone());
    Ok(path)
}

fn clip(path: String, slot: u16) -> SceneClip {
    SceneClip {
        motion: path,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: None,
        pose_chains: Vec::new(),
    }
}

pub(crate) fn convert(library: &Path, id: u32, name: &str, package: &[u8]) -> Result<()> {
    convert_with_owner(library, id, name, package, None)
}

/// Reuse proven identical geometry, while converting this package's own motions.
pub(crate) fn convert_with_owner(
    library: &Path,
    id: u32,
    name: &str,
    package: &[u8],
    owner: Option<&str>,
) -> Result<()> {
    convert_shared(library, id, name, package, owner, &mut BTreeMap::new())
}

/// Share only identical final media bytes; model bindings remain independently editable.
pub(crate) fn convert_shared(
    library: &Path,
    id: u32,
    name: &str,
    package: &[u8],
    owner: Option<&str>,
    shared: &mut BTreeMap<Vec<u8>, String>,
) -> Result<()> {
    convert_package(library, name, package, owner, shared, false)
        .with_context(|| format!("resource {id} ({name})"))
}

pub(crate) fn convert_package(
    library: &Path,
    name: &str,
    package: &[u8],
    owner: Option<&str>,
    shared: &mut BTreeMap<Vec<u8>, String>,
    battle: bool,
) -> Result<()> {
    let body = field_core::optional_section(package, 0)?.context("NPC has no body")?;
    ensure!(world_gpl::is_model(body), "invalid native actor model");
    let base = format!("meshes/{name}");
    let reused: Option<Model> = owner
        .map(|owner| -> Result<Model> {
            Ok(serde_json::from_slice(&fs::read(
                library.join(format!("meshes/{owner}/model.json")),
            )?)?)
        })
        .transpose()?;
    let (mesh, mut body_part) = if let Some(model) = &reused {
        let part = model
            .parts
            .first()
            .context("geometry owner has no body")?
            .clone();
        (fs::read(library.join(&part.mesh))?, part)
    } else {
        let (paths, alpha) = textures(library, &base, "body", body, shared)?;
        let (mesh, mut part) = world_gpl::static_model(body, paths, alpha)
            .with_context(|| format!("model {name} body"))?;
        part.mesh = write_shared(
            library,
            format!("{base}/meshes/body.glb"),
            mesh.clone(),
            shared,
        )?;
        (mesh, part)
    };
    body_part.appearance = Some(AppearanceTextures {
        eyes: body[0x0c].checked_sub(1).map(usize::from),
        mouth: body[0x0d].checked_sub(1).map(usize::from),
        variant: atlas_variant(body)?,
        costume: None,
    });
    for material in &mut body_part.materials {
        if material.color.is_none() && material.multiply.is_none() {
            material.blend = false;
        }
    }
    body_part.prepare_bones(&mesh)?;
    let bindings = world_gpl::animation_bindings(body)?;
    if reused.is_none() && name.starts_with("characters/") {
        let json_len =
            u32::from_le_bytes(mesh.get(12..16).context("short NPC GLB")?.try_into()?) as usize;
        let gltf: serde_json::Value =
            serde_json::from_slice(mesh.get(20..20 + json_len).context("short NPC GLB JSON")?)?;
        let source_names = bindings
            .iter()
            .map(|(_, name)| name.clone())
            .collect::<Vec<_>>();
        body_part.secondary_motion = character_models::character_dynamics::bind(
            name.strip_prefix("characters/").unwrap(),
            &gltf,
            &source_names,
            &body_part.bone_names,
        )?;
    }
    let mut clips = Vec::new();
    let mut unique = BTreeMap::<Vec<u8>, String>::new();
    for clip in reused
        .iter()
        .flat_map(|model| &model.parts)
        .flat_map(|part| &part.clips)
    {
        let bytes = fs::read(library.join(&clip.motion))?;
        unique.insert(
            character_models::motion_signature(&bytes)?,
            clip.motion.clone(),
        );
    }
    let count = dol::u32(package, 0)? as usize;
    for index in 2..count {
        let Some(native) = field_core::optional_section(package, index)? else {
            continue;
        };
        let slot = if battle {
            (index - 2) as u16
        } else {
            4 + index as u16 * 4
        };
        let expanded = if dol::u32(native, 0)? == 0x007b_7960 {
            native.to_vec()
        } else {
            let Some(header) = native.get(..9) else {
                continue;
            };
            let packed = u32::from_le_bytes(header[1..5].try_into()?) as usize;
            if !matches!(header[0], 0 | 1 | 3) || packed == 0 || packed > native.len() - 9 {
                // Actor packages may use unused trailing slots for other
                // model data. Only resource-wrapped motions are clips.
                continue;
            }
            compression::decode(native)
                .with_context(|| format!("model {name} motion slot {slot}"))?
        };
        let glb = world_motion::encode_bound(&expanded, &mesh, &bindings)
            .with_context(|| format!("model {name} motion slot {slot}"))?;
        let signature = character_models::motion_signature(&glb)?;
        let path = if let Some(path) = unique.get(&signature) {
            path.clone()
        } else {
            let label = if battle {
                resonance_content::battle_motion::name(slot).to_owned()
            } else {
                match slot {
                    12 => "idle".into(),
                    36 => "walk".into(),
                    40 => "run".into(),
                    _ => format!("motion-{slot}"),
                }
            };
            let path = write_shared(
                library,
                format!("{base}/animations/{label}.glb"),
                glb,
                shared,
            )?;
            unique.insert(signature, path.clone());
            path
        };
        let mut binding = clip(path, slot);
        if !body_part.secondary_motion.is_empty() {
            binding.pose_chains = character_models::pose_chains(
                &expanded,
                &bindings,
                &body_part.bone_names,
                &body_part.secondary_motion,
            )
            .with_context(|| format!("model {name} pose chains for motion slot {slot}"))?;
        }
        clips.push(binding);
    }
    body_part.clips = clips.clone();
    let mut parts = vec![body_part];
    if let Some(model) = &reused {
        for mut part in model.parts.iter().skip(1).cloned() {
            part.clips = clips.clone();
            parts.push(part);
        }
    } else if let Some(outline) = field_core::optional_section(package, 1)? {
        let own_palette = dol::u32(outline, 0)? != 0;
        let normalized = if own_palette {
            outline.to_vec()
        } else {
            world_gpl::with_primary_palette(body, outline)?
        };
        let (paths, alpha) = if own_palette {
            textures(library, &base, "outline", &normalized, shared)?
        } else {
            (
                parts[0].textures.clone(),
                vec![false; parts[0].textures.len()],
            )
        };
        let (outline_mesh, mut part) = world_gpl::static_model(&normalized, paths, alpha)
            .with_context(|| format!("model {name} outline"))?;
        part.mesh = write_shared(
            library,
            format!("{base}/meshes/outline.glb"),
            outline_mesh,
            shared,
        )?;
        part.resource = 1;
        part.outline_color = Some([0, 0, 0, 127]);
        if let Some(variant) = atlas_variant(&normalized)? {
            part.appearance = Some(AppearanceTextures {
                eyes: None,
                mouth: None,
                variant: Some(variant),
                costume: None,
            });
        }
        part.secondary_motion = parts[0].secondary_motion.clone();
        part.clips = clips;
        for material in &mut part.materials {
            material.cull = CullFace::Front;
            material.blend = true;
            material.draw_order += 65536;
        }
        parts.push(part);
    }
    let hidden_geometry = parts[0]
        .bone_names
        .iter()
        .filter(|name| name.starts_with("kk"))
        .cloned()
        .collect();
    let collision = if battle || count <= 30 {
        Default::default()
    } else {
        field_local_models::actor_collision(package)?
    };
    let collision = if collision
        .floors
        .iter()
        .chain(&collision.solids)
        .any(|group| !group.triangles.is_empty())
    {
        let path = write_shared(
            library,
            format!("{base}/collision.glb"),
            collision_mesh::encode_model(&collision)?,
            shared,
        )?;
        Some(path)
    } else {
        None
    };
    let document = Model {
        parts,
        collision,
        hidden_geometry,
    };
    crate::output::json(&library.join(format!("{base}/model.json")), &document)
}

/// One named shared NPC/prop, without recooking the whole resource bank.
pub fn cook_field_npc_model(extracted: &Path, library: &Path, id: u32) -> Result<()> {
    ensure!(id >> 16 == 2, "resource {id} is not in the NPC bank");
    let aliases: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("mesh-model-aliases.json"))?;
    ensure!(
        !aliases.contains_key(&id),
        "NPC model {id} is an alias; cook its physical owner instead"
    );
    let names: BTreeMap<u32, String> = serde_json::from_str(include_str!("mesh-model-names.json"))?;
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let bank = native_bank(extracted, &exe)?;
    let additions = npc_mappings(extracted)?;
    let name = match names.get(&id) {
        Some(name) => name.as_str(),
        None => match additions
            .get(&id)
            .context("NPC has no body; use its animation route")?
        {
            ModelTarget::Model(name) => name,
            ModelTarget::Alias(alias) => {
                anyhow::bail!("NPC {id} reuses {}; cook that owner", alias.model)
            }
        },
    };
    let package = member(&bank, id)?;
    let owners = npc_geometry_owners(extracted, &bank, &names)?;
    let owner = body_pair(package)?
        .and_then(|pair| owners.get(&pair))
        .filter(|owner| {
            owner.as_str() != name && library.join(format!("meshes/{owner}/model.json")).is_file()
        });
    convert_with_owner(library, id, name, package, owner.map(String::as_str))
}

/// Group one is the shared field-effect bank; group two is the NPC bank.
pub(crate) fn grouped_bank(extracted: &Path, exe: &[u8], group: u32) -> Result<Vec<u8>> {
    ensure!(matches!(group, 1 | 2), "invalid field model group {group}");
    let name = text::pointer(exe, 0x801f_86b8 + (group - 1) * 12)?;
    ensure!(!name.contains(['/', '\\']), "invalid field model bank name");
    let bytes = native_archive::read_named(&extracted.join("disc1/files"), &name)?;
    if bytes.starts_with(b"MSCF") {
        compression::cabinet(&bytes)
    } else {
        Ok(bytes)
    }
}

pub(crate) fn grouped_member(bank: &[u8], id: u32) -> Result<&[u8]> {
    let count = dol::u32(bank, 0)? as usize;
    let index = (id & 0xffff) as usize;
    ensure!(
        index < count && 4 + count * 8 <= bank.len(),
        "field model {id} exceeds its native bank"
    );
    let mut at = 4 + index * 8;
    if dol::u32(bank, at + 4)? == 0 {
        at = 4;
    }
    let start = dol::u32(bank, at)? as usize;
    let size = dol::u32(bank, at + 4)? as usize;
    ensure!(
        start >= 4 + count * 8 && size > 0,
        "field model {id} has no native bank member"
    );
    bank.get(start..start + size)
        .with_context(|| format!("field model {id} exceeds its native bank"))
}

pub(crate) fn expanded_member(bytes: &[u8]) -> Result<Vec<u8>> {
    let bytes = if bytes.starts_with(b"MSCF") {
        compression::cabinet(bytes)
    } else if bytes.len() >= 9
        && matches!(bytes[0], 0 | 1 | 3)
        && u32::from_le_bytes(bytes[1..5].try_into()?) as usize + 9 == bytes.len()
    {
        compression::decode(bytes)
    } else {
        Ok(bytes.to_vec())
    }?;
    if dol::u32(&bytes, 0)? == 31 {
        return Ok(bytes);
    }
    // Some effect resources are a bare GPL with its palette rather than a
    // 31-member actor package. Give the existing model converter one body.
    ensure!(
        world_gpl::is_model(&bytes),
        "field effect is neither an actor package nor a bare GPL"
    );
    let mut package = vec![0; 4 + 31 * 4];
    let body_at = package.len() as u32;
    package[..4].copy_from_slice(&31u32.to_be_bytes());
    package[4..8].copy_from_slice(&body_at.to_be_bytes());
    package.extend(bytes);
    Ok(package)
}

/// Find each named bank-zero package once. Repeated MAP entries must have the
/// same source bytes; the readable output name is maintained independently.
pub(crate) fn field_sources(
    extracted: &Path,
    exe: &[u8],
    wanted: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, Vec<u8>>> {
    if wanted.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rows = dol::slice(exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let mut found = BTreeMap::<u32, (usize, Vec<u8>)>::new();
    for (field, row) in rows.chunks_exact(24).enumerate() {
        let Some(archive) = field_core::native_archive(extracted, exe, row, field)? else {
            continue;
        };
        let Some(bank) = field_core::optional_section(&archive, 7)? else {
            continue;
        };
        for (id, package) in field_model_entries(bank)? {
            let id = u32::from(id);
            if !wanted.contains(&id) {
                continue;
            }
            if let Some((source, previous)) = found.get(&id) {
                ensure!(
                    previous == package,
                    "field model {id} differs between fields {source} and {field}"
                );
            } else {
                found.insert(id, (field, package.to_vec()));
            }
        }
    }
    Ok(found
        .into_iter()
        .map(|(id, (_, bytes))| (id, bytes))
        .collect())
}

/// Publish every named shared field model from one native physical source.
/// Alias IDs remain in meshes/mappings.json and do not duplicate media.
pub fn cook_field_models(extracted: &Path, library: &Path) -> Result<()> {
    let names: BTreeMap<u32, String> = serde_json::from_str(include_str!("mesh-model-names.json"))?;
    let aliases: BTreeMap<u32, String> =
        serde_json::from_str(include_str!("mesh-model-aliases.json"))?;
    let mut selected = names
        .iter()
        .filter(|(id, _)| {
            (**id >> 16 <= 2 || service_source(**id).is_some()) && !aliases.contains_key(*id)
        })
        .filter(|(_, name)| !library.join(format!("meshes/{name}/model.json")).is_file())
        .map(|(&id, name)| (id, name.clone()))
        .collect::<BTreeMap<_, _>>();
    for (id, target) in npc_mappings(extracted)? {
        if let ModelTarget::Model(name) = target {
            if !library.join(format!("meshes/{name}/model.json")).is_file() {
                selected.insert(id, name);
            }
        }
    }
    if selected.is_empty() {
        return Ok(());
    }
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let field_ids = selected
        .keys()
        .copied()
        .filter(|id| *id >> 16 == 0)
        .collect::<BTreeSet<_>>();
    let mut fields = field_sources(extracted, &exe, &field_ids)?;
    let effects = selected
        .keys()
        .any(|id| *id >> 16 == 1)
        .then(|| grouped_bank(extracted, &exe, 1))
        .transpose()?;
    let npcs = selected
        .keys()
        .any(|id| *id >> 16 == 2)
        .then(|| native_bank(extracted, &exe))
        .transpose()?;
    let mut geometry = npcs
        .as_ref()
        .map(|bank| npc_geometry_owners(extracted, bank, &names))
        .transpose()?
        .unwrap_or_default();
    let mut selected = selected.into_iter().collect::<Vec<_>>();
    selected.sort_by_key(|(id, _)| (!names.contains_key(id), *id));
    for (id, name) in selected {
        if service_source(id).is_some() {
            convert(library, id, &name, &service_package(extracted, &exe, id)?)
                .with_context(|| format!("convert field service model {id} ({name})"))?;
            continue;
        }
        let package = match id >> 16 {
            0 => match fields.remove(&id) {
                Some(package) => package,
                None => standalone_package(extracted, &exe, id)
                    .with_context(|| format!("field model {id} has no MAP or standalone source"))?,
            },
            1 => expanded_member(grouped_member(
                effects.as_deref().context("missing effect bank")?,
                id,
            )?)?,
            2 => member(npcs.as_deref().context("missing NPC bank")?, id)?.to_vec(),
            _ => unreachable!(),
        };
        let pair = if id >> 16 == 2 {
            body_pair(&package)?
        } else {
            None
        };
        let owner = pair
            .as_ref()
            .and_then(|pair| geometry.get(pair))
            .filter(|owner| {
                **owner != name && library.join(format!("meshes/{owner}/model.json")).is_file()
            });
        convert_with_owner(library, id, &name, &package, owner.map(String::as_str))
            .with_context(|| format!("convert shared field model {id} ({name})"))?;
        if let Some(pair) = pair {
            geometry.entry(pair).or_insert(name);
        }
    }
    Ok(())
}

/// The first member of MAP section 7 maps each following actor package to a script ID.
pub(crate) fn field_model_entries(bank: &[u8]) -> Result<Vec<(u16, &[u8])>> {
    let count = dol::u32(bank, 0)? as usize;
    ensure!(
        count > 0 && count <= bank.len().saturating_sub(4) / 4,
        "invalid field model bank"
    );
    let starts = (0..count)
        .map(|index| dol::u32(bank, 4 + index * 4).map(|value| (value & !3) as usize))
        .collect::<Result<Vec<_>>>()?;
    if count == 1 && starts[0] == 0 {
        return Ok(Vec::new());
    }
    ensure!(
        starts
            .iter()
            .all(|&at| at >= 4 + count * 4 && at < bank.len()),
        "field model bank offset is invalid"
    );
    let end = |at| {
        starts
            .iter()
            .copied()
            .filter(|&other| other > at)
            .min()
            .unwrap_or(bank.len())
    };
    let ids = bank
        .get(starts[0]..end(starts[0]))
        .context("field model IDs exceed bank")?;
    ensure!(ids.len() >= (count - 1) * 2, "truncated field model IDs");
    (1..count)
        .map(|index| {
            let id = dol::u16(ids, (index - 1) * 2)?;
            Ok((id, &bank[starts[index]..end(starts[index])]))
        })
        .collect()
}

pub(crate) fn standalone_package(extracted: &Path, exe: &[u8], id: u32) -> Result<Vec<u8>> {
    const STANDALONE: u32 = 0x801f_85e4;
    const GROUPS: u32 = 0x801f_86b8;
    ensure!(id < (GROUPS - STANDALONE) / 4, "no standalone model {id}");
    let filename = text::pointer(exe, STANDALONE + id * 4)?;
    ensure!(
        !filename.is_empty() && !filename.contains(['/', '\\']),
        "invalid standalone model filename"
    );
    let files = extracted.join("disc1/files");
    let path = if files.join(&filename).is_file() {
        files.join(&filename)
    } else {
        fs::read_dir(&files)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&filename))
            })
            .with_context(|| format!("standalone model {filename} is missing"))?
    };
    let bytes = fs::read(path)?;
    Ok(if bytes.starts_with(b"MSCF") {
        compression::cabinet(&bytes)?
    } else {
        bytes
    })
}

/// Fixed native service models used by save points and treasure chests.
pub(crate) fn service_source(id: u32) -> Option<u32> {
    if id == resonance_content::field::SAVE_POINT_RESOURCE {
        Some(0x8017_a52c)
    } else {
        id.checked_sub(resonance_content::field::TREASURE_RESOURCE_BASE)
            .filter(|&kind| kind < 3)
            .map(|kind| 0x8017_a538 + kind * 12)
    }
}

pub(crate) fn service_package(extracted: &Path, exe: &[u8], id: u32) -> Result<Vec<u8>> {
    let address = service_source(id).context("resource is not a field service model")?;
    let name = text::string(exe, address)?;
    ensure!(
        !name.contains('/') && !name.contains(char::from(92)),
        "invalid field service filename"
    );
    let bytes = crate::native_archive::read_named(&extracted.join("disc1/files"), &name)?;
    if bytes.starts_with(b"MSCF") {
        compression::cabinet(&bytes)
    } else {
        Ok(bytes)
    }
}

/// Read a standalone field model's native GPL body for clip bindings.
pub(crate) fn standalone_model_body(extracted: &Path, id: u32) -> Result<Vec<u8>> {
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    let package = standalone_package(extracted, &exe, id)?;
    Ok(field_core::optional_section(&package, 0)?
        .context("standalone model has no body")?
        .to_vec())
}

/// Select one script-ID package from a field's native model bank.
pub fn cook_field_model(extracted: &Path, library: &Path, field_id: u32, id: u32) -> Result<()> {
    ensure!(
        (field_id as usize) < field_core::FIELD_COUNT,
        "field ID is outside native catalogue"
    );
    let names: BTreeMap<u32, String> = serde_json::from_str(include_str!("mesh-model-names.json"))?;
    let name = names.get(&id).context("unnamed field model")?;
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    if service_source(id).is_some() {
        return convert(library, id, name, &service_package(extracted, &exe, id)?);
    }
    if id >> 16 == 1 {
        let bank = grouped_bank(extracted, &exe, 1)?;
        return convert(
            library,
            id,
            name,
            &expanded_member(grouped_member(&bank, id)?)?,
        );
    }
    ensure!(id <= u16::MAX as u32, "field model ID is too large");
    let rows = dol::slice(&exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
    let row = &rows[field_id as usize * 24..][..24];
    let archive = field_core::native_archive(extracted, &exe, row, field_id as usize)?
        .context("field has no native archive")?;
    let bank = field_core::optional_section(&archive, 7)?.context("field has no model bank")?;
    let package = field_model_entries(bank)?
        .into_iter()
        .find(|&(resource, _)| u32::from(resource) == id)
        .map(|(_, package)| package.to_vec());
    let package = match package {
        Some(package) => package,
        None => standalone_package(extracted, &exe, id)?,
    };
    convert(library, id, name, &package)
}
