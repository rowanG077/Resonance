//! Field-owned actor packages, directly from native MAP sections.
use crate::{
    character_models, compression, dol, field_actors, field_core, field_npc_models, mesh_mappings,
    shared_animations, world_gpl, world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_asset_writer::gpu;
use resonance_content::{
    AppearanceTextures, CullFace, SceneClip, SceneImage,
    field::{
        ModelCollision, collision_mesh,
        document::{Actor, Model},
    },
};
use std::{collections::BTreeMap, fs, path::Path};

const LOCAL_BASE: u32 = 0xffee_0000;

pub(crate) fn local_motion_sections(archive: &[u8]) -> Result<Vec<usize>> {
    let count = dol::u32(archive, 0)? as usize;
    let mut sections = Vec::new();
    for section in 16..count {
        let Some(bytes) = field_core::optional_section(archive, section)? else {
            continue;
        };
        if bytes.len() >= 36 && dol::u32(bytes, 0)? == 0x007b_7960 {
            sections.push(section - 16);
        }
    }
    Ok(sections)
}

/// Bind each local resource to a field-owned definition decoded from its own slot.
pub(crate) fn local_models(id: u32, archive: &[u8]) -> Result<Vec<(usize, Actor)>> {
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let mut models = Vec::new();
    for section in 16..dol::u32(archive, 0)? as usize {
        let Some(package) = field_core::optional_section(archive, section)? else {
            continue;
        };
        if crate::field_scene::caller_textured_model(id, section) {
            continue;
        }
        let resource = LOCAL_BASE + (section - 16) as u32;
        if dol::u32(package, 0)? != 31 && !world_gpl::is_model(package) {
            continue;
        }
        let model = format!("{root}/models/unidentified-{}.json", section - 15);
        models.push((
            section,
            Actor {
                resource,
                model,
                hidden_geometry: None,
            },
        ));
    }
    Ok(models)
}

fn clip(motion: String, slot: u16, bank: Option<u32>) -> SceneClip {
    SceneClip {
        motion,
        joints: Default::default(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: bank,
        pose_chains: Vec::new(),
    }
}

fn shared_clips(
    extracted: &Path,
    library: &Path,
    field_id: u32,
    banks: &BTreeMap<u32, String>,
) -> Result<
    Vec<(
        u32,
        String,
        Vec<crate::field_bank_bindings::Track>,
        BTreeMap<u16, String>,
    )>,
> {
    if banks.is_empty() {
        return Ok(Vec::new());
    }
    let party = [
        "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
    ];
    let mappings = mesh_mappings::mesh_mappings()?;
    let mut npc_ids = BTreeMap::new();
    for name in banks.values() {
        if !name.starts_with("characters/") {
            continue;
        }
        let owner = name
            .split('/')
            .nth(1)
            .context("field animation has no owner")?;
        if !party.contains(&owner) {
            let target = format!("characters/{owner}");
            let id = mappings
                .0
                .iter()
                .find(|(_, model)| model.name() == target)
                .map(|(&id, _)| id)
                .with_context(|| format!("field animation owner {owner} has no model ID"))?;
            npc_ids.insert(owner.to_owned(), id);
        }
    }
    let npc = field_npc_models::npc_native_bodies(
        extracted,
        &npc_ids
            .values()
            .copied()
            .filter(|id| id >> 16 == 2)
            .collect::<Vec<_>>(),
    )?;
    let embedded = if npc_ids.values().any(|id| *id <= u16::MAX as u32) {
        let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
        let rows = dol::slice(&exe, field_core::PHASES, field_core::FIELD_COUNT * 24)?;
        let row = &rows[field_id as usize * 24..field_id as usize * 24 + 24];
        let archive = field_core::native_archive(extracted, &exe, row, field_id as usize)?
            .context("field animation owner has no native archive")?;
        field_core::optional_section(&archive, 7)?
            .map(field_npc_models::field_model_entries)
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _)| {
                npc_ids
                    .values()
                    .any(|requested| *requested == u32::from(*id))
            })
            .map(|(id, package)| {
                let body = if package.len() >= 36 && dol::u32(package, 32)? == 0x005b_bc61 {
                    package
                } else {
                    field_core::optional_section(package, 0)?
                        .context("embedded field model has no body")?
                };
                Ok((u32::from(id), body.to_vec()))
            })
            .collect::<Result<BTreeMap<_, _>>>()?
    } else {
        BTreeMap::new()
    };
    let mut rigs = BTreeMap::new();
    for name in banks.values() {
        if !name.starts_with("characters/") {
            continue;
        }
        let owner = name
            .split('/')
            .nth(1)
            .context("field animation has no owner")?;
        if rigs.contains_key(owner) {
            continue;
        }
        let joints = if let Some(index) = party.iter().position(|name| *name == owner) {
            let body = character_models::party_body(extracted, index as u8 + 1, 0)?;
            world_gpl::named_animation_bindings(&body)?
        } else if let Some(body) = npc.get(&npc_ids[owner]) {
            world_gpl::named_animation_bindings(body)?
        } else if let Some(body) = embedded.get(&npc_ids[owner]) {
            world_gpl::named_animation_bindings(body)?
        } else {
            let body = field_npc_models::standalone_model_body(extracted, npc_ids[owner])?;
            world_gpl::named_animation_bindings(&body)?
        };
        rigs.insert(owner.to_owned(), joints);
    }
    banks
        .iter()
        .map(|(id, name)| {
            let owner = name
                .split('/')
                .nth(1)
                .context("field animation has no owner")?;
            let native = shared_animations::read_bank(extracted, Some(field_id), *id)?;
            let joints = crate::field_bank_bindings::source_tracks(&native)?;
            let empty = Vec::new();
            let rig = rigs.get(owner).unwrap_or(&empty);
            let labels = if name.starts_with("unassigned/") {
                shared_animations::unassigned_track_names(&native)?
            } else {
                world_motion::shared_track_names(&native, rig)
                    .with_context(|| format!("field animation {id} {name}"))?
            };
            Ok((
                *id,
                shared_animations::mapped_motion_path(library, *id)?,
                joints,
                labels,
            ))
        })
        .collect()
}

