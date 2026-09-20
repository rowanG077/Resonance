//! Source menu records retain unused rows and authored model selectors.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(crate) struct FigurineRow {
    pub slot: usize,
    pub name: Option<String>,
    description: Option<String>,
    pub model: FigurineModel,
    pub elevation: f32,
    pub appearance_row: u32,
    /// Ordered prefix rules; null entries remain null rather than disappearing.
    pub bone_rules: Vec<Option<BoneRule>>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum FigurineModel {
    Null,
    Unavailable,
    /// Both untagged and 0x20000-tagged IDs address the declared NPC archive.
    Npc {
        entry: u32,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BoneRule {
    Hide { prefix: String },
    Show { prefix: String },
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Figurines {
    pub title: String,
    pub archive: String,
    pub hidden_prefix: String,
    pub records: Vec<FigurineRow>,
}

pub(super) fn figurines(
    source: &crate::all_assets::figurine_catalogue::Catalogue,
) -> Result<Figurines> {
    use crate::all_assets::figurine_catalogue::Resource;
    const TAGGED_EFREET: Resource = Resource::TaggedNpc(73);
    Ok(Figurines {
        title: source.required_text(source.title)?.into(),
        archive: source.text(source.archive.text).into(),
        hidden_prefix: source.text(source.preview.hidden_prefix.text).into(),
        records: source
            .records
            .iter()
            .enumerate()
            .map(|(slot, row)| {
                let model = if row.is_null() {
                    FigurineModel::Null
                } else {
                    match row.resource {
                        Resource::Unavailable => FigurineModel::Unavailable,
                        Resource::DirectNpc(entry) | Resource::TaggedNpc(entry) => {
                            FigurineModel::Npc { entry }
                        }
                        Resource::Negative(selector) => {
                            anyhow::bail!("invalid figurine resource {slot}: {selector}")
                        }
                    }
                };
                Ok(FigurineRow {
                    slot,
                    name: row.name.map(|id| source.text(id).into()),
                    description: row.description.map(|id| source.text(id).into()),
                    model,
                    // Efreet's tagged entry lowers the preview origin; its untagged NPC does not.
                    elevation: if row.resource == TAGGED_EFREET {
                        source.preview.tagged_efreet_elevation
                    } else {
                        source.preview.default_elevation
                    },
                    appearance_row: row.appearance_row,
                    bone_rules: row
                        .bone_rules
                        .map(|reference| {
                            reference.map(|id| {
                                let rule = source.text(id);
                                match rule.strip_prefix('-') {
                                    Some(prefix) => BoneRule::Hide {
                                        prefix: prefix.into(),
                                    },
                                    None => BoneRule::Show {
                                        prefix: rule.into(),
                                    },
                                }
                            })
                        })
                        .into(),
                })
            })
            .collect::<Result<_>>()?,
    })
}

#[derive(Serialize, Deserialize)]
pub(crate) struct MonsterRow {
    pub slot: usize,
    pub name: Option<String>,
    description: Option<String>,
    /// Group used by the script query that counts undiscovered monsters.
    unseen_count_group: u8,
    location_id: u8,
    pub location: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Monsters {
    pub labels: BTreeMap<String, String>,
    pub records: Vec<MonsterRow>,
    pub categories: Vec<String>,
}

/// Enemy packages separately supply combat statistics and preview resources.
pub(super) fn monsters(
    source: &crate::all_assets::monster_catalogue::Catalogue,
    ui: &super::inventory_ui::Catalogue,
) -> Result<Monsters> {
    use crate::all_assets::monster_catalogue::TextKind;
    let monster = &ui.inventory.monster;
    let mut labels = BTreeMap::new();
    for (key, kind) in [
        ("number", TextKind::Number),
        ("hp", TextKind::Hp),
        ("tp", TextKind::Tp),
        ("unknown_item", TextKind::UnknownItem),
    ] {
        labels.insert(key.into(), source.direct_text(kind).into());
    }
    for (key, reference) in [
        ("title", monster.title),
        ("normal", monster.difficulties[0]),
        ("hard", monster.difficulties[1]),
        ("mania", monster.difficulties[2]),
        ("attack", monster.attack),
        ("experience", monster.experience),
        ("gald", monster.gald),
        ("defense", monster.defense),
        ("drops", monster.drops),
        ("steal", monster.steal),
        ("location", monster.location),
        ("attack_element", monster.attack_element),
        ("weak", monster.weak),
        ("strong", monster.strong),
        ("battle_rank", monster.battle_rank),
    ] {
        labels.insert(key.into(), ui.text(reference).to_owned());
    }
    labels.insert(
        "unknown_stat".into(),
        source.required_text(source.stat_labels.unknown)?.into(),
    );
    Ok(Monsters {
        labels,
        categories: monster
            .categories
            .map(|reference| ui.text(reference).to_owned())
            .into(),
        records: source
            .records
            .iter()
            .enumerate()
            .map(|(slot, row)| {
                Ok(MonsterRow {
                    slot,
                    name: row.name.map(|id| source.text(id).into()),
                    description: row.description.map(|id| source.text(id).into()),
                    unseen_count_group: row.unseen_count_group,
                    location_id: row.location,
                    location: source.location(row.location)?.into(),
                })
            })
            .collect::<Result<_>>()?,
    })
}
