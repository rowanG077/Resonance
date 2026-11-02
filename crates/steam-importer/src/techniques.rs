//! Native technique rows and learning lists directly into named rules.
use crate::{pe, source::Source, text::Bank};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    arte::{
        self, AdmissionFlash, AiTargetScope, ArteFamily, Casting, CastingEffects, Definition,
        LearningPrerequisite, LearningRoute, LearningRules, RegalArteFamily, TechniqueCapabilities,
        TechniqueTarget,
        document::{Document, Entry, Menu},
        elemental,
    },
    library::Mappings,
    menu_data::{Element, NamedText, TechniqueUse},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use symphonia_constants::{KnownId, PartyCharacterId};

const TECHNIQUES: u32 = 0x8d84b0;
const NATIVE_COUNT: usize = 255;
const STRIDE: usize = 96;
const LEARNING: u32 = 0x8d82c0;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let bytes = source.require("TOS_TEXT.TXB")?;
    let bank = Bank::parse(&bytes)?;
    let document = convert(&image, &bank, source.language.slot())?;
    crate::output::json(&library.join(arte::PATH), &document)
}

// Keep the common elemental-variant IDs; Steam's extra native rows get ordinary
// logical IDs after them. All source references are resolved at this boundary.
pub(crate) fn source_name(names: &Mappings, id: u32) -> Result<&str> {
    names.target(logical_id(id))
}

pub(crate) fn logical_id(native: u32) -> u32 {
    match native {
        253 => 260,
        254 => 261,
        id => id,
    }
}

fn signed(row: &[u8], at: usize) -> Result<i16> {
    Ok(pe::half(row, at)? as i16)
}

fn caption(image: &pe::Image<'_>, bank: &Bank<'_>, slot: u32, index: u32) -> Result<String> {
    let directory = pe::word(image.slice(0x8b88b4 + slot * 4, 4)?, 0)?;
    let indices = image.slice(directory, NATIVE_COUNT * 3 * 4)?;
    bank.menu(
        pe::word(indices, index as usize * 4)?,
        encoding_rs::WINDOWS_1252,
    )
}

fn named_id(id: i16, names: &Mappings) -> Result<Option<String>> {
    if id == 0 {
        return Ok(None);
    }
    let id = u16::try_from(id).context("negative technique reference")?;
    Ok(Some(source_name(names, u32::from(id))?.to_owned()))
}

fn capabilities(flags: u32, ai_target_scope: AiTargetScope) -> TechniqueCapabilities {
    TechniqueCapabilities {
        ai_target_scope,
        summon: flags & 0x20000000 != 0,
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
        revives: flags & 0x2000 != 0,
        healing: flags & 0x200 != 0,
    }
}

fn field_use(id: usize) -> Option<TechniqueUse> {
    let (hp, party) = match id {
        98 | 221 => (30, false),
        99 | 117 => (45, true),
        100 => (60, false),
        118 => (30, true),
        119 => (100, false),
        120 => (60, true),
        122 => (70, true),
        192 => (25, false),
        193 => (35, false),
        194 => (45, false),
        _ => (0, false),
    };
    if hp != 0 {
        return Some(TechniqueUse::Recover { hp, party });
    }
    match id {
        101 | 102 => Some(TechniqueUse::Cure { party: id == 102 }),
        121 => Some(TechniqueUse::Revive),
        _ => None,
    }
}

