//! Character mesh layers and animation tables, resolved offline from their
//! original files. Runtime assets contain no relocations or resource pointers.
use crate::{
    animation::AuthoredAnimation,
    digest,
    field::sections,
    scene::{PartSource, SourceClip, cook_part},
};
use anyhow::{Context, Result, ensure};
use resonance_content::field::ActorAssets;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

fn word(data: &[u8], at: usize) -> Result<usize> {
    Ok(crate::read::u32(data, at)? as usize)
}

/// Count followed by offset/length pairs; missing entries fall back to entry zero.
pub(crate) fn archive_entry(data: &[u8], index: usize) -> Result<&[u8]> {
    let count = word(data, 0)?;
    ensure!(
        count <= 65536 && index < count && 4 + count * 8 <= data.len(),
        "invalid resource archive index"
    );
    let entry = if word(data, 8 + index * 8)? == 0 {
        0
    } else {
        index
    };
    let start = word(data, 4 + entry * 8)?;
    let size = word(data, 8 + entry * 8)?;
    ensure!(
        start >= 4 + count * 8 && size > 0,
        "invalid resource archive payload"
    );
    data.get(start..start.checked_add(size).context("resource range overflow")?)
        .context("resource archive payload exceeds file")
}

fn section<'a>(bytes: &'a [u8], ranges: &[Option<Range<usize>>], index: usize) -> Result<&'a [u8]> {
    let range = ranges
        .get(index)
        .and_then(Option::as_ref)
        .context("missing character layer")?;
    Ok(&bytes[range.clone()])
}

/// A field's model bank starts with an offset table. Its first section maps
/// model indices to script IDs; remaining sections are complete actor packages.
fn field_models(bytes: &[u8]) -> Result<Vec<(u16, &[u8])>> {
    let mut ids = std::collections::BTreeSet::new();
    field_model_entries(bytes)?
        .into_iter()
        .map(|(id, range)| {
            ensure!(
                id > 9 && id != 24 && ids.insert(id),
                "duplicate or reserved field model ID {id}"
            );
            Ok((id, &bytes[range]))
        })
        .collect()
}

/// Physical field model entries, independent of runtime actor admission.
/// The first offset addresses IDs; subsequent offsets address model packages.
pub(crate) fn field_model_count(bytes: &[u8]) -> Result<usize> {
    let count = word(bytes, 0)?;
    ensure!(
        count > 0 && count <= bytes.len().saturating_sub(4) / 4,
        "invalid field model count"
    );
    let ids = word(bytes, 4)? & !3;
    ensure!(
        (count == 1 && ids == 0)
            || (ids >= 4 + count * 4
                && bytes
                    .get(ids..)
                    .is_some_and(|ids| ids.len() >= (count - 1) * 2)),
        "invalid field model ID table"
    );
    Ok(count)
}

pub(crate) fn field_model_entries(bytes: &[u8]) -> Result<Vec<(u16, Range<usize>)>> {
    let count = field_model_count(bytes)?;
    let header = 4 + count * 4;
    let offsets = (0..count)
        .map(|i| word(bytes, 4 + i * 4).map(|v| v & !3))
        .collect::<Result<Vec<_>>>()?;
    if count == 1 && offsets[0] == 0 {
        return Ok(Vec::new());
    }
    ensure!(
        offsets.iter().all(|v| (header..bytes.len()).contains(v)),
        "invalid field model offset"
    );
    let end = |start| {
        offsets
            .iter()
            .copied()
            .filter(|v| *v > start)
            .min()
            .unwrap_or(bytes.len())
    };
    let names = &bytes[offsets[0]..end(offsets[0])];
    ensure!((count - 1) * 2 <= names.len(), "truncated field model IDs");
    offsets
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, &offset)| Ok((crate::read::u16(names, (i - 1) * 2)?, offset..end(offset))))
        .collect()
}

