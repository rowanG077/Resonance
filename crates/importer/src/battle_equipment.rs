//! Physical equipment directly from the native battle weapon archive.
use crate::{
    battle_party_model, character_models, compression, dol, field_core, rel::Rel, world_gpl,
    world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CullFace, SceneClip,
    animation::Skeleton,
    battle_model::{
        AttachmentAnimation, AttachmentTextureAnimation, TrailMaterial, TrailTexture,
        document::{
            Equipment, EquipmentKind, Model, TrailDefinition, TrailStyle, WeaponDefinition,
        },
    },
    model_preview::PreviewPart,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    num::NonZeroU16,
    path::Path,
};
use symphonia_constants::ItemId;

fn sections(bytes: &[u8]) -> Result<Vec<Option<&[u8]>>> {
    let count = dol::u32(bytes, 0)? as usize;
    ensure!(
        (1..=16).contains(&count),
        "invalid equipment section count {count}"
    );
    let offsets = (0..count)
        .map(|i| Ok(dol::u32(bytes, 4 + i * 4)? as usize))
        .collect::<Result<Vec<_>>>()?;
    offsets
        .iter()
        .map(|&start| {
            if start == 0 {
                return Ok(None);
            }
            ensure!(
                start >= 4 + count * 4,
                "equipment section overlaps directory"
            );
            let end = offsets
                .iter()
                .copied()
                .filter(|&other| other > start)
                .min()
                .unwrap_or(bytes.len());
            Ok(Some(
                bytes
                    .get(start..end)
                    .context("equipment section exceeds package")?,
            ))
        })
        .collect()
}

fn material(bytes: &[u8]) -> Result<TrailMaterial> {
    let bytes = bytes.get(..0x1c).context("short equipment metadata")?;
    ensure!(bytes[0x0f] <= 1, "unsupported equipment trail blend");
    Ok(TrailMaterial {
        texture: match bytes[0x0d] as i8 {
            ..0 => None,
            2 => Some(TrailTexture::Enemy { slot: 2 }),
            slot => Some(TrailTexture::Common { slot: slot as u8 }),
        },
        palette: bytes[0x0e],
        additive: bytes[0x0f] == 1,
        color: std::array::from_fn(|i| f32::from(bytes[0x10 + i]) / 128.),
        uv: std::array::from_fn(|i| i16::from_be_bytes([bytes[0x14 + i * 2], bytes[0x15 + i * 2]])),
    })
}

pub(crate) fn trail(glb: &[u8], metadata: &[u8]) -> Result<Option<TrailDefinition>> {
    let skeleton = Skeleton::from_glb(glb)?;
    // Native endpoint indices start at zero; repeated KI slots leave an
    // unassigned endpoint at the root rather than removing that trail point.
    let mut markers =
        std::array::from_fn::<_, 3, _>(|_| skeleton.bones.first().map(|bone| bone.name.clone()));
    let mut count = 0;
    for bone in &skeleton.bones {
        let name = bone.name.as_bytes();
        if name.len() < 2 || !name[..2].eq_ignore_ascii_case(b"KI") {
            continue;
        }
        ensure!(
            name.len() >= 4 && (b'0'..=b'2').contains(&name[3]),
            "invalid equipment trail endpoint"
        );
        ensure!(count < 3, "too many equipment trail endpoints");
        markers[usize::from(name[3] - b'0')] = Some(bone.name.clone());
        count += 1;
    }
    if count < 2 {
        return Ok(None);
    }
    let joints = markers[..count]
        .iter()
        .map(|name| name.clone().context("equipment trail marker is absent"))
        .collect::<Result<Vec<_>>>()?;
    let native = material(metadata)?;
    let palette: BTreeMap<String, TrailMaterial> =
        serde_json::from_str(include_str!("battle-trail-materials.json"))?;
    let style = palette
        .into_iter()
        .find(|(_, candidate)| *candidate == native)
        .map(|(name, _)| TrailStyle::Named(name))
        .unwrap_or(TrailStyle::Inline(native));
    Ok(Some(TrailDefinition {
        joints,
        material: style,
    }))
}

