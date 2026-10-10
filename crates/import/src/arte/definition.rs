//! Decode technique parameters and casting behavior at the input boundary.
use super::{Definition, MenuDefinition};
use crate::{dol, read::u32 as word};
use anyhow::{Context, Result};
use resonance_content::arte::{
    AdmissionFlash, ArteFamily, LearningPrerequisite, LearningRoute, LearningRules,
    RegalArteFamily, TechniqueCapabilities, TechniqueTarget,
};

const ADDRESS: u32 = 0x80202f90;
const COUNT: usize = 253;
const BYTES: usize = 88;

pub(crate) struct Row {
    pub gameplay: Definition,
    pub menu: MenuDefinition,
}

fn decode(executable: &[u8], row: &[u8], table: &[u8]) -> Result<Row> {
    let row = row.get(..BYTES).context("truncated arte definition")?;
    let signed = |at| i16::from_be_bytes([row[at], row[at + 1]]);
    let text = |at| dol::optional_text(executable, word(row, at)?);
    let flags = word(row, 0x34)?;
    let range = word(row, 0x38)? as i32;
    let action_range = if flags & 2 != 0 && range >= 1000 {
        8000.
    } else {
        range as f32
    };
    let optional_id = |id: i16| -> Result<Option<u16>> {
        Ok(match id {
            0 => None,
            _ => Some(id.try_into().context("negative learning reference")?),
        })
    };
    let required: [i16; 4] = std::array::from_fn(|i| signed(0x26 + i * 2));
    let successor_alternatives = required[0] == -1;
    let mut prerequisites = Vec::new();
    for &id in required.iter().skip(usize::from(successor_alternatives)) {
        if id == 0 {
            continue;
        }
        let id: u16 = id.try_into().context("negative learning prerequisite")?;
        let any_of = if successor_alternatives {
            let start = usize::from(id) * BYTES;
            let parent = table
                .get(start..start + BYTES)
                .context("learning prerequisite outside table")?;
            [0x1a, 0x1c]
                .into_iter()
                .filter_map(|at| {
                    let successor = i16::from_be_bytes([parent[at], parent[at + 1]]);
                    optional_id(successor).transpose()
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![id]
        };
        prerequisites.push(LearningPrerequisite {
            any_of,
            minimum_uses: if successor_alternatives {
                signed(0x3c) as u16
            } else {
                0
            },
        });
    }
    let gameplay = Definition {
        tp_cost: row[8],
        element: row[0x15],
        learning: LearningRules {
            route: match row[0x17] {
                1 => Some(LearningRoute::Technical),
                2 => Some(LearningRoute::Strike),
                _ => None,
            },
            parent: optional_id(signed(0x18))?,
            technical_successor: optional_id(signed(0x1a))?,
            strike_successor: optional_id(signed(0x1c))?,
            parent_uses: signed(0x3c) as u16,
            prerequisites,
            excludes: [signed(0x2e), signed(0x30)]
                .into_iter()
                .filter_map(|id| optional_id(id).transpose())
                .collect::<Result<_>>()?,
            requires_story_unlock: false,
        },
        capabilities: capabilities(flags),
        admission_flash: if flags & 0x80 != 0 {
            None
        } else if flags & 0x08000010 != 0 {
            Some(AdmissionFlash::Arcane)
        } else if flags & 0x04000008 != 0 {
            Some(AdmissionFlash::Advanced)
        } else if flags & 0x02000004 != 0 {
            Some(AdmissionFlash::Basic)
        } else {
            None
        },
        learn_on_level_up: flags & 0x80000000 != 0,
        casting: resonance_content::arte::Casting {
            support_target: row[0x16] != 0,
            effects: if flags & 0x00400000 != 0 {
                resonance_content::arte::CastingEffects::Offensive
            } else if flags & 0x00800000 != 0 {
                resonance_content::arte::CastingEffects::Healing
            } else {
                resonance_content::arte::CastingEffects::Standard
            },
        },
        action_range,
        cast_time_adjustment: signed(0x38),
        recovery_ticks: signed(0x3a),
        required_level: signed(0x3e) as u16,
    };
    Ok(Row {
        gameplay,
        menu: MenuDefinition {
            tp_percent: signed(0) == 34,
            text: resonance_content::menu_data::NamedText {
                name: text(16)?.unwrap_or_default(),
                description: text(12)?.unwrap_or_default(),
            },
            rank: row[0x14],
            route: row[0x17],
            alternatives: std::array::from_fn(|i| signed(0x1e + i * 2) as u16),
        },
    })
}

fn capabilities(flags: u32) -> TechniqueCapabilities {
    TechniqueCapabilities {
        family: if flags & 0x20 != 0 {
            Some(ArteFamily::Finisher)
        } else if flags & 0x10 != 0 {
            Some(ArteFamily::Arcane)
        } else if flags & 8 != 0 {
            Some(ArteFamily::Advanced)
        } else if flags & 4 != 0 {
            Some(ArteFamily::Basic)
        } else {
            None
        },
        regal_family: if flags & 0x08000000 != 0 {
            Some(RegalArteFamily::Aerial)
        } else if flags & 0x04000000 != 0 {
            Some(RegalArteFamily::AntiAir)
        } else if flags & 0x02000000 != 0 {
            Some(RegalArteFamily::Ground)
        } else {
            None
        },
        spell: flags & 0x80 != 0,
        aerial: flags & 0x40 != 0,
        uses_weapon_reach: flags & 2 == 0,
        chains_without_contact: flags & 0x1000 != 0,
        target: match flags & 0x001c0000 {
            0x00040000 => TechniqueTarget::Enemy,
            0x00080000 => TechniqueTarget::Ally,
            0x00100000 => TechniqueTarget::SelfTarget,
            _ => TechniqueTarget::Unavailable,
        },
        offensive: flags & 0x100 != 0,
        revives: flags & 0x10000 != 0,
        healing: flags & 0x8000 != 0,
    }
}

pub(crate) fn definitions(executable: &[u8]) -> Result<Vec<Row>> {
    let table = dol::slice(executable, ADDRESS, COUNT * BYTES)?;
    table
        .chunks_exact(BYTES)
        .enumerate()
        .map(|(index, row)| decode(executable, row, table).with_context(|| format!("arte {index}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_technique_parameters_and_rejects_bad_input() -> Result<()> {
        let mut row = [0; BYTES];
        row[8] = 4;
        row[0x38..0x3c].copy_from_slice(&800_u32.to_be_bytes());
        let value = decode(&[], &row, &[])?;
        assert_eq!(
            (value.gameplay.tp_cost, value.gameplay.action_range),
            (4, 800.)
        );
        assert!(value.menu.text.name.is_empty());
        assert!(decode(&[], &row[..BYTES - 1], &[]).is_err());
        row[16..20].copy_from_slice(&1_u32.to_be_bytes());
        assert!(decode(&[], &row, &[]).is_err());
        row[16..20].fill(0);
        row[0x26..0x28].copy_from_slice(&(-1_i16).to_be_bytes());
        row[0x28..0x2a].copy_from_slice(&1_i16.to_be_bytes());
        row[0x3c..0x3e].copy_from_slice(&50_i16.to_be_bytes());
        let mut table = vec![0; BYTES * 2];
        table[BYTES + 0x1a..BYTES + 0x1e].copy_from_slice(&[0, 2, 0, 3]);
        let value = decode(&[], &row, &table)?;
        let group = &value.gameplay.learning.prerequisites[0];
        assert_eq!((&group.any_of, group.minimum_uses), (&vec![2, 3], 50));
        assert!(decode(&[], &row, &[]).is_err());
        Ok(())
    }

    #[test]
    fn technique_policy_is_decoded_before_runtime_admission() -> Result<()> {
        let mut row = [0; BYTES];
        row[0x34..0x38].copy_from_slice(&0x80444186_u32.to_be_bytes());
        row[0x16] = 1;
        let value = decode(&[], &row, &[])?;
        assert_eq!(value.gameplay.capabilities.family, Some(ArteFamily::Basic));
        assert!(value.gameplay.capabilities.spell && value.gameplay.capabilities.offensive);
        assert_eq!(value.gameplay.capabilities.target, TechniqueTarget::Enemy);
        assert!(!value.gameplay.capabilities.uses_weapon_reach);
        assert!(value.gameplay.learn_on_level_up && value.gameplay.casting.support_target);
        assert_eq!(value.gameplay.admission_flash, None);
        row[0x34..0x38].copy_from_slice(&0x0800000c_u32.to_be_bytes());
        let value = decode(&[], &row, &[])?;
        assert_eq!(
            value.gameplay.capabilities.regal_family,
            Some(RegalArteFamily::Aerial)
        );
        assert_eq!(
            value.gameplay.capabilities.target,
            TechniqueTarget::Unavailable
        );
        assert_eq!(value.gameplay.admission_flash, Some(AdmissionFlash::Arcane));
        for (raw, target) in [
            (0x40000_u32, TechniqueTarget::Enemy),
            (0x80000, TechniqueTarget::Ally),
            (0x100000, TechniqueTarget::SelfTarget),
            (0xc0000, TechniqueTarget::Unavailable),
        ] {
            row[0x34..0x38].copy_from_slice(&raw.to_be_bytes());
            assert_eq!(decode(&[], &row, &[])?.gameplay.capabilities.target, target);
        }
        for (flags, effects) in [
            (0_u32, [5, 7]),
            (0x400000, [3, 7]),
            (0x800000, [4, 8]),
            (0xc00000, [3, 7]),
        ] {
            let mut row = [0; BYTES];
            row[0x34..0x38].copy_from_slice(&(flags | 0x101).to_be_bytes());
            let casting = decode(&[], &row, &[])?.gameplay.casting;
            assert_eq!(casting.effects.members(), effects);
        }
        Ok(())
    }
}