fn actor_motion(bytes: &[u8]) -> bool {
    if dol::u32(bytes, 0).ok() == Some(0x007b_7960) {
        return true;
    }
    if !matches!(bytes.first(), Some(0 | 1 | 3)) {
        return false;
    }
    compression::decode(bytes)
        .ok()
        .is_some_and(|motion| world_motion::indexed_joints(&motion).is_ok())
}

pub(crate) fn actor_collision(package: &[u8]) -> Result<ModelCollision> {
    let mut collision = ModelCollision::default();
    for (slot, groups) in [(29, &mut collision.floors), (30, &mut collision.solids)] {
        let Some(native) = field_core::optional_section(package, slot)? else {
            continue;
        };
        if actor_motion(native) {
            continue;
        }
        *groups = field_core::collision_groups(native, true)?;
    }
    Ok(collision)
}

fn shared_joint_map(
    bindings: &[(u16, String)],
    target: &BTreeMap<u16, String>,
    source: &[crate::field_bank_bindings::Track],
    labels: &BTreeMap<u16, String>,
) -> BTreeMap<String, Option<String>> {
    let joints = bindings
        .iter()
        .filter_map(|(id, raw)| Some((*id, target.get(id)?.clone(), raw.clone())))
        .collect::<Vec<_>>();
    let selected = crate::field_bank_bindings::selected_tracks(&joints, source);
    joints
        .iter()
        .zip(selected)
        .filter_map(|((_, target, _), track)| {
            let selected = track
                .and_then(|index| labels.get(&source[index].id))
                .cloned();
            (selected.is_some() || labels.values().any(|name| name == target))
                .then(|| (target.clone(), selected))
        })
        .collect()
}

/// Fixed field-owned paths never depend on previously cooked files or field order.
struct ModelOutput<'a> {
    library: &'a Path,
    base: &'a str,
}

impl ModelOutput<'_> {
    fn write(&self, suffix: &str, bytes: &[u8]) -> Result<String> {
        let path = format!("{}{suffix}", self.base);
        crate::output::write(&self.library.join(&path), bytes)?;
        Ok(path)
    }

    fn texture(
        &self,
        textures: &mut BTreeMap<[u8; 32], String>,
        pixels: &[resonance_asset_writer::texture::Pixels],
        suffix: &str,
    ) -> Result<String> {
        let key = world_texture::texture_key(pixels);
        if let Some(path) = textures.get(&key) {
            return Ok(path.clone());
        }
        let encoded = gpu::encode(pixels).with_context(|| format!("{}{suffix}", self.base))?;
        let path = self.write(suffix, &encoded)?;
        textures.insert(key, path.clone());
        Ok(path)
    }

    fn model(&self, model: &Model) -> Result<()> {
        crate::output::json(&self.library.join(format!("{}.json", self.base)), model)
    }
}

