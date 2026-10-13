//! Decode effect timelines, particle inputs, and combat colors.
pub(crate) mod art;
mod declaration;
mod palettes;
pub(crate) use palettes::SharedPalettes;
mod source;
mod timeline;
use anyhow::{Context, Result, ensure};
pub use source::{publish, read};
pub(crate) use source::{read_enemy_with_palettes, read_with_palettes};
use timeline::Record;

/// Recompile metadata while retaining the unchanged package's prepared artwork.
pub(crate) fn refresh_cached(
    bytes: &[u8],
    bank: &mut resonance_content::battle_effect::SourceBank,
    mut shared: Option<&mut SharedPalettes>,
) -> Result<()> {
    use resonance_content::battle_effect::{declaration::Declaration, visual::ParticleTexture};
    let (mut fresh, strides) = read_with_palettes(bytes)?;
    ensure!(
        fresh.source_sha256 == bank.source_sha256 && fresh.actors.len() == bank.actors.len(),
        "cached effect declarations differ from source"
    );
    for (index, (declaration, prepared)) in fresh.actors.iter_mut().zip(&bank.actors).enumerate() {
        if let Declaration::Particle { visual, .. } = declaration
            && let ParticleTexture::Atlas {
                slot,
                dual,
                palette_set,
            } = &mut visual.texture
        {
            if *slot != 6
                && let Some(shared) = shared.as_deref_mut()
            {
                *palette_set = shared.set(*slot, strides[index])?;
                continue;
            }
            let texture = if matches!(prepared, Declaration::Unsupported { .. }) {
                // A newly supported particle can share an already prepared palette
                // table only when its authored slot and selector stride agree.
                let mut binding = None;
                for (other, &stride) in bank.actors.iter().zip(&strides) {
                    if stride == strides[index]
                        && let Declaration::Particle { visual, .. } = other
                        && visual.texture.slot() == Some(*slot)
                        && visual.texture.dual() == *dual
                    {
                        let texture = visual.texture;
                        ensure!(
                            binding.is_none_or(|previous| previous == texture),
                            "conflicting cached effect palette bindings"
                        );
                        binding = Some(texture);
                    }
                }
                let texture = binding.with_context(|| {
                    format!("effect particle {index} has no prepared palette binding")
                })?;
                let textures = bank
                    .art
                    .as_ref()
                    .and_then(|art| art.textures.get(slot))
                    .context("cached effect texture slot is not prepared")?;
                ensure!(
                    !textures.is_empty()
                        && textures.iter().all(|prepared| {
                            prepared
                                .palette_sets
                                .get(usize::from(texture.palette_set()))
                                .is_some()
                        }),
                    "cached effect palette set is not prepared"
                );
                texture
            } else {
                prepared.particle_visual()?.texture
            };
            ensure!(
                texture.slot() == Some(*slot) && texture.dual() == *dual,
                "cached effect texture binding differs from source"
            );
            *palette_set = texture.palette_set();
        }
    }
    fresh.art = bank.art.clone();
    *bank = fresh;
    Ok(())
}

/// Refresh the resident banks against an existing library, without publishing art.
pub fn publish_cached(
    usual: &[u8],
    library: &std::path::Path,
    output: &std::path::Path,
) -> Result<Vec<String>> {
    use resonance_content::battle_effect::{COMMON_PATH, TECHNIQUES_PATH};
    [(2, COMMON_PATH), (3, TECHNIQUES_PATH)]
        .into_iter()
        .map(|(member, path)| {
            let mut bank = serde_json::from_slice(&std::fs::read(library.join(path))?)?;
            refresh_cached(
                crate::source_assets::section(usual, member)?,
                &mut bank,
                None,
            )?;
            crate::write_atomic(&output.join(path), &serde_json::to_vec(&bank)?)?;
            Ok(path.to_owned())
        })
        .collect()
}

