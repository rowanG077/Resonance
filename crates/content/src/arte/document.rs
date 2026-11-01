//! One named source for technique rules, menu behavior, text and learning.
use super::{Catalogue, Definition, LearningPrerequisite, LearningRules, TechniqueTarget};
use crate::{
    diagnostics::Diagnostics,
    library::Mappings,
    menu_data::{Element, NamedText, Technique, TechniqueUse},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub mappings: Mappings,
    pub characters: Mappings,
    pub techniques: BTreeMap<String, Entry>,
    pub learning: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub rules: Definition<String, Option<Element>>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub menu: Menu,
    /// A malformed optional caption must not invalidate its gameplay rules.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub text: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Menu {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tp_percent: bool,
    /// Selects the optional technique-menu marker sprite; zero means no marker.
    #[serde(skip_serializing_if = "crate::defaults::is_default")]
    pub rank: u8,
    /// Event-only menu entries remain hidden until acquired.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub event_only: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forget_with: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_use: Option<TechniqueUse>,
}

pub struct Prepared {
    pub mappings: Mappings,
    pub catalogue: Catalogue,
    pub menu: Vec<Technique>,
    pub text: Vec<Value>,
}

pub fn load(files: &crate::prepared::Files) -> Result<Prepared> {
    files.json::<Document>(super::PATH)?.prepare()
}

impl Prepared {
    pub fn captions(&self, diagnostics: &Diagnostics) -> Result<Vec<Option<NamedText>>> {
        self.text
            .iter()
            .enumerate()
            .map(|(id, value)| {
                if value.is_null() {
                    return Ok(None);
                }
                diagnostics.attempt(
                    &format!("technique caption {id}"),
                    serde_json::from_value(value.clone()).map_err(anyhow::Error::from),
                )
            })
            .collect()
    }

    pub fn names(&self, diagnostics: &Diagnostics) -> Result<BTreeMap<u16, String>> {
        let mut names = BTreeMap::new();
        for (id, value) in self
            .text
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.is_null())
        {
            if let Some(name) = diagnostics.attempt(
                &format!("technique name {id}"),
                value["name"]
                    .as_str()
                    .context("technique name must be text"),
            )? {
                names.insert(u16::try_from(id)?, name.to_owned());
            }
        }
        Ok(names)
    }

    /// Preserve catalogue ordering for acquisition notices and shortcut filling.
    pub fn level_techniques(&self, character: u8) -> Result<BTreeMap<u8, Vec<u16>>> {
        let mut levels = BTreeMap::<u8, Vec<u16>>::new();
        for &id in self.catalogue.learned_by(character)? {
            let rule = self.catalogue.definition(usize::from(id))?;
            if (1..=250).contains(&rule.required_level)
                && self.menu[usize::from(id)].route == 0
                && rule.learning.parent.is_none()
                && rule.learning.prerequisites.is_empty()
            {
                levels
                    .entry(rule.required_level as u8)
                    .or_default()
                    .push(id);
            }
        }
        Ok(levels)
    }
}

fn element(value: u8) -> Result<Option<Element>> {
    if value == 0 {
        return Ok(None);
    }
    Ok(Some(
        *Element::ALL
            .get(usize::from(value - 1))
            .context("invalid technique element")?,
    ))
}

fn convert<I, E, J, F>(
    value: Definition<I, E>,
    mut id: impl FnMut(I) -> Result<J>,
    element: impl FnOnce(E) -> Result<F>,
) -> Result<Definition<J, F>> {
    let learning = value.learning;
    Ok(Definition {
        action: value.action,
        tp_cost: value.tp_cost,
        element: element(value.element)?,
        learning: LearningRules {
            route: learning.route,
            parent: learning.parent.map(&mut id).transpose()?,
            technical_successor: learning.technical_successor.map(&mut id).transpose()?,
            strike_successor: learning.strike_successor.map(&mut id).transpose()?,
            parent_uses: learning.parent_uses,
            prerequisites: learning
                .prerequisites
                .into_iter()
                .map(|group| {
                    Ok(LearningPrerequisite {
                        any_of: group
                            .any_of
                            .into_iter()
                            .map(&mut id)
                            .collect::<Result<_>>()?,
                        minimum_uses: group.minimum_uses,
                    })
                })
                .collect::<Result<_>>()?,
            excludes: learning
                .excludes
                .into_iter()
                .map(&mut id)
                .collect::<Result<_>>()?,
            requires_story_unlock: learning.requires_story_unlock,
        },
        capabilities: value.capabilities,
        admission_flash: value.admission_flash,
        learn_on_level_up: value.learn_on_level_up,
        casting: value.casting,
        action_range: value.action_range,
        cast_time_adjustment: value.cast_time_adjustment,
        recovery_ticks: value.recovery_ticks,
        required_level: value.required_level,
    })
}

