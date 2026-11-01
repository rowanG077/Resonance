//! Named strategy choices, formation lanes and editable preset defaults.
use crate::{
    diagnostics::Diagnostics, library::Mappings, menu_data as runtime, overworld::PartyCharacter,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

mod choices;
pub use choices::{PositionStrategy, ResolvedChoices, TechniqueStrategy};

pub const PATH: &str = "game/strategy.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub groups: BTreeMap<Group, Choices>,
    pub preset_mappings: Mappings,
    pub presets: BTreeMap<String, Preset>,
    pub default_positions: BTreeMap<PartyCharacter, Lane>,
    #[serde(default)]
    pub keyboard: Value,
    #[serde(default)]
    pub labels: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Action,
    SkillMagic,
    Position,
}
impl Group {
    pub const ALL: [Self; 3] = [Self::Action, Self::SkillMagic, Self::Position];
    pub fn index(self) -> usize {
        self as usize
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Front,
    Middle,
    Rear,
}
impl Lane {
    pub fn from_index(index: u8) -> Result<Self> {
        match index {
            0 => Ok(Self::Front),
            1 => Ok(Self::Middle),
            2 => Ok(Self::Rear),
            _ => anyhow::bail!("invalid strategy lane {index}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choices {
    pub mappings: Mappings,
    #[serde(default)]
    pub name: Value,
    pub options: BTreeMap<String, Choice>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    #[serde(default)]
    pub text: Value,
    /// Omitted means all nine party members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub characters: Option<Vec<PartyCharacter>>,
    /// Fixed lane for position choices 1–6. Choice 0 uses character defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<Lane>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    #[serde(default)]
    pub name: Value,
    pub default: Orders,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub characters: BTreeMap<PartyCharacter, Overrides>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Orders {
    pub action: String,
    pub skill_magic: String,
    pub position: String,
}
impl Orders {
    pub fn choices(&self) -> [&str; 3] {
        [&self.action, &self.skill_magic, &self.position]
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_magic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
}
impl Overrides {
    fn choices(&self) -> [Option<&str>; 3] {
        [
            self.action.as_deref(),
            self.skill_magic.as_deref(),
            self.position.as_deref(),
        ]
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caption {
    pub name: String,
    /// One or two displayed lines; whitespace is significant.
    pub description: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyboard {
    pub rows: [String; 9],
    pub actions: Actions,
    #[serde(default = "blank_modes", skip_serializing_if = "modes_blank")]
    pub mode_labels: [String; 3],
}
fn blank_modes() -> [String; 3] {
    std::array::from_fn(|_| " ".into())
}
fn modes_blank(value: &[String; 3]) -> bool {
    *value == blank_modes()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actions {
    pub move_right: String,
    pub move_left: String,
    pub delete: String,
    pub confirm: String,
    pub cancel: String,
    pub initial: String,
}

fn ordered<'a, T>(
    mappings: &'a Mappings,
    entries: &'a BTreeMap<String, T>,
    count: usize,
) -> Result<Vec<&'a T>> {
    ensure!(
        entries.len() == count && mappings.iter().count() == count,
        "strategy mapping count differs from definitions"
    );
    let mut names = BTreeSet::new();
    mappings
        .iter()
        .enumerate()
        .map(|(index, (id, name))| {
            ensure!(
                id as usize == index && names.insert(name),
                "strategy mappings need distinct names and consecutive saved IDs starting at zero"
            );
            entries
                .get(name)
                .with_context(|| format!("missing strategy definition {name}"))
        })
        .collect()
}
impl Document {
    pub fn prepare(&self) -> Result<runtime::StrategyData> {
        ensure!(
            self.groups.len() == 3 && self.default_positions.len() == 9,
            "strategy needs three groups and nine character positions"
        );
        let mut data = runtime::StrategyData::default();
        let mut ids: [BTreeMap<&str, u8>; 3] = std::array::from_fn(|_| BTreeMap::new());
        for group in Group::ALL {
            let index = group.index();
            let choices = self.groups.get(&group).context("missing strategy group")?;
            for (id, row) in ordered(
                &choices.mappings,
                &choices.options,
                runtime::STRATEGY_COUNTS[index],
            )?
            .into_iter()
            .enumerate()
            {
                let characters = match &row.characters {
                    None => 511,
                    Some(characters) => {
                        let unique: BTreeSet<_> = characters.iter().collect();
                        ensure!(
                            !unique.is_empty() && unique.len() == characters.len(),
                            "strategy character eligibility must be nonempty and unique"
                        );
                        characters
                            .iter()
                            .fold(0, |mask, c| mask | (1 << (*c as u8 - 1)))
                    }
                };
                data.groups[index].push(runtime::StrategyOption { characters });
                if group == Group::Position && id > 0 {
                    data.positions[id - 1] =
                        row.lane.context("fixed position choice needs a lane")? as u8;
                } else {
                    ensure!(
                        row.lane.is_none(),
                        "only fixed position choices have a lane"
                    );
                }
            }
            ids[index] = choices
                .mappings
                .iter()
                .map(|(id, name)| (name, id as u8))
                .collect();
        }
        for (character, lane) in &self.default_positions {
            data.default_positions[*character as usize - 1] = *lane as u8;
        }
        for (index, preset) in ordered(&self.preset_mappings, &self.presets, 3)?
            .into_iter()
            .enumerate()
        {
            let mut defaults = [0; 3];
            for (group, name) in preset.default.choices().into_iter().enumerate() {
                defaults[group] = *ids[group]
                    .get(name)
                    .with_context(|| format!("unknown strategy default {name}"))?;
            }
            data.presets[index] = [defaults; 9];
            for (character, overrides) in &preset.characters {
                for (group, name) in overrides.choices().into_iter().enumerate() {
                    if let Some(name) = name {
                        data.presets[index][*character as usize - 1][group] = *ids[group]
                            .get(name)
                            .with_context(|| format!("unknown strategy choice {name}"))?;
                    }
                }
            }
        }
        data.validate_rules()?;
        Ok(data)
    }

    pub fn captions(&self, diagnostics: &Diagnostics) -> Result<runtime::StrategyText> {
        let mut groups: [Vec<Option<runtime::ItemText>>; 3] = std::array::from_fn(|_| Vec::new());
        let mut labels = Vec::new();
        for group in Group::ALL {
            let choices = self.groups.get(&group).context("missing strategy group")?;
            labels.push(choices.name.clone());
            for row in ordered(
                &choices.mappings,
                &choices.options,
                runtime::STRATEGY_COUNTS[group.index()],
            )? {
                let caption = (|| -> Result<_> {
                    let caption: Caption = serde_json::from_value(row.text.clone())?;
                    ensure!(
                        (1..=2).contains(&caption.description.len()),
                        "strategy description needs one or two lines"
                    );
                    Ok(runtime::ItemText {
                        name: caption.name,
                        description: caption.description[0].clone(),
                        details: caption.description.get(1).cloned().unwrap_or_default(),
                    })
                })();
                groups[group.index()].push(diagnostics.attempt("Strategy option", caption)?);
            }
        }
        let names: Vec<_> = ordered(&self.preset_mappings, &self.presets, 3)?
            .into_iter()
            .map(|p| p.name.clone())
            .collect();
        // Admit keyboard cells and action captions independently, as before.
        let keyboard = diagnostics.attempt(
            "Strategy keyboard",
            (|| -> Result<String> {
                let rows: [String; 9] =
                    serde_json::from_value(self.keyboard.get("rows").cloned().unwrap_or_default())?;
                ensure!(
                    rows.iter().all(|row| row.len() == 10 && row.is_ascii()),
                    "strategy keyboard needs nine rows of ten ASCII cells"
                );
                Ok(rows.concat())
            })(),
        )?;
        let keys = diagnostics.attempt(
            "Strategy keyboard labels",
            (|| -> Result<_> {
                let actions: Actions = serde_json::from_value(
                    self.keyboard.get("actions").cloned().unwrap_or_default(),
                )?;
                let modes = match self.keyboard.get("mode_labels") {
                    Some(value) => serde_json::from_value(value.clone())?,
                    None => blank_modes(),
                };
                let [a, b, c] = modes;
                Ok([
                    a,
                    b,
                    c,
                    actions.move_right,
                    actions.move_left,
                    actions.delete,
                    actions.confirm,
                    actions.cancel,
                    actions.initial,
                ])
            })(),
        )?;
        Ok(runtime::StrategyText {
            groups,
            keyboard,
            keys,
            presets: diagnostics.attempt(
                "Strategy presets",
                serde_json::from_value(Value::Array(names)).map_err(Into::into),
            )?,
            labels: diagnostics.attempt(
                "Strategy group labels",
                serde_json::from_value(Value::Array(labels)).map_err(Into::into),
            )?,
        })
    }
}