/// A secondary mesh layer can share the primary layer's texture palette.
/// Assemble an offline source view for the existing geometry decoder; all GPL
/// and model-relative offsets remain unchanged.
pub(crate) fn texture_palette(primary: &[u8], secondary: &[u8]) -> Result<Vec<u8>> {
    if word(secondary, 0)? != 0 {
        return Ok(secondary.to_vec());
    }
    const EMPTY_PALETTE: [u8; 12] = [0, 0x20, 0xaf, 0x30, 0, 0, 0, 0, 0, 0, 0, 12];
    let texture = if word(primary, 0)? == 0 {
        // Color-only meshes and attachment placeholders need no texture table.
        // A textured draw without a real palette still fails material binding.
        &EMPTY_PALETTE[..]
    } else {
        primary
            .get(word(primary, 0)?..word(primary, 4)?)
            .context("invalid primary texture palette")?
    };
    let end = match word(secondary, 4)? {
        // Bare untextured geometry has no trailing palette or model boundary.
        0 => secondary.len(),
        end => end,
    };
    ensure!(
        (0x20..=secondary.len()).contains(&end),
        "invalid secondary geometry range"
    );
    let mut data = secondary[..end].to_vec();
    data.extend(texture);
    data.extend(&secondary[end..]);
    data[..4].copy_from_slice(&(end as u32).to_be_bytes());
    data[4..8].copy_from_slice(&((end + texture.len()) as u32).to_be_bytes());
    Ok(data)
}

#[derive(Default)]
pub(crate) struct Sources {
    packages: BTreeMap<u32, Vec<u8>>,
    animations: BTreeSet<u32>,
    pub(crate) textures: BTreeSet<u32>,
}

impl Sources {
    pub(crate) fn scene_clips(
        &self,
        cooked: &mut crate::field_resources::binding::Resources<'_>,
    ) -> Result<Vec<(u32, AuthoredAnimation)>> {
        self.animations
            .iter()
            .map(|&id| Ok((id, cooked.animation(id)?)))
            .collect()
    }

    pub(crate) fn read(
        files: &Path,
        catalogue: &crate::resource::Catalogue,
        declared: &BTreeSet<u32>,
    ) -> Result<Self> {
        let groups: BTreeSet<_> = declared
            .iter()
            .map(|id| id >> 16)
            .filter(|g| *g > 0)
            .collect();
        let read = |id| -> Result<_> {
            let path = crate::field_resources::resolve_path(files, catalogue.source(id)?)?;
            Ok(fs::read(files.join(path))?)
        };
        let archives: BTreeMap<_, _> = groups
            .into_iter()
            .map(|group| Ok((group, read(group << 16)?)))
            .collect::<Result<_>>()?;
        let mut sources = Self::default();
        for &id in declared {
            let bytes = if id >> 16 == 0 {
                read(id)?
            } else {
                archive_entry(&archives[&(id >> 16)], (id & 0xffff) as usize)?.to_vec()
            };
            match word(&bytes, 0)? {
                31 => {
                    // Validate the package directory and primary layer before admission.
                    section(&bytes, &sections(&bytes)?, 0)?;
                    sources.packages.insert(id, bytes);
                }
                0x0020af30 => {
                    crate::tpl::parse_tpl(&bytes)?;
                    sources.textures.insert(id);
                }
                _ => {
                    let animation = decode_clip(&bytes)
                        .with_context(|| format!("decode field resource {id:#x}"))?;
                    ensure!(
                        crate::animation::is_animation(&animation),
                        "field resource {id:#x} is not an actor package or animation"
                    );
                    sources.animations.insert(id);
                }
            }
        }
        Ok(sources)
    }
}