pub(crate) fn texture_animation(bytes: &[u8]) -> Result<Option<AttachmentAnimation>> {
    ensure!(bytes.len() == 64, "invalid equipment metadata size");
    let channel = |index: usize| -> Result<Option<AttachmentTextureAnimation>> {
        let mode = bytes[4 + index];
        if bytes[index] == 0 || mode == 0 {
            return Ok(None);
        }
        ensure!(
            bytes[index] == 1,
            "equipment animation uses multiple textures"
        );
        let texture = u16::try_from(bytes[2 + index] as i8)?;
        let parameter = bytes[6 + index] as i8;
        Ok(Some(match mode {
            2 | 5 => {
                let axis = usize::from(mode == 2);
                let mut offset_milli_uv = [0; 2];
                let mut step_milli_uv = [0; 2];
                offset_milli_uv[axis] =
                    i16::from_be_bytes([bytes[28 + index * 2], bytes[29 + index * 2]]);
                step_milli_uv[axis] = i16::from(parameter);
                AttachmentTextureAnimation::Scroll {
                    texture,
                    step_milli_uv,
                    offset_milli_uv,
                }
            }
            1 | 4 => {
                let rows = u8::try_from(parameter)?;
                ensure!(rows > 0, "empty equipment atlas");
                let ticks_per_frame = NonZeroU16::new(u16::try_from(bytes[8 + index] as i8)? + 1)
                    .context("empty equipment frame duration")?;
                let mut frames = if mode == 4 {
                    let length = usize::from(bytes[11]);
                    ensure!(
                        (1..=36).contains(&length),
                        "invalid equipment frame sequence"
                    );
                    bytes[28..28 + length].to_vec()
                } else {
                    Vec::new()
                };
                ensure!(
                    frames.iter().all(|&frame| frame < rows),
                    "equipment frame exceeds atlas"
                );
                if frames.iter().copied().eq(0..rows) {
                    frames.clear();
                }
                AttachmentTextureAnimation::Atlas {
                    texture,
                    rows,
                    ticks_per_frame,
                    frames,
                }
            }
            _ => anyhow::bail!("unsupported equipment animation mode {mode}"),
        }))
    };
    let animation = AttachmentAnimation {
        primary: channel(0)?,
        independent: channel(1)?,
        effect_every_ticks: None,
    };
    Ok((!animation.is_empty()).then_some(animation))
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

fn native_package(root: &Path, slot: usize) -> Result<Vec<u8>> {
    let files = root.join("disc1/files");
    let module = Rel::read(fs::read(files.join("US_r_Top2Btl.rel"))?)?;
    let directory = module.slice(5, 0x6e8, 158 * 4)?;
    let start = dol::u32(directory, slot * 4)? as usize;
    let end = directory[(slot + 1) * 4..]
        .chunks_exact(4)
        .map(|row| dol::u32(row, 0))
        .find(|value| !matches!(value, Ok(0)))
        .context("equipment archive has no range end")?? as usize;
    ensure!(start < end, "empty equipment package");
    let mut archive = fs::File::open(files.join("BTL/BTLwepon.dat"))?;
    ensure!(
        end <= archive.metadata()?.len() as usize,
        "equipment range exceeds archive"
    );
    archive.seek(SeekFrom::Start(start as u64))?;
    let mut bytes = vec![0; end - start];
    archive.read_exact(&mut bytes)?;
    compression::cabinet(&bytes)
}

fn equipment_name(item: u16) -> Result<String> {
    Ok(match item {
        528 => "blue-shield-prop".into(),
        529 | 531 | 532 => format!("equipment-{item:04}"),
        _ => ItemId::try_from(item)?
            .resource_name()
            .context("equipment has no readable name")?
            .into(),
    })
}

fn convert(root: &Path, item: u16) -> Result<BTreeMap<String, Vec<u8>>> {
    let (slot, kind) = match item {
        135..=273 => (usize::from(item - 135), EquipmentKind::Weapon),
        356..=366 => (usize::from(item - 217), EquipmentKind::Shield),
        529 | 531 | 532 => (usize::from(item - 379), EquipmentKind::Weapon),
        _ => anyhow::bail!("item has no named native weapon or shield"),
    };
    let name = equipment_name(item)?;
    let base = format!("meshes/equipment/{name}");
    let owner_bank = (175..=189)
        .contains(&item)
        .then(|| battle_party_model::motion_bank(root, 3, 0))
        .transpose()?;
    let package = native_package(root, slot)?;
    let (weapon, mut files) = convert_package(&package, &base, owner_bank.as_deref(), Some(item))?;
    files.insert(
        format!("{base}/equipment.json"),
        resonance_asset_writer::json::encode(&Equipment {
            kind,
            parts: weapon.parts,
            trails: weapon.trails,
            uv_animation: weapon.uv_animation,
        })?,
    );
    Ok(files)
}

/// Convert the native equipment package shape shared by inventory weapons and
/// the weapon embedded in Falcon's Crest. Paths are rooted at the caller's owner.
pub(crate) fn convert_package(
    package: &[u8],
    base: &str,
    owner_bank: Option<&[u8]>,
    item: Option<u16>,
) -> Result<(WeaponDefinition, BTreeMap<String, Vec<u8>>)> {
    let directory = sections(&package).context("equipment package directory")?;
    // Multi-part weapons wrap part directories in an outer directory. Simple
    // shields and several weapons use the first directory as the part itself.
    let packages = if directory
        .first()
        .and_then(Option::as_ref)
        .is_some_and(|part| sections(part).is_ok())
    {
        directory
    } else {
        vec![Some(package)]
    };
    let mut files = BTreeMap::new();
    let mut parts = BTreeMap::new();
    let mut trails = BTreeMap::new();
    let mut uv_animation = BTreeMap::new();
    let mut decoded =
        BTreeMap::<Vec<u8>, (Model, Option<TrailDefinition>, Option<AttachmentAnimation>)>::new();
    for (index, package) in packages.into_iter().enumerate() {
        let Some(package) = package else { continue };
        let (model, ribbon, animation) = if let Some(existing) = decoded.get(package) {
            existing.clone()
        } else {
            let members =
                sections(package).with_context(|| format!("equipment part {index} directory"))?;
            ensure!(
                (5..=7).contains(&members.len()),
                "unsupported equipment layer package"
            );
            ensure!(members[4].is_none(), "equipment has reserved layer");
            let metadata = members[0].context("equipment metadata is absent")?;
            ensure!(
                metadata.len() == 64
                    && metadata[10] == 0
                    && metadata[12] == 0
                    && metadata[19] == 255,
                "equipment has unsupported appearance controls"
            );
            let animation = texture_animation(metadata)?;
            let mut layers: Vec<PreviewPart> = Vec::new();
            let mut ribbon = None;
            let mut independent_layers = BTreeSet::new();
            let primary = members[1].context("equipment model is absent")?;
            let mut primary_palette = None::<(Vec<String>, Vec<bool>)>;
            let mut primary_clips = Vec::new();
            for (source_slot, additive, outline) in
                [(1, false, false), (2, false, true), (5, true, false)]
            {
                let Some(source) = members.get(source_slot).and_then(Option::as_ref) else {
                    continue;
                };
                let borrowed = outline && dol::u32(source, 0)? == 0;
                let normalized = borrowed
                    .then(|| world_gpl::with_primary_palette(primary, source))
                    .transpose()?;
                let source = normalized.as_deref().unwrap_or(source);
                let palette_start = dol::u32(source, 0)? as usize;
                let palette_end = dol::u32(source, 4)? as usize;
                let tpl = source
                    .get(palette_start..palette_end)
                    .context("equipment palette exceeds model")?;
                let texture_count = if palette_start == 0 {
                    0
                } else {
                    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing equipment TPL");
                    dol::u32(tpl, 4)? as usize
                };
                let layer = layers.len();
                let stem = format!("{base}/part-{index}-layer-{layer}");
                let (paths, alpha) = if borrowed {
                    primary_palette
                        .clone()
                        .context("outline lacks primary palette")?
                } else {
                    let mut paths = Vec::new();
                    let mut alpha = Vec::new();
                    for texture in 0..texture_count {
                        let path = format!("{stem}-texture-{texture}.ktx2");
                        let (pixels, transparent) = world_texture::tpl_texture(tpl, texture)?;
                        files.insert(path.clone(), pixels);
                        paths.push(path);
                        alpha.push(transparent);
                    }
                    if layer == 0 {
                        primary_palette = Some((paths.clone(), alpha.clone()));
                    }
                    (paths, alpha)
                };
                let (glb, mut scene) = world_gpl::static_model(source, paths, alpha)?;
                scene.mesh = format!("{stem}.glb");
                scene.resource = u16::try_from(layer)?;
                if outline {
                    scene.secondary_motion = layers[0].scene.secondary_motion.clone();
                } else {
                    let bindings = world_gpl::animation_bindings(source)?;
                    if bindings.iter().any(|(_, name)| name.starts_with("AB_ROOT")) {
                        scene.prepare_bones(&glb)?;
                        let json_len = u32::from_le_bytes(
                            glb.get(12..16).context("short equipment GLB")?.try_into()?,
                        ) as usize;
                        let gltf: serde_json::Value = serde_json::from_slice(
                            glb.get(20..20 + json_len)
                                .context("short equipment GLB JSON")?,
                        )?;
                        let native_names = bindings
                            .into_iter()
                            .map(|(_, name)| name)
                            .collect::<Vec<_>>();
                        scene.secondary_motion = character_models::character_dynamics::bind(
                            "",
                            &gltf,
                            &native_names,
                            &scene.bone_names,
                        )?;
                    }
                }
                if !outline && !additive && !matches!(item, Some(179 | 186)) {
                    for material in &mut scene.materials {
                        if material.color.is_none() && material.multiply.is_none() {
                            material.blend = false;
                        }
                    }
                }
                if outline || additive {
                    let span =
                        u32::from(scene.resource) * resonance_content::field::MODEL_DRAW_SPAN;
                    for material in &mut scene.materials {
                        material.draw_order += span;
                        if outline || additive {
                            material.cull = if outline {
                                CullFace::Front
                            } else {
                                CullFace::None
                            };
                            material.blend = true;
                        }
                        material.depth_write = !additive;
                    }
                    scene.outline_color = outline.then_some([0, 0, 0, 127]);
                }
                if additive {
                    independent_layers.insert(scene.resource);
                } else if !outline {
                    ribbon = trail(&glb, metadata)?;
                }
                if outline {
                    scene.clips = primary_clips.clone();
                    // One World's outline uses Northern Lights' motion binding.
                    if item == Some(187) {
                        ensure!(index == 0 && layer == 1, "unexpected One World outline");
                        for clip in &mut scene.clips {
                            clip.motion = format!(
                                "meshes/equipment/northern-lights/part-0-layer-0-animation-{}.glb",
                                clip.resource_slot
                            );
                        }
                    }
                    scene.autoplay = members[3].is_some();
                } else {
                    let motion_slot = if additive { 6 } else { 3 };
                    if let Some(native) = members.get(motion_slot).and_then(Option::as_ref) {
                        ensure!(
                            additive || owner_bank.is_none(),
                            "owner-linked equipment also has local motion"
                        );
                        let motion = battle_party_model::expanded_motion(native)?;
                        let bindings = world_gpl::animation_bindings(source)?;
                        let path = format!(
                            "{base}/part-{index}-layer-{}-animation-0.glb",
                            if additive { layer } else { 0 }
                        );
                        files.insert(
                            path.clone(),
                            world_motion::encode_bound(&motion, &glb, &bindings)?,
                        );
                        scene.clips.push(clip(path, 0));
                        scene.autoplay = true;
                    }
                    if !additive {
                        if let Some(bank) = &owner_bank {
                            let count = dol::u32(bank, 0)? as usize;
                            let bindings = world_gpl::animation_bindings(source)?;
                            for member in 62..count {
                                let Some(native) = field_core::optional_section(bank, member)?
                                else {
                                    continue;
                                };
                                let motion = battle_party_model::expanded_motion(native)?;
                                let slot = u16::try_from(member - 2)?;
                                // Thunderbolt has the same bound motions as Northern Lights.
                                let path = if item == Some(180) {
                                    ensure!(
                                        index == 0 && layer == 0,
                                        "unexpected Thunderbolt part"
                                    );
                                    format!(
                                        "meshes/equipment/northern-lights/part-0-layer-0-animation-{slot}.glb"
                                    )
                                } else {
                                    let path =
                                        format!("{base}/part-{index}-layer-0-animation-{slot}.glb");
                                    files.insert(
                                        path.clone(),
                                        world_motion::encode_bound(&motion, &glb, &bindings)?,
                                    );
                                    path
                                };
                                scene.clips.push(clip(path, slot));
                            }
                        }
                        primary_clips = scene.clips.clone();
                    }
                }
                files.insert(scene.mesh.clone(), glb);
                layers.push(PreviewPart {
                    animation: scene.autoplay.then_some(0),
                    scene,
                    attached_to: None,
                    additive,
                    uv_offsets: Vec::new(),
                });
            }
            ensure!(!layers.is_empty(), "equipment model is absent");
            let model = Model {
                attachment_nodes: Default::default(),
                independent_layers,
                layers,
            };
            decoded.insert(
                package.to_vec(),
                (model.clone(), ribbon.clone(), animation.clone()),
            );
            (model, ribbon, animation)
        };
        let slot = u8::try_from(index)?;
        parts.insert(slot, model);
        if let Some(ribbon) = ribbon {
            trails.insert(slot, ribbon);
        }
        if let Some(animation) = animation {
            uv_animation.insert(slot, animation);
        }
    }
    ensure!(!parts.is_empty(), "equipment has no model parts");
    // A few native parts differ in metadata but yield identical editable media.
    // Share those files inside this equipment owner instead of publishing copies.
    let mut canonical = Vec::<(&String, &Vec<u8>)>::new();
    let mut aliases = BTreeMap::new();
    for (path, bytes) in &files {
        if let Some((owner, _)) = canonical.iter().find(|(owner, data)| {
            Path::new(owner).extension() == Path::new(path).extension() && *data == bytes
        }) {
            aliases.insert(path.clone(), (*owner).clone());
        } else {
            canonical.push((path, bytes));
        }
    }
    for model in parts.values_mut() {
        for layer in &mut model.layers {
            let scene = &mut layer.scene;
            if let Some(path) = aliases.get(&scene.mesh) {
                scene.mesh = path.clone();
            }
            for texture in &mut scene.textures {
                if let Some(path) = aliases.get(texture) {
                    *texture = path.clone();
                }
            }
            for clip in &mut scene.clips {
                if let Some(path) = aliases.get(&clip.motion) {
                    clip.motion = path.clone();
                }
            }
        }
    }
    files.retain(|path, _| !aliases.contains_key(path));
    Ok((
        WeaponDefinition {
            parts,
            trails,
            uv_animation,
        },
        files,
    ))
}

pub fn cook_battle_equipment(root: &Path, library: &Path, item: u16) -> Result<()> {
    let files = if item == 528 {
        let source = library.join("meshes/equipment/blue-shield/equipment.json");
        let mut shield: Equipment =
            serde_json::from_slice(&fs::read(&source).with_context(|| {
                format!(
                    "cook Blue Shield before its carried prop: {}",
                    source.display()
                )
            })?)?;
        ensure!(
            shield.kind == EquipmentKind::Shield,
            "Blue Shield prop has no shield owner"
        );
        shield.kind = EquipmentKind::Weapon;
        BTreeMap::from([(
            "meshes/equipment/blue-shield-prop/equipment.json".into(),
            resonance_asset_writer::json::encode(&shield)?,
        )])
    } else {
        convert(root, item)?
    };
    ensure!(
        files.keys().all(|path| !library.join(path).exists()),
        "equipment output already exists"
    );
    for (path, bytes) in files {
        let output = library.join(path);
        crate::output::write(&output, &bytes)?;
    }
    Ok(())
}

/// Publish the native equipment lookup without cooking any model or texture.
pub fn cook_battle_equipment_mappings(root: &Path, library: &Path) -> Result<Vec<u16>> {
    let files = root.join("disc1/files");
    let module = Rel::read(fs::read(files.join("US_r_Top2Btl.rel"))?)?;
    let directory = module.slice(5, 0x6e8, 158 * 4)?;
    let offsets = directory
        .chunks_exact(4)
        .map(|row| Ok(dol::u32(row, 0)? as usize))
        .collect::<Result<Vec<_>>>()?;
    let archive_len = usize::try_from(fs::metadata(files.join("BTL/BTLwepon.dat"))?.len())?;
    ensure!(
        offsets[156] == archive_len && offsets[157] == 0,
        "invalid equipment archive end"
    );
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let mut entries = BTreeMap::new();
    let mut names = BTreeSet::new();
    for slot in 0..156_usize {
        let present = slot == 0 || offsets[slot] != 0;
        if !present {
            continue;
        }
        ensure!(
            offsets[slot] < archive_len,
            "equipment slot {slot} exceeds archive"
        );
        let item = u16::try_from(if slot < 139 {
            slot + 135
        } else if slot < 150 {
            slot + 217
        } else {
            slot + 379
        })?;
        let name = equipment_name(item)?;
        ensure!(
            names.insert(name.to_ascii_lowercase()),
            "duplicate equipment owner {name}"
        );
        ensure!(
            entries.insert(u32::from(item), Some(name)).is_none(),
            "duplicate equipment ID {item}"
        );
    }
    // The same physical shield has a distinct battle-only carried-prop role.
    ensure!(offsets[149] != 0, "missing Blue Shield prop source");
    let prop = equipment_name(528)?;
    ensure!(
        names.insert(prop.to_ascii_lowercase()),
        "duplicate equipment prop name"
    );
    entries.insert(528, Some(prop));
    for item in 367..=391_u32 {
        let row = dol::slice(&exe, 0x801f_ad98 + item * 60, 60)?;
        ensure!(
            (32..=34).contains(&row[0x1a]),
            "item {item} unexpectedly has a visual model"
        );
        ensure!(
            entries.insert(item, None).is_none(),
            "duplicate equipment item {item}"
        );
    }
    ensure!(
        entries.len() == 179 && names.len() == 154,
        "incomplete equipment catalogue"
    );
    let path = library.join("meshes/equipment/mappings.json");
    crate::output::json(&path, &entries)?;
    entries
        .into_iter()
        .filter_map(|(id, name)| name.map(|_| u16::try_from(id).map_err(Into::into)))
        .collect()
}
