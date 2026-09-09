//! Character mesh layers and animation tables, resolved offline from their
//! original files. Runtime assets contain no relocations or resource pointers.
use crate::{
    digest,
    field::sections,
    scene::{PartSource, SourceClip, cook_part},
};
use anyhow::{Context, Result, ensure};
use resonance_content::field::ActorAssets;
use std::{fs, ops::Range, path::Path};

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

/// A secondary mesh layer can share the primary layer's texture palette.
/// Assemble an offline source view for the existing geometry decoder; all GPL
/// and model-relative offsets remain unchanged.
fn texture_palette(primary: &[u8], secondary: &[u8]) -> Result<Vec<u8>> {
    if word(secondary, 0)? != 0 {
        return Ok(secondary.to_vec());
    }
    let texture = primary
        .get(word(primary, 0)?..word(primary, 4)?)
        .context("invalid primary texture palette")?;
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

pub(crate) fn cook_classroom(
    extracted: &Path,
    output: &Path,
    ktx: &Path,
) -> Result<Vec<ActorAssets>> {
    let files = extracted.join("files");
    let npc = fs::read(files.join("npc_all.bin"))?;
    let special = fs::read(files.join("d.d"))?;
    let colette_clips = fs::read(files.join("col_all.bin"))?;
    let lloyd_clips = fs::read(files.join("llo_all.bin"))?;
    let genis_clips = fs::read(files.join("gen_all.bin"))?;
    let mut assets = Vec::new();
    for (id, name) in [(1, "lloyd"), (2, "collet"), (3, "genius"), (4, "refill")] {
        let model = fs::read(files.join(format!("{name}000.bin")))?;
        let animation = fs::read(files.join(format!("{name}.bin")))?;
        let mut extra: Vec<_> = match id {
            1 => &[18, 19][..],
            3 => &[518, 519][..],
            _ => &[],
        }
        .iter()
        .copied()
        .map(|i| {
            Ok(SourceClip {
                slot: 12,
                resource: Some(0x10000 + i as u32),
                bytes: archive_entry(&special, i)?,
            })
        })
        .collect::<Result<_>>()?;
        let (archive, family, indices): (&[u8], u32, &[usize]) = match id {
            1 => (&lloyd_clips, 3, &[48, 103, 117, 118]),
            2 => (&colette_clips, 4, &[47, 48, 117, 118]),
            3 => (&genis_clips, 5, &[80, 117, 118]),
            _ => (&[], 0, &[]),
        };
        for &index in indices {
            extra.push(SourceClip {
                slot: 12,
                resource: Some((family << 16) + index as u32),
                bytes: archive_entry(archive, index)?,
            });
        }
        if id == 2 {
            // The research branch deliberately plays llo_all entry 48 on
            // Colette. Animation archive families do not constrain which
            // compatible skeleton may consume a clip; bind it to her model.
            extra.push(SourceClip {
                slot: 12,
                resource: Some(0x30030),
                bytes: archive_entry(&lloyd_clips, 48)?,
            });
        }
        assets.push(cook(id, name, &model, &animation, &extra, output, ktx)?);
    }
    for index in [65, 101, 102, 103, 104] {
        let data = archive_entry(&npc, index)?;
        assets.push(cook(
            0x20000 + index as u32,
            &format!("npc-{index}"),
            data,
            data,
            &[],
            output,
            ktx,
        )?);
    }
    for index in 2660..=2661 {
        let data = archive_entry(&special, index)?;
        assets.push(cook(
            0x10000 + index as u32,
            &format!("classroom-prop-{index}"),
            data,
            data,
            &[],
            output,
            ktx,
        )?);
    }
    Ok(assets)
}

fn cook(
    id: u32,
    name: &str,
    model: &[u8],
    animation: &[u8],
    extra: &[SourceClip<'_>],
    output: &Path,
    ktx: &Path,
) -> Result<ActorAssets> {
    let ranges = sections(model)?;
    let primary = section(model, &ranges, 0)?;
    let mut decoded = Vec::new();
    for (index, range) in sections(animation)?.into_iter().enumerate().skip(2) {
        if let Some(range) = range {
            let bytes = &animation[range];
            let bytes = if bytes.get(..4) == Some(&0x007b7960u32.to_be_bytes()) {
                bytes.to_vec()
            } else {
                crate::compression::decode(bytes)
                    .with_context(|| format!("decode {name} slot {}", 4 + index * 4))?
            };
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
            ktx,
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