fn convert(
    archive: &[u8],
    root: &str,
    section: usize,
    shared: &[(
        u32,
        String,
        Vec<crate::field_bank_bindings::Track>,
        BTreeMap<u16, String>,
    )],
    local_motions: &[usize],
    output: ModelOutput<'_>,
    motions: &mut BTreeMap<Vec<u8>, String>,
    textures: &mut BTreeMap<[u8; 32], String>,
) -> Result<()> {
    let native = field_core::optional_section(archive, section)?.context("local actor missing")?;
    let base = output.base;
    let bare = world_gpl::is_model(native);
    if bare && !bare_local_model(native)? {
        let palette = if section + 1 < dol::u32(archive, 0)? as usize {
            field_core::optional_section(archive, section + 1)?
        } else {
            None
        };
        if palette.is_some_and(|bytes| bytes.starts_with(&0x0020_af30_u32.to_be_bytes())) {
            return cook_separate_palette(archive, section, output);
        }
        return cook_scene_capture(archive, section, output);
    }
    let package = native;
    let body = if bare {
        native
    } else {
        field_core::optional_section(package, 0)?.context("local actor has no body")?
    };
    let start = dol::u32(body, 0)? as usize;
    let end = dol::u32(body, 4)? as usize;
    let mut normalized = body.to_vec();
    if start == 0 {
        ensure!(
            end >= 0x20 && end <= body.len(),
            "invalid palette-free actor body"
        );
        normalized[..4].copy_from_slice(&(end as u32).to_be_bytes());
    }
    let body = normalized.as_slice();
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    let mut images = Vec::new();
    let mut sizes = Vec::new();
    if start != 0 {
        let tpl = body.get(start..end).context("local palette exceeds body")?;
        ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "local body lacks TPL");
        for index in 0..dol::u32(tpl, 4)? as usize {
            let pixels = world_texture::tpl_pixels(tpl, index, 0, false, true)?;
            alpha.push(world_texture::has_alpha(&pixels[0].rgba));
            sizes.push([pixels[0].width, pixels[0].height]);
            paths.push(format!("{base}-texture-{}.ktx2", index + 1));
            images.push(pixels);
        }
    }
    let (mut mesh, mut part) = world_gpl::static_model(body, paths.clone(), alpha)
        .with_context(|| format!("local actor section {section}"))?;
    let wrapped = world_gpl::console_wrap(&mut mesh, &mut part, &sizes)
        .with_context(|| format!("local actor section {section} textures"))?;
    for (index, mut pixels) in images.into_iter().enumerate() {
        if wrapped[index] != [false; 2] {
            pixels = resonance_asset_writer::texture::console_wrapped(&pixels, wrapped[index]);
        }
        paths[index] =
            output.texture(textures, &pixels, &format!("-texture-{}.ktx2", index + 1))?;
    }
    part.textures = paths.clone();
    if start == 0 {
        for material in &mut part.materials {
            if material.color.is_none() && material.multiply.is_none() {
                material.blend = false;
            }
        }
    }
    part.mesh = output.write(".glb", &mesh)?;
    let bindings = world_gpl::animation_bindings(body)?;
    let target_bindings = world_gpl::animation_target_bindings(bindings.clone());
    let target = target_bindings.into_iter().collect::<BTreeMap<_, _>>();
    let count = if bare {
        0
    } else {
        dol::u32(package, 0)? as usize
    };
    for index in 2..count {
        let Some(native) = field_core::optional_section(package, index)? else {
            continue;
        };
        // Actor packages reserve their final two members for optional collision.
        if index >= 29 && !actor_motion(native) {
            continue;
        }
        let slot = u16::try_from(4 + index * 4)?;
        let expanded = if dol::u32(native, 0)? == 0x007b_7960 {
            native.to_vec()
        } else {
            compression::decode(native)?
        };
        let motion = world_motion::encode_bound(&expanded, &mesh, &bindings)
            .with_context(|| format!("local actor {section} motion {slot}"))?;
        let path = if let Some(path) = motions.get(&motion) {
            path.clone()
        } else {
            let label = if slot == 12 {
                "idle".to_owned()
            } else {
                format!("motion-{slot}")
            };
            let path = output.write(&format!("-{label}.glb"), &motion)?;
            motions.insert(motion, path.clone());
            path
        };
        part.clips.push(clip(path, slot, None));
    }
    for (id, path, joints, rig) in shared {
        let mut binding = clip(path.clone(), 12, Some(*id));
        binding.joints = shared_joint_map(&bindings, &target, joints, rig).into();
        part.clips.push(binding);
    }
    for &index in local_motions {
        let motion = field_core::optional_section(archive, 16 + index)?
            .context("field local motion is missing")?;
        let joints = world_motion::indexed_joints(motion)?;
        let source_names = world_motion::indexed_track_names(motion)?;
        let mut binding = clip(
            format!("{root}/animations/local-motion-{index}.glb"),
            12,
            Some(LOCAL_BASE + index as u32),
        );
        binding.joints = bindings
            .iter()
            .filter(|(id, _)| joints.binary_search(id).is_ok())
            .filter_map(|(id, _)| {
                Some((
                    target.get(id)?.clone(),
                    Some(
                        source_names
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| format!("joint-{id}")),
                    ),
                ))
            })
            .collect::<BTreeMap<_, _>>()
            .into();
        part.clips.push(binding);
    }
    let collision = if bare {
        ModelCollision::default()
    } else {
        actor_collision(package)?
    };
    let collision = if collision
        .floors
        .iter()
        .chain(&collision.solids)
        .any(|group| !group.triangles.is_empty())
    {
        let path = output.write("-collision.glb", &collision_mesh::encode_model(&collision)?)?;
        Some(path)
    } else {
        None
    };
    let outline = if bare {
        None
    } else {
        field_core::optional_section(package, 1)?
    };
    ensure!(
        outline.is_none_or(world_gpl::is_model),
        "actor outline is not a GPL model"
    );
    let hidden_geometry = {
        part.prepare_bones(&mesh)?;
        // The Ice Temple bird uses its second TPL image as a two-row eye atlas.
        if matches!(
            (root, section),
            ("fields/temple-of-ice-0323", 20) | ("fields/temple-of-ice-0324", 25)
        ) {
            ensure!(paths.len() == 4, "Ice Temple bird palette changed");
            part.appearance = Some(AppearanceTextures {
                eyes: Some(1),
                mouth: None,
                variant: None,
                costume: None,
            });
            for material in &mut part.materials {
                if material.color.is_none() && material.multiply.is_none() {
                    material.blend = false;
                }
            }
        }
        part.bone_names
            .iter()
            .filter(|name| name.starts_with("kk"))
            .cloned()
            .collect()
    };
    let mut parts = vec![part];
    if let Some(outline) = outline {
        let own_palette = dol::u32(outline, 0)? != 0;
        let normalized = if own_palette {
            outline.to_vec()
        } else {
            world_gpl::with_primary_palette(body, outline)?
        };
        let outline_paths = if own_palette {
            let tpl = world_texture::model_palette(&normalized)?;
            let mut outline_paths = Vec::new();
            for index in 0..dol::u32(tpl, 4)? as usize {
                let pixels = world_texture::tpl_pixels(tpl, index, 0, false, true)?;
                outline_paths.push(output.texture(
                    textures,
                    &pixels,
                    &format!("-part-1-texture-{}.ktx2", index + 1),
                )?);
            }
            outline_paths
        } else {
            paths.clone()
        };
        let (outline_mesh, mut part) = world_gpl::static_model(
            &normalized,
            outline_paths.clone(),
            vec![false; outline_paths.len()],
        )?;
        part.mesh = output.write("-part-1.glb", &outline_mesh)?;
        part.resource = 1;
        part.outline_color = Some([0, 0, 0, 127]);
        part.clips = parts[0].clips.clone();
        for material in &mut part.materials {
            material.cull = CullFace::Front;
            material.blend = true;
            material.draw_order += 65536;
        }
        parts.push(part);
    }
    let model = Model {
        parts,
        collision,
        hidden_geometry,
    };
    output.model(&model)
}

