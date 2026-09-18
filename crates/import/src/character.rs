//! Character mesh layers and animation tables, resolved offline from their
//! original files. Runtime assets contain no relocations or resource pointers.
use crate::{
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

const PARTY_MODEL_FILES: u32 = 0x801face4;
const PARTY_ANIMATION_NAMES: u32 = 0x8017e4c0;

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
    let count = word(bytes, 0)?;
    ensure!((1..=512).contains(&count), "invalid field model count");
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
    let mut models = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for (i, &offset) in offsets.iter().enumerate().skip(1) {
        let id = crate::read::u16(names, (i - 1) * 2)?;
        ensure!(
            id > 9 && id != 24 && ids.insert(id),
            "duplicate or reserved field model ID {id}"
        );
        models.push((id, &bytes[offset..end(offset)]));
    }
    Ok(models)
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
        // Attachment placeholders can contain a skeleton and no geometry or palette.
        ensure!(
            word(primary, 0x2c)? == 0,
            "model geometry has no texture palette"
        );
        &EMPTY_PALETTE[..]
    } else {
        primary
            .get(word(primary, 0)?..word(primary, 4)?)
            .context("invalid primary texture palette")?
    };
    let end = word(secondary, 4)?;
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

/// Unlike the party archives, the shared event bank mixes models and clips.
/// Bind its clips to the model used by the event, never to a transient actor ID.
fn special_clip_model(resource: u32) -> Result<u32> {
    Ok(match resource {
        0x10004 | 0x1000b | 0x1000c | 0x10012 | 0x10013 | 0x10016 => 1,
        0x10107 => 2,
        0x10206 | 0x10207 => 3,
        0x10802 | 0x10804 => 9,
        0x10a20 | 0x10a21 => 0x2000d,
        0x10a30 | 0x10a31 => 0x2000e,
        _ => anyhow::bail!("event animation {resource:#x} needs a model binding"),
    })
}