impl Document {
    /// Publish already prepared values. Importers may create elemental variants
    /// first; this function never regenerates or overwrites existing entries.
    pub fn from_parts(
        catalogue: &Catalogue,
        menu: &[Technique],
        text: &[Value],
        mappings: Mappings,
        characters: Mappings,
    ) -> Result<Self> {
        ensure!(
            catalogue.definitions.len() == menu.len() && menu.len() == text.len(),
            "technique rules, menu data and captions have different lengths"
        );
        let name = |id: u16| -> Result<String> { Ok(mappings.target(u32::from(id))?.to_owned()) };
        let mut techniques = BTreeMap::new();
        for (id, owner) in mappings.iter() {
            let index = usize::try_from(id)?;
            let rule = catalogue
                .definitions
                .get(index)
                .context("technique mapping outside catalogue")?;
            let row = &menu[index];
            let route = match rule.learning.route {
                None => 0,
                Some(super::LearningRoute::Technical) => 1,
                Some(super::LearningRoute::Strike) => 2,
            };
            ensure!(
                row.tp == rule.tp_cost
                    && row.element == rule.element
                    && row.level == rule.required_level
                    && row.prerequisite == rule.learning.parent.unwrap_or(0)
                    && row.unison_usable == rule.capabilities.offensive
                    && (row.route == route || row.route == 3 && route == 0),
                "menu and battle definitions disagree for {owner}"
            );
            ensure!(
                row.alternatives[0] != 0 || row.alternatives == [0; 4],
                "inactive forgetting alternatives for {owner}"
            );
            let forget_with = row
                .alternatives
                .into_iter()
                .filter(|id| *id != 0)
                .map(&name)
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                techniques
                    .insert(
                        owner.to_owned(),
                        Entry {
                            rules: convert(rule.clone(), &name, element)?,
                            menu: Menu {
                                tp_percent: row.tp_percent,
                                rank: row.rank,
                                event_only: row.route == 3,
                                forget_with,
                                field_use: row.field_use
                            },
                            text: text[index].clone(),
                        }
                    )
                    .is_none(),
                "duplicate technique owner {owner}"
            );
        }
        let mut learning = BTreeMap::new();
        for (id, owner) in characters.iter() {
            let list = catalogue.learned_by(id.try_into()?)?;
            ensure!(
                learning
                    .insert(
                        owner.to_owned(),
                        list.iter().copied().map(&name).collect::<Result<_>>()?
                    )
                    .is_none(),
                "duplicate learning character {owner}"
            );
        }
        Ok(Self {
            mappings,
            characters,
            techniques,
            learning,
        })
    }

    pub fn prepare(&self) -> Result<Prepared> {
        ensure!(
            self.characters.iter().map(|(id, _)| id).eq(1..=9),
            "techniques require the nine playable learning lists"
        );
        let mut ids = BTreeMap::new();
        let mut count = 0;
        for (id, owner) in self.mappings.iter() {
            let id = u16::try_from(id).context("technique ID exceeds u16")?;
            ensure!(
                id != 0 && ids.insert(owner, id).is_none(),
                "technique names require unique nonzero IDs"
            );
            count = count.max(usize::from(id) + 1);
        }
        ensure!(count > 1, "empty technique catalogue");
        let id = |name: String| -> Result<u16> {
            ids.get(name.as_str())
                .copied()
                .with_context(|| format!("unknown technique {name}"))
        };
        let mut unavailable = Definition::default();
        unavailable.capabilities.target = TechniqueTarget::Unavailable;
        unavailable.capabilities.uses_weapon_reach = true;
        let mut definitions = vec![unavailable; count];
        let mut menu = vec![Technique::default(); count];
        let mut text = vec![Value::Null; count];
        for (identity, owner) in self.mappings.iter() {
            let index = identity as usize;
            let entry = self
                .techniques
                .get(owner)
                .with_context(|| format!("missing technique {owner}"))?;
            let rule = convert(entry.rules.clone(), &id, |value| {
                Ok(value.map_or(0, |element| element as u8 + 1))
            })?;
            ensure!(
                entry.menu.rank <= 2 && entry.menu.forget_with.len() <= 4,
                "invalid menu rules for {owner}"
            );
            ensure!(
                !entry.menu.event_only || rule.learning.route.is_none(),
                "event-only technique cannot also select a T/S route: {owner}"
            );
            let mut alternatives = [0; 4];
            for (slot, name) in entry.menu.forget_with.iter().enumerate() {
                alternatives[slot] = id(name.clone())?;
            }
            menu[index] = Technique {
                tp: rule.tp_cost,
                tp_percent: entry.menu.tp_percent,
                unison_usable: rule.capabilities.offensive,
                rank: entry.menu.rank,
                element: rule.element,
                level: rule.required_level,
                route: if entry.menu.event_only {
                    3
                } else {
                    match rule.learning.route {
                        None => 0,
                        Some(super::LearningRoute::Technical) => 1,
                        Some(super::LearningRoute::Strike) => 2,
                    }
                },
                prerequisite: rule.learning.parent.unwrap_or(0),
                alternatives,
                field_use: entry.menu.field_use,
            };
            definitions[index] = rule;
            text[index] = entry.text.clone();
        }
        let learning = self
            .characters
            .iter()
            .map(|(_, name)| {
                self.learning
                    .get(name)
                    .with_context(|| format!("missing learning list for {name}"))?
                    .iter()
                    .cloned()
                    .map(&id)
                    .collect::<Result<_>>()
            })
            .collect::<Result<_>>()?;
        let catalogue = Catalogue {
            definitions,
            learning,
        };
        catalogue.validate()?;
        Ok(Prepared {
            mappings: self.mappings.clone(),
            catalogue,
            menu,
            text,
        })
    }
}