pub fn publish_tints(
    file: &std::path::Path,
    output: &std::path::Path,
    prefix: &str,
) -> Result<String> {
    use resonance_content::battle_effect::{ActorColors, AdmissionColors, Tints};
    let module = crate::rel::Rel::read(file)?;
    let actors: [[u8; 4]; 12] = crate::read::Field::read(module.at((4, 0x1564))?, 0)?;
    let admission: [[u8; 4]; 4] = crate::read::Field::read(module.at((4, 0x11b0))?, 0)?;
    let tints = Tints {
        palettes: module
            .at((4, 0x2174))?
            .get(..10)
            .context("truncated effect palette table")?
            .try_into()?,
        colors: crate::read::Field::read(module.at((4, 0x2180))?, 0)?,
        actors: ActorColors {
            recovery: actors[0],
            buff: actors[2],
            scan: actors[7],
            debuff: actors[8],
        },
        contact_effects: crate::read::Field::read(module.at((5, 0x13f8))?, 0)?,
        contact_colors: crate::read::Field::read(module.at((5, 0x13d0))?, 0)?,
        admission_colors: AdmissionColors {
            basic: admission[0],
            advanced: admission[1],
            arcane: admission[2],
        },
    };
    let path = format!("{prefix}/effects/tints.json");
    crate::write_atomic(&output.join(&path), &serde_json::to_vec(&tints)?)?;
    Ok(path)
}

pub fn modifier(bytes: &[u8], at: usize) -> Result<Vec<u16>> {
    let mut bytes = bytes.get(at..).context("effect modifier outside bank")?;
    let mut words = Vec::new();
    loop {
        let opcode = u16::from_be_bytes(
            bytes
                .get(..2)
                .context("truncated effect modifier")?
                .try_into()
                .unwrap(),
        );
        let count = match opcode {
            0xffff => 1,
            0 | 2..=6 | 10..=12 | 16 | 17 | 19 | 21 | 23..=25 => 4,
            1 | 20 | 22 => 8,
            7 | 8 | 13..=15 | 18 | 26..=30 => 6,
            9 => 12,
            _ => anyhow::bail!("unknown effect modifier opcode {opcode}"),
        };
        let row = bytes
            .get(..count * 2)
            .context("truncated effect modifier operands")?;
        words.extend(
            row.chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]])),
        );
        if opcode == 0xffff {
            return Ok(words);
        }
        bytes = &bytes[count * 2..];
    }
}

fn timelines(bytes: &[u8]) -> Result<Vec<Result<Vec<Record>>>> {
    ensure!(
        bytes.len() >= 20 && &bytes[..4] == b"ef1\0",
        "invalid battle effect bank"
    );
    let half = |offset| u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    let offsets: Vec<_> = (8..20).step_by(2).map(half).collect();
    ensure!(
        offsets
            .iter()
            .all(|&offset| (20..=bytes.len()).contains(&offset)),
        "battle effect section outside bank"
    );
    let events = offsets[1];
    let end = offsets
        .iter()
        .copied()
        .filter(|&offset| offset > events)
        .min()
        .unwrap_or(bytes.len());
    let table = offsets[4];
    let count = usize::from(bytes[4]);
    let table_end = offsets
        .iter()
        .copied()
        .filter(|&offset| offset > table)
        .min()
        .unwrap_or(bytes.len());
    let roots = bytes[table..table_end]
        .get(..count * 2)
        .context("truncated effect program table")?;
    let starts = roots
        .chunks_exact(2)
        .map(|root| {
            let start = events
                .checked_add_signed(i16::from_be_bytes([root[0], root[1]]) as isize)
                .context("effect program offset underflow")?;
            ensure!(
                (events..end).contains(&start),
                "effect program outside command section"
            );
            Ok(start)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(starts
        .into_iter()
        .map(|start| {
            let mut records = Vec::new();
            let mut payload = false;
            for bytes in bytes[start..end].chunks_exact(6) {
                let record = Record::from_bytes(bytes.try_into().unwrap());
                records.push(record);
                if payload {
                    ensure!(
                        record.command < 254,
                        "effect repeat requires an emission, sound or modification"
                    );
                    payload = false;
                } else if record.command == 254 {
                    return Ok(records);
                } else {
                    payload = record.command == 255;
                }
            }
            anyhow::bail!("unterminated effect program")
        })
        .collect())
}