/// A standalone GPL with an embedded TPL is a local model even without an
/// actor-package header (Gaoracchia's cone is one native example).
fn bare_local_model(bytes: &[u8]) -> Result<bool> {
    if bytes.len() < 36 || dol::u32(bytes, 32)? != 0x005b_bc61 {
        return Ok(false);
    }
    let start = dol::u32(bytes, 0)? as usize;
    let end = dol::u32(bytes, 4)? as usize;
    Ok(start >= 36
        && start + 8 <= end
        && end <= bytes.len()
        && dol::u32(bytes, start)? == 0x0020_af30)
}

/// Convert a field's native local actor packages.
pub fn cook_field_local_models(extracted: &Path, library: &Path, id: u32) -> Result<()> {
    let (exe, archive) = field_core::read(extracted, id)?;
    let banks = field_actors::animation_banks(extracted, &exe, &archive)?;
    cook_local_models(extracted, library, id, &archive, &banks)
}

pub(crate) fn cook_local_models(
    extracted: &Path,
    library: &Path,
    id: u32,
    archive: &[u8],
    banks: &BTreeMap<u32, String>,
) -> Result<()> {
    let root = format!("fields/{}", field_core::field_name(id as usize));
    let shared = shared_clips(extracted, library, id, banks)?;
    let local_motions = local_motion_sections(archive)?;
    let mut motions = BTreeMap::new();
    let mut textures = BTreeMap::new();
    for (section, actor) in local_models(id, archive)? {
        let base = actor
            .model
            .strip_suffix(".json")
            .context("local model lacks .json")?;
        convert(
            archive,
            &root,
            section,
            &shared,
            &local_motions,
            ModelOutput { library, base },
            &mut motions,
            &mut textures,
        )?;
    }
    Ok(())
}