fn decode(
    image: &pe::Image<'_>,
    bank: &Bank<'_>,
    slot: u32,
    row: &[u8],
    table: &[u8],
    id: usize,
    names: &Mappings,
    unlocks: &BTreeSet<u16>,
) -> Result<Entry> {
    let flags = pe::word(row, 0x38)?;
    let route = match row[0x17] {
        1 => Some(LearningRoute::Technical),
        2 => Some(LearningRoute::Strike),
        _ => None,
    };
    let successor_alternatives = signed(row, 0x2a)? == -1;
    let mut prerequisites = Vec::new();
    for at in (0x2a + usize::from(successor_alternatives) * 2..0x32).step_by(2) {
        let reference = signed(row, at)?;
        if reference == 0 {
            continue;
        }
        let reference = u16::try_from(reference).context("negative learning prerequisite")?;
        let any_of = if successor_alternatives {
            let parent = table
                .get(usize::from(reference) * STRIDE..usize::from(reference) * STRIDE + STRIDE)
                .context("learning prerequisite outside native technique table")?;
            [0x1a, 0x1c]
                .into_iter()
                .map(|at| named_id(signed(parent, at)?, names))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect()
        } else {
            vec![source_name(names, u32::from(reference))?.to_owned()]
        };
        let minimum_uses = if successor_alternatives {
            signed(row, 0x40)? as u16
        } else if (id == 122 && reference == 120) || (id == 123 && reference == 119) {
            50
        } else {
            0
        };
        prerequisites.push(LearningPrerequisite {
            any_of,
            minimum_uses,
        });
    }
    let learning = LearningRules {
        route,
        parent: named_id(signed(row, 0x18)?, names)?,
        technical_successor: named_id(signed(row, 0x1a)?, names)?,
        strike_successor: named_id(signed(row, 0x1c)?, names)?,
        parent_uses: signed(row, 0x40)? as u16,
        prerequisites,
        excludes: [0x32, 0x34]
            .into_iter()
            .map(|at| named_id(signed(row, at)?, names))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect(),
        requires_story_unlock: unlocks.contains(&(id as u16)),
    };
    let element = match row[0x15] {
        0 => None,
        value => Some(
            *Element::ALL
                .get(usize::from(value - 1))
                .context("invalid technique element")?,
        ),
    };
    let range = pe::word(row, 0x3c)? as i32;
    let action_range = if flags & 2 != 0 && range >= 1000 {
        8000.
    } else {
        range as f32
    };
    let rules = Definition {
        action: None,
        tp_cost: row[8],
        element,
        learning,
        capabilities: capabilities(
            flags,
            match row[0x16] {
                0 | 1 => AiTargetScope::Single,
                2 => AiTargetScope::Either,
                3 => AiTargetScope::Group,
                _ => anyhow::bail!("unsupported technique AI target scope"),
            },
        ),
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
        casting: Casting {
            fixed_timing: flags & 0x60000020 != 0,
            support_target: row[0x16] != 0,
            effects: if flags & 0x01000000 != 0 {
                CastingEffects::Angel
            } else if flags & 0x00400000 != 0 {
                CastingEffects::Offensive
            } else if flags & 0x00800000 != 0 {
                CastingEffects::Healing
            } else {
                CastingEffects::Standard
            },
        },
        action_range,
        cast_time_adjustment: signed(row, 0x3c)?,
        recovery_ticks: signed(row, 0x3e)?,
        required_level: signed(row, 0x42)? as u16,
    };
    let alternatives = [0x1e, 0x20, 0x22, 0x24, 0x26, 0x28]
        .into_iter()
        .map(|at| pe::half(row, at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        alternatives[0] != 0 || alternatives.iter().all(|&value| value == 0),
        "inactive forgetting alternatives for technique {id}"
    );
    let menu = Menu {
        tp_percent: signed(row, 0)? == 34,
        rank: row[0x14],
        event_only: row[0x17] == 3,
        forget_with: alternatives
            .into_iter()
            .filter(|&value| value != 0)
            .map(|value| Ok(source_name(names, u32::from(value))?.to_owned()))
            .collect::<Result<Vec<_>>>()?,
        field_use: field_use(id),
    };
    Ok(Entry {
        rules,
        menu,
        text: serde_json::to_value(NamedText {
            name: caption(image, bank, slot, pe::word(row, 16)?)?,
            description: caption(image, bank, slot, pe::word(row, 12)?)?,
        })?,
    })
}

fn convert(image: &pe::Image<'_>, bank: &Bank<'_>, slot: u32) -> Result<Document> {
    let mappings: Mappings =
        serde_json::from_str(include_str!("../../importer/src/technique-names.json"))?;
    let mut names = mappings
        .iter()
        .map(|(id, name)| (id, name.to_owned()))
        .collect::<BTreeMap<_, _>>();
    names.insert(30, "sword-rain-tempest".into());
    names.insert(144, "divine-judgment".into());
    names.insert(260, "infernal-ruin".into());
    names.insert(261, "crimson-devastation".into());
    let mappings = Mappings::new(names)?;
    let characters = Mappings::new(
        PartyCharacterId::ALL
            .iter()
            .filter_map(|&id| id.resource_name().map(|name| (id.value(), name.to_owned())))
            .collect(),
    )?;
    ensure!(
        mappings.iter().count() == NATIVE_COUNT - 1 + elemental::VARIANTS.len()
            && characters.iter().count() == 9,
        "technique names do not cover source rows"
    );
    let table = image.slice(TECHNIQUES, NATIVE_COUNT * STRIDE)?;
    let lists = image.slice(LEARNING, 11 * 41)?;
    let colette = &lists[41..82];
    let mut unlocks = BTreeSet::from([45]);
    for &id in colette
        .get(1..1 + usize::from(colette[0]))
        .context("Colette learning list exceeds native capacity")?
    {
        let row = table
            .get(usize::from(id) * STRIDE..usize::from(id) * STRIDE + STRIDE)
            .context("Colette technique outside source table")?;
        for at in [0x1a, 0x1c] {
            let successor = signed(row, at)?;
            if successor > 0 {
                unlocks.insert(successor as u16);
            }
        }
    }
    let mut techniques = BTreeMap::new();
    for (id, row) in table.chunks_exact(STRIDE).enumerate().skip(1) {
        let name = source_name(&mappings, id as u32)?;
        ensure!(
            techniques
                .insert(
                    name.to_owned(),
                    decode(image, bank, slot, row, table, id, &mappings, &unlocks)
                        .with_context(|| format!("technique {id}"))?
                )
                .is_none(),
            "duplicate technique name {name}"
        );
    }
    for variant in elemental::VARIANTS {
        let base = mappings.target(u32::from(variant.base))?;
        let name = mappings.target(u32::from(variant.id))?;
        let mut entry = techniques
            .get(base)
            .context("missing elemental base technique")?
            .clone();
        entry.rules.element = Some(variant.element);
        entry.rules.required_level = 0;
        entry.rules.learn_on_level_up = false;
        entry.rules.learning = LearningRules::default();
        entry.menu.event_only = true;
        entry.menu.forget_with = vec![name.to_owned()];
        let description = entry.text["description"]
            .as_str()
            .unwrap_or("An elemental martial arte.")
            .to_owned();
        entry.text = serde_json::to_value(NamedText {
            name: variant.name.into(),
            description,
        })?;
        ensure!(
            techniques.insert(name.to_owned(), entry).is_none(),
            "duplicate elemental technique name {name}"
        );
    }
    let mut learning = BTreeMap::new();
    for (index, row) in lists.chunks_exact(41).take(9).enumerate() {
        let allowed = row
            .get(1..1 + usize::from(row[0]))
            .context("technique learning count exceeds source row")?;
        let mut ids: Vec<u16> = allowed
            .iter()
            .copied()
            .map(|id| match id {
                253 => 260,
                254 => 261,
                _ => u16::from(id),
            })
            .collect();
        for variant in elemental::VARIANTS {
            if ids.contains(&variant.base) && !ids.contains(&variant.id) {
                ids.push(variant.id);
            }
        }
        let owner = characters.target(index as u32 + 1)?;
        learning.insert(
            owner.to_owned(),
            ids.into_iter()
                .map(|id| Ok(mappings.target(u32::from(id))?.to_owned()))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok(Document {
        mappings,
        characters,
        techniques,
        learning,
    })
}