pub(crate) fn cook_field(
    extracted: &Path,
    output: &Path,
    map_id: u32,
    map: &crate::field::MapArchive,
) -> Result<Vec<ActorAssets>> {
    let files = extracted.join("files");
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    let declarations = crate::field_resources::declarations(map.section(6)?)?;
    let groups: BTreeSet<_> = (1..=5)
        .chain(
            declarations
                .resources
                .iter()
                .map(|id| id >> 16)
                .filter(|g| *g > 0),
        )
        .collect();
    let archives: BTreeMap<_, _> = groups
        .into_iter()
        .map(|group| -> Result<_> {
            let path = crate::field_resources::source_path(&executable, &files, group << 16)?;
            Ok((group, fs::read(files.join(path))?))
        })
        .collect::<Result<_>>()?;
    let clip = |resource: u32| -> Result<SourceClip<'_>> {
        Ok(SourceClip {
            slot: 12,
            resource: Some(resource),
            bytes: archive_entry(&archives[&(resource >> 16)], (resource & 0xffff) as usize)?,
        })
    };
    let mut declared_clips = BTreeMap::<u32, Vec<u32>>::new();
    let mut declared_packages = Vec::new();
    for &resource in &declarations.resources {
        let model = match resource >> 16 {
            1 => {
                if word(clip(resource)?.bytes, 0)? == 31 {
                    declared_packages.push(resource);
                    continue;
                }
                special_clip_model(resource)?
            }
            // The nine party animation archives follow the shared and NPC banks.
            group @ 3..=11 => group - 2,
            _ => continue,
        };
        declared_clips.entry(model).or_default().push(resource);
    }
    let mut model_resources = declarations.resources.clone();
    let mut assets = Vec::new();
    let package = |id, name: &str, data: &[u8]| cook(id, name, data, data, &[], output);
    let party: BTreeSet<_> = (1..=4)
        .chain(declared_clips.keys().copied().filter(|id| *id <= 9))
        .collect();
    for id in party {
        let model_pointer = crate::read::u32(
            // Five costume pointers per party member; use the default costume.
            crate::dol::slice(&executable, PARTY_MODEL_FILES + (id - 1) * 5 * 4, 4)?,
            0,
        )?;
        let model_path = crate::dol::text(&executable, model_pointer)?;
        let name = model_path
            .strip_suffix("000.bin")
            .context("unexpected default party model path")?;
        let model = fs::read(files.join(&model_path))?;
        let animation_path = crate::dol::text(&executable, PARTY_ANIMATION_NAMES + (id - 1) * 12)?;
        let animation_path = if files.join(&animation_path).is_file() {
            animation_path
        } else {
            animation_path.to_ascii_lowercase()
        };
        let animation = fs::read(files.join(animation_path))?;
        let service = fs::read(files.join(format!("{name}_ex.bin")))?;
        let service_ranges = sections(&service)?;
        let doors = [20, 24]
            .into_iter()
            .map(|slot| -> Result<_> {
                let bytes = section(&service, &service_ranges, (slot - 4) / 4)?;
                Ok((slot as u16, decode_clip(bytes)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut extra: Vec<_> = match id {
            1 => &[18, 19, 22][..],
            3 => &[518, 519][..],
            _ => &[],
        }
        .iter()
        .copied()
        .map(|i| {
            Ok(SourceClip {
                slot: 12,
                resource: Some(0x10000 + i as u32),
                bytes: archive_entry(&archives[&1], i)?,
            })
        })
        .collect::<Result<_>>()?;
        extra.extend(doors.iter().map(|(slot, bytes)| SourceClip {
            slot: *slot,
            resource: Some(resonance_content::field::DOOR_MOTION_RESOURCE_BASE + id),
            bytes,
        }));
        let indices: &[u32] = match id {
            1 => &[48, 103, 117, 118],
            2 => &[47, 48, 117, 118],
            3 => &[80, 117, 118],
            _ => &[],
        };
        for &index in indices {
            extra.push(clip(((id + 2) << 16) + index)?);
        }
        if id == 2 {
            // Colette's research interaction uses Lloyd's compatible clip 48.
            extra.push(clip(0x30030)?);
        }
        for &resource in declared_clips.get(&id).into_iter().flatten() {
            if !extra.iter().any(|clip| clip.resource == Some(resource)) {
                extra.push(clip(resource)?);
            }
        }
        assets.push(cook(id, name, &model, &animation, &extra, output)?);
    }
    for index in declarations
        .resources
        .iter()
        .filter(|id| **id >> 16 == 2)
        .map(|id| (*id & 0xffff) as usize)
    {
        let id = 0x20000 + index as u32;
        let data = archive_entry(&archives[&2], index)?;
        let extra = declared_clips
            .get(&id)
            .into_iter()
            .flatten()
            .map(|id| clip(*id))
            .collect::<Result<Vec<_>>>()?;
        assets.push(cook(
            id,
            &format!("npc-{index}"),
            data,
            data,
            &extra,
            output,
        )?);
    }
    for &id in declarations.resources.range(..=u32::from(u16::MAX)) {
        let path = crate::field_resources::source_path(&executable, &files, id)?;
        let data = fs::read(files.join(&path))?;
        if word(&data, 0)? == 0x0020af30 {
            crate::tpl::parse_tpl(&data)?;
            model_resources.remove(&id);
            eprintln!(
                "Field {map_id}: texture resource {id:#x} ({path}) requires an overlay recipe; not a character package"
            );
            continue;
        }
        ensure!(
            word(&data, 0)? == 31,
            "resource {id:#x} ({path}) needs a cooking recipe"
        );
        ensure!(
            !assets.iter().any(|a| a.resource == id),
            "resource {id:#x} conflicts with an actor binding"
        );
        assets.push(package(id, &format!("resource-{id}"), &data)?);
    }
    for resource in declared_packages {
        let index = (resource & 0xffff) as usize;
        let data = archive_entry(&archives[&1], index)?;
        let name = if (2660..=2661).contains(&index) {
            format!("classroom-prop-{index}")
        } else {
            format!("resource-{resource}")
        };
        assets.push(package(resource, &name, data)?);
    }
    if map_id != 340 {
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

fn cook(
    id: u32,
    name: &str,
    model: &[u8],
    animation: &[u8],
    extra: &[SourceClip<'_>],
    output: &Path,
) -> Result<ActorAssets> {
    let ranges = sections(model)?;
    let primary = section(model, &ranges, 0)?;
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
                extra_clips: &clips,
                texture_animations: Vec::new(),
            },
            output,
        )
        .with_context(|| format!("cook {name} layer {index}"))?;
        part.secondary_motion = crate::secondary_motion::cook(&gltf, &part.bone_names, id == 1)?;
        if id == 2 {
            crate::secondary_motion::colette(&mut part.secondary_motion, &part.bone_names)?;
        } else if id == 4 {
            crate::secondary_motion::raine(&mut part.secondary_motion, &part.bone_names)?;
        }
        // Render the secondary model as an inverted hull for the outline.
        if index == 1 {
            // Outline alpha is half the primary ambient alpha.
            part.outline_color = Some([0, 0, 0, 127]);
            for material in &mut part.materials {
                material.cull = resonance_content::CullFace::Front;
                material.blend = true;
            }
        }
        let textures = crate::tpl::parse_tpl(&normalized[word(&normalized, 0)?..])?;
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
    println!(
        "Cooked {name}: {} clips across {} mesh layers",
        clips.len(),
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
}