fn cook_separate_palette(archive: &[u8], section: usize, output: ModelOutput<'_>) -> Result<()> {
    let base = output.base;
    let body = field_core::optional_section(archive, section)?.context("local GPL missing")?;
    let tpl =
        field_core::optional_section(archive, section + 1)?.context("local palette missing")?;
    ensure!(
        dol::u32(body, 0)? == 0 && dol::u32(tpl, 0)? == 0x0020_af30 && dol::u32(tpl, 4)? == 1,
        "local model {base} has an unexpected GPL/TPL shape"
    );
    // The bare GPL leaves its geometry end unset and has no model hierarchy.
    // Its separate TPL supplies texture pixels, not a trailing model section.
    let mut body = body.to_vec();
    let end = u32::try_from(body.len())?;
    body[..4].copy_from_slice(&end.to_be_bytes());
    let (texture, transparent) = world_texture::tpl_texture(tpl, 0)?;
    let texture_path = output.write("-texture-1.ktx2", &texture)?;
    let (mesh, mut part) = world_gpl::static_model(&body, vec![texture_path], vec![transparent])?;
    part.mesh = output.write(".glb", &mesh)?;
    output.model(&Model {
        parts: vec![part],
        collision: None,
        hidden_geometry: Vec::new(),
    })
}

fn cook_scene_capture(archive: &[u8], section: usize, output: ModelOutput<'_>) -> Result<()> {
    let base = output.base;
    let body = field_core::optional_section(archive, section)?.context("capture GPL missing")?;
    ensure!(
        dol::u32(body, 0)? == 0 && dol::u32(body, 4)? == 0 && dol::u32(body, 32)? == 0x005b_bc61,
        "local model {base} has an unexpected capture GPL"
    );
    let mut body = body.to_vec();
    let end = u32::try_from(body.len())?;
    body[..4].copy_from_slice(&end.to_be_bytes());
    // The native draw samples the current scene capture as texture stage 0.
    // Supply that stage for GPL decoding, then express it as the editable
    // capture role rather than a nonexistent image file.
    let (mesh, mut part) = world_gpl::static_model(&body, vec!["capture".into()], vec![false])?;
    ensure!(
        part.materials.len() == 1,
        "local capture model {base} has multiple draws"
    );
    part.textures.clear();
    let material = &mut part.materials[0];
    material.image = SceneImage::Capture;
    material.color = None;
    material.multiply = None;
    material.vertex_color = true;
    material.blend = false;
    part.mesh = output.write(".glb", &mesh)?;
    output.model(&Model {
        parts: vec![part],
        collision: None,
        hidden_geometry: Vec::new(),
    })
}