pub(crate) fn cook_field(
    extracted: &Path,
    output: &Path,
    map_id: u32,
    map: &crate::field::MapArchive,
    sources: &Sources,
    shared: &[(u32, AuthoredAnimation)],
) -> Result<Vec<ActorAssets>> {
    let files = extracted.join("files");
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let resources = crate::resource::read(&executable)?;
    let declarations = crate::field_resources::declarations(map.section(6)?)?;
    let mut model_resources = declarations.resources.clone();
    for id in &sources.textures {
        model_resources.remove(id);
    }
    let mut assets = Vec::new();
    // Scripts choose actor and animation handles independently. Prepare every
    // pairing instead of assigning a guessed owner to a shared clip.
    let package = |id, name: &str, data: &[u8]| cook(id, name, data, data, &[], shared, output);
    for id in 1..=resources.party_bodies.len() as u32 {
        let model_path = resources.party(crate::resource::PartyResource::Body, id as u8, 0)?;
        let name = model_path
            .strip_suffix("000.bin")
            .context("unexpected default party model path")?;
        let model =
            fs::read(files.join(crate::field_resources::resolve_path(&files, model_path)?))?;
        let animation_path =
            crate::field_resources::resolve_path(&files, resources.field_motion(id as u8)?)?;
        let animation = fs::read(files.join(animation_path))?;
        let service_path =
            crate::field_resources::resolve_path(&files, resources.field_service(id as u8)?)?;
        let service = fs::read(files.join(service_path))?;
        let service_ranges = sections(&service)?;
        let doors = [20, 24]
            .into_iter()
            .map(|slot| -> Result<_> {
                let bytes = section(&service, &service_ranges, (slot - 4) / 4)?;
                Ok((slot as u16, decode_clip(bytes)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let extra: Vec<_> = doors
            .iter()
            .map(|(slot, bytes)| SourceClip {
                slot: *slot,
                resource: Some(resonance_content::field::DOOR_MOTION_RESOURCE_BASE + id),
                bytes,
            })
            .collect();
        assets.push(cook(id, name, &model, &animation, &extra, shared, output)?);
    }
    for (&id, data) in &sources.packages {
        ensure!(
            !assets.iter().any(|a| a.resource == id),
            "resource {id:#x} conflicts with an actor binding"
        );
        let index = id & 0xffff;
        let name = match id >> 16 {
            2 => format!("npc-{index}"),
            _ => format!("resource-{id}"),
        };
        assets.push(package(id, &name, data)?);
    }
    for (id, data) in field_models(map.section(7)?)? {
        assets.push(package(
            u32::from(id),
            &format!("field-{map_id}-npc-{id}"),
            data,
        )?);
    }
    // MAP-local model handles address slots starting at section 16. A
    // model's own wrapper holds its mesh layers and animation table.
    for index in 16..map.sections.len() {
        let Ok(data) = map.section(index) else {
            continue;
        };
        if data.get(..4) != Some(&31u32.to_be_bytes()) {
            continue;
        }
        assets.push(package(
            0xffee0000 + (index - 16) as u32,
            &format!("field-{map_id}-object-{}", index - 16),
            data,
        )?);
    }
    if declarations.save_point {
        let save_point = crate::field::MapArchive::open(&files.join("mahou.cab"))?;
        assets.push(package(
            resonance_content::field::SAVE_POINT_RESOURCE,
            "save-point",
            &save_point.bytes,
        )?);
    }
    crate::field_resources::validate_cooked(
        &model_resources,
        assets.iter().flat_map(|actor| {
            std::iter::once(actor.resource).chain(
                actor
                    .parts
                    .iter()
                    .flat_map(|part| &part.clips)
                    .filter_map(|clip| clip.animation_resource),
            )
        }),
    )?;
    Ok(assets)
}

fn decode_clip(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.get(..4) == Some(&0x007b7960u32.to_be_bytes()) {
        Ok(bytes.to_vec())
    } else {
        crate::compression::decode(bytes)
    }
}

fn unique_clips<'a>(
    actor: u32,
    clips: impl IntoIterator<Item = SourceClip<'a>>,
) -> Result<Vec<SourceClip<'a>>> {
    let mut unique = BTreeMap::<_, SourceClip<'a>>::new();
    for clip in clips {
        let key = (clip.resource.unwrap_or(actor), clip.slot);
        if let Some(previous) = unique.get(&key) {
            ensure!(
                previous.bytes == clip.bytes,
                "conflicting animation {key:?}"
            );
        } else {
            unique.insert(key, clip);
        }
    }
    Ok(unique.into_values().collect())
}

fn cook(
    id: u32,
    name: &str,
    model: &[u8],
    animation: &[u8],
    extra: &[SourceClip<'_>],
    shared: &[(u32, AuthoredAnimation)],
    output: &Path,
) -> Result<ActorAssets> {
    ensure!(
        shared.iter().all(|(resource, _)| *resource != id),
        "standalone animation conflicts with actor resource {id:#x}"
    );
    let mut decoded = Vec::new();
    for (index, range) in sections(animation)?.into_iter().enumerate().skip(2) {
        if let Some(range) = range {
            let bytes = &animation[range];
            let bytes = decode_clip(bytes)
                .with_context(|| format!("decode {name} slot {}", 4 + index * 4))?;
            decoded.push(((4 + index * 4) as u16, bytes));
        }
    }
    let mut clips: Vec<_> = decoded
        .iter()
        .map(|(slot, bytes)| SourceClip {
            slot: *slot,
            bytes,
            resource: None,
        })
        .collect();
    clips.extend(extra.iter().map(|c| SourceClip {
        slot: c.slot,
        bytes: c.bytes,
        resource: c.resource,
    }));
    let clips = unique_clips(id, clips)?;
    let parts = cook_parts(name, model, &clips, shared, output)?;
    println!(
        "Cooked {name}: {} clips across {} mesh layers",
        clips.len() + shared.len(),
        parts.len()
    );
    Ok(ActorAssets {
        resource: id,
        model_sha256: digest(model),
        animation_sha256: digest(animation),
        // Optional accessories whose names start with “kk” begin hidden.
        hidden_nodes: parts[0]
            .bone_names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.starts_with("kk"))
            .map(|(index, _)| index as u16)
            .collect(),
        parts,
    })
}

pub(crate) fn cook_parts(
    name: &str,
    model: &[u8],
    clips: &[SourceClip<'_>],
    shared: &[(u32, AuthoredAnimation)],
    output: &Path,
) -> Result<Vec<resonance_content::ScenePart>> {
    let ranges = sections(model)?;
    let primary = section(model, &ranges, 0)?;
    let mut parts = Vec::new();
    for index in 0..2 {
        if ranges.get(index).is_none_or(Option::is_none) {
            continue;
        }
        let source = section(model, &ranges, index)?;
        let normalized = texture_palette(primary, source)?;
        let (mut part, gltf, _) = cook_part(
            PartSource {
                name: &format!("characters/{name}/{index}"),
                source: &normalized,
                resource: index as u16,
                draw_order: index as u32,
                depth_write: true,
                translation: [0.; 3],
                autoplay: None,
                animation_slots: &[],
                clip_prefix: name,
                extra_clips: clips,
                shared_clips: shared,
                texture_animations: Vec::new(),
            },
            output,
        )
        .with_context(|| format!("cook {name} layer {index}"))?;
        part.secondary_motion =
            crate::secondary_motion::cook(&normalized, &gltf, &part.bone_names)?;
        // Render the secondary model as an inverted hull for the outline.
        if index == 1 {
            // Outline alpha is half the primary ambient alpha.
            part.outline_color = Some([0, 0, 0, 127]);
            for material in &mut part.materials {
                material.cull = resonance_content::CullFace::Front;
                material.blend = true;
            }
        }
        let textures =
            crate::tpl::parse_tpl(&normalized[word(&normalized, 0)?..word(&normalized, 4)?])?;
        let channel = |offset: usize| -> Result<Option<usize>> {
            let value = *normalized
                .get(offset)
                .context("missing character atlas channel")?;
            let texture = value.checked_sub(1).map(usize::from);
            ensure!(
                texture.is_none_or(|t| t < textures.len()),
                "character atlas channel exceeds textures"
            );
            Ok(texture)
        };
        let variant = channel(14)?.and_then(|texture| {
            let t = &textures[texture];
            let frames = match (t.width == 128, t.height) {
                (true, 256) | (false, 512) => 2,
                (true, 512) | (false, 1024) => 4,
                _ => return None,
            };
            Some(resonance_content::AtlasChannel { texture, frames })
        });
        part.appearance = Some(resonance_content::AppearanceTextures {
            eyes: if index == 0 { channel(12)? } else { None },
            mouth: if index == 0 { channel(13)? } else { None },
            variant,
            costume: if index == 0 { channel(15)? } else { None },
        });
        parts.push(part);
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_model_bank_uses_its_id_table_and_checks_aliases_and_ranges() {
        // ID order differs from ID value; offsets can alias the same package.
        let mut data: Vec<_> = [4u32, 20, 28, 36, 28, 0x014a0030, 0x00350000, 31, 7, 31, 9]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        let models = field_models(&data).unwrap();
        assert_eq!(
            models.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [330, 48, 53]
        );
        assert_eq!(models[0].1, models[2].1);
        assert_eq!(word(models[1].1, 4).unwrap(), 9);
        data[16..20].copy_from_slice(&0xfffcu32.to_be_bytes());
        assert!(field_models(&data).is_err());
        data[16..20].copy_from_slice(&28u32.to_be_bytes());
        data[24..26].copy_from_slice(&330u16.to_be_bytes());
        assert!(field_models(&data).is_err());
        assert!(field_models(&[0, 0, 0, 1, 0, 0, 0, 0]).unwrap().is_empty());

        // The file extent, rather than a roster-sized limit, bounds the table.
        let count = 513;
        let ids = 4 + count * 4;
        let model = ids + (count - 1) * 2;
        let mut data = vec![0; model + 4];
        data[..4].copy_from_slice(&(count as u32).to_be_bytes());
        for index in 0..count {
            let offset = if index == 0 { ids } else { model };
            data[4 + index * 4..8 + index * 4].copy_from_slice(&(offset as u32).to_be_bytes());
        }
        for index in 0..count - 1 {
            data[ids + index * 2..ids + index * 2 + 2]
                .copy_from_slice(&(index as u16).to_be_bytes());
        }
        assert_eq!(field_model_entries(&data).unwrap().len(), count - 1);
        data[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(field_model_count(&data).is_err());
    }
    #[test]
    fn archive_fallback_checks_counts_and_payload_ranges() {
        let data: Vec<u8> = [2u32, 20, 4, 0, 0, 0x12345678]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        assert_eq!(
            archive_entry(&data, 1).unwrap(),
            &0x12345678u32.to_be_bytes()
        );
        assert!(archive_entry(&data, 2).is_err());
        assert!(archive_entry(&data[..23], 0).is_err());
    }

    #[test]
    #[ignore = "requires both original extracted discs and cook-all records; no mesh or texture conversion"]
    fn original_shared_clips_bind_to_all_party_defaults_and_npc_models() -> Result<()> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
        let cooked = local.join("all-assets");
        for disc in ["disc1", "disc2"] {
            let root = local.join("extracted").join(disc);
            let files = root.join("files");
            let catalogue = crate::resource::read(&fs::read(root.join("sys/main.dol"))?)?;
            let declarations = [0x10004, 0x10012, 0x10802, 0x10a20, 0x2000d, 0x30030].into();
            let mut sources = Sources::read(&files, &catalogue, &declarations)?;
            assert_eq!(
                sources.packages.keys().copied().collect::<Vec<_>>(),
                [0x2000d]
            );
            assert_eq!(sources.animations.len(), 5);
            assert!(sources.textures.is_empty());
            assert_eq!(catalogue.party_bodies.len(), 9);
            let read = |name| {
                fs::read(files.join(crate::field_resources::resolve_path(&files, name)?))
                    .map_err(anyhow::Error::from)
            };
            let mut resources =
                crate::field_resources::binding::Resources::open(&cooked, &root, &catalogue)?;
            let motions = sources
                .scene_clips(&mut resources)?
                .into_iter()
                .map(|(id, authored)| {
                    let archive = read(catalogue.source(id)?)?;
                    let bytes = decode_clip(archive_entry(&archive, (id & 0xffff) as usize)?)?;
                    let json = crate::animation::unbound_indexed(&bytes)?;
                    assert_eq!(serde_json::to_value(&authored)?, json);
                    let duration = json["duration_frames"]
                        .as_f64()
                        .context("motion duration")? as f32;
                    Ok((id, duration, authored, bytes))
                })
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(
                motions.iter().map(|(id, ..)| *id).collect::<BTreeSet<_>>(),
                sources.animations
            );
            let mut packages = std::mem::take(&mut sources.packages);
            for id in 1..=catalogue.party_bodies.len() as u32 {
                packages.insert(
                    id,
                    read(catalogue.party(crate::resource::PartyResource::Body, id as u8, 0)?)?,
                );
                let field_motion = read(catalogue.field_motion(id as u8)?)?;
                section(&field_motion, &sections(&field_motion)?, 2)?;
                let service = read(catalogue.field_service(id as u8)?)?;
                let ranges = sections(&service)?;
                let doors = [20, 24]
                    .into_iter()
                    .map(|slot| {
                        Ok((
                            slot,
                            decode_clip(section(&service, &ranges, (slot - 4) / 4)?)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let service_id = resonance_content::field::DOOR_MOTION_RESOURCE_BASE + id;
                let extra = doors.iter().chain(&doors).map(|(slot, bytes)| SourceClip {
                    slot: *slot as u16,
                    resource: Some(service_id),
                    bytes,
                });
                let clips = unique_clips(id, extra)?;
                assert_eq!(clips.len(), 2);
                assert_eq!(
                    clips
                        .iter()
                        .filter(|c| c.resource == Some(service_id))
                        .map(|c| c.slot)
                        .collect::<Vec<_>>(),
                    [20, 24]
                );
            }
            assert_eq!(packages.len(), 10);
            for (id, package) in packages {
                let layers = sections(&package)?;
                for range in layers.into_iter().take(2).flatten() {
                    let (_, model) = crate::geometry::model_resource(&package[range])?;
                    let range = crate::geometry::skeleton_range(model)?;
                    let bindings = crate::animation::ModelBindings::read(&model[range])?;
                    let node_count = bindings.node_ids.len();
                    for (resource, duration, authored, bytes) in &motions {
                        let motion = authored.motion(&bindings).with_context(|| {
                            format!("{disc}: animation {resource:#x} on model {id:#x}")
                        })?;
                        assert_eq!(
                            serde_json::to_value(&motion)?,
                            serde_json::to_value(bindings.motion(bytes)?)?
                        );
                        assert_eq!(motion.duration_frames, *duration);
                        assert!(
                            motion
                                .tracks
                                .iter()
                                .all(|t| usize::from(t.bone) < node_count)
                        );
                        if id <= 2 && matches!(*resource, 0x10004 | 0x30030) {
                            // The same source clip keeps each target layer's bone indices.
                            let nodes: Vec<_> = motion
                                .tracks
                                .iter()
                                .filter(|t| t.times.len() > 2)
                                .map(|t| t.bone)
                                .collect();
                            let mut gltf = serde_json::json!({
                                "nodes":vec![serde_json::json!({"translation":[0.,0.,0.]});node_count],
                                "buffers":[{}], "bufferViews":[], "accessors":[],
                            });
                            let mut original = gltf.clone();
                            let mut original_bytes = Vec::new();
                            let expected_seconds = crate::animation::bake(
                                bytes,
                                &bindings,
                                &mut original,
                                &mut original_bytes,
                                "shared",
                            )?;
                            let mut binary = Vec::new();
                            let seconds = crate::animation::bake_motion(
                                &motion,
                                &mut gltf,
                                &mut binary,
                                "shared",
                            )?;
                            assert_eq!(seconds, expected_seconds);
                            assert_eq!(gltf, original);
                            assert_eq!(binary, original_bytes);
                            assert_eq!(
                                seconds,
                                *duration / resonance_content::battle::pose::FRAME_HZ
                            );
                            let extras = &gltf["animations"][0]["extras"];
                            assert_eq!(extras["secondary_pose_nodes"], serde_json::json!(nodes));
                            assert!(!motion.tracks.is_empty());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires original disc and cook-all records; prepares actors without launching the game"]
    fn original_field_actors_publish_shared_clips_for_every_model() -> Result<()> {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let cooked = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
        let output = crate::temporary_path(&std::env::temp_dir().join("field-actor-bindings"));
        let result = (|| -> Result<()> {
            for map_id in [330, 340] {
                let map = crate::field::MapArchive::open(&crate::field::source_for_id(
                    &extracted, map_id,
                )?)?;
                let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
                let declared = crate::field_resources::declarations(map.section(6)?)?;
                let sources =
                    Sources::read(&extracted.join("files"), &catalogue, &declared.resources)?;
                let mut resources = crate::field_resources::binding::Resources::open(
                    &cooked, &extracted, &catalogue,
                )?;
                let shared = sources.scene_clips(&mut resources)?;
                let shared_ids: BTreeSet<_> = shared.iter().map(|(id, _)| *id).collect();
                let actors = cook_field(&extracted, &output, map_id, &map, &sources, &shared)?;
                assert!((1..=9).all(|id| actors.iter().any(|actor| actor.resource == id)));
                assert!(actors.iter().any(|actor| actor.resource > 9));
                let mut durations = BTreeSet::new();
                for actor in actors {
                    for part in actor.parts {
                        let glb = crate::scene::glb::Glb::read(&output.join(&part.mesh))?;
                        let animations = glb.json["animations"]
                            .as_array()
                            .context("actor animations")?;
                        assert_eq!(animations.len(), part.clips.len());
                        for (clip, animation) in part.clips.iter().zip(animations) {
                            assert_eq!(
                                serde_json::to_value(&clip.secondary_pose_nodes)?,
                                animation["extras"]["secondary_pose_nodes"]
                            );
                            let input = animation["samplers"][0]["input"]
                                .as_u64()
                                .context("clip input")?
                                as usize;
                            assert_eq!(
                                glb.json["accessors"][input]["max"][0]
                                    .as_f64()
                                    .context("clip duration")?
                                    as f32,
                                clip.duration_seconds
                            );
                        }
                        let keys: BTreeSet<_> = part
                            .clips
                            .iter()
                            .map(|clip| {
                                (
                                    clip.animation_resource.unwrap_or(actor.resource),
                                    clip.resource_slot,
                                )
                            })
                            .collect();
                        assert_eq!(keys.len(), part.clips.len());
                        assert_eq!(
                            part.clips
                                .iter()
                                .filter_map(|clip| clip.animation_resource)
                                .filter(|id| shared_ids.contains(id))
                                .collect::<BTreeSet<_>>(),
                            shared_ids
                        );
                        if map_id == 330 {
                            assert!(
                                !part
                                    .clips
                                    .iter()
                                    .any(|clip| clip.animation_resource == Some(0x30030))
                            );
                            continue;
                        }
                        let clip = part
                            .clips
                            .iter()
                            .find(|clip| {
                                clip.animation_resource == Some(0x30030) && clip.resource_slot == 12
                            })
                            .context("shared motion missing from actor layer")?;
                        durations.insert(clip.duration_ticks());
                        assert!(
                            clip.secondary_pose_nodes
                                .iter()
                                .all(|&node| usize::from(node) < part.bone_names.len())
                        );
                    }
                }
                assert_eq!(durations.len(), usize::from(map_id == 340));
            }
            Ok(())
        })();
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        result
    }
}
