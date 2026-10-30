//! Character-owned openings and named, ordered team-attack recipes.
use crate::{
    battle_action::ActionHit,
    battle_normal::{
        Event,
        document::{Hit, map_hits},
    },
    battle_unison::{self as runtime, Host},
    battle_voice::Sound,
    library::Mappings,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unison {
    pub host: Host,
    pub characters: BTreeMap<String, Character>,
    pub openers: BTreeMap<String, Opener>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hits: BTreeMap<String, ActionHit>,
    /// Matching action IDs are bridged once; recipes use these names.
    pub mappings: Mappings,
    pub placements: Vec<Placement>,
    /// List order is selection priority.
    pub combinations: Vec<Combination>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub opener: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<Sound>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opener {
    pub duration: u16,
    pub recovery: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_ticks: Option<u16>,
    pub events: Vec<(u16, Event<Hit>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub techniques: Vec<Technique>,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub altitude: u32,
    pub distance: u16,
    pub duration: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Technique {
    Named(String),
    Shared { technique: String, matches: String },
}
impl Technique {
    fn names(&self) -> (&str, &str) {
        match self {
            Self::Named(name) => (name, name),
            Self::Shared { technique, matches } => (technique, matches),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Combination {
    pub name: String,
    pub scene: String,
    pub recipes: Vec<Vec<String>>,
    /// Needed only for a dormant combination without a recipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participants: Option<u8>,
    pub duration: u16,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub camera_pitch: f32,
}

fn ids(mappings: &Mappings) -> Result<BTreeMap<&str, u16>> {
    let names = mappings
        .iter()
        .map(|(id, name)| Ok((name, u16::try_from(id)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        names.len() == mappings.iter().count(),
        "ambiguous Unison binding names"
    );
    Ok(names)
}

impl Unison {
    pub fn from_catalogue(
        value: &runtime::Catalogue,
        techniques: &Mappings,
        characters: &Mappings,
        scenes: &Mappings,
        groups: Mappings,
        opener_names: &Mappings,
    ) -> Result<Self> {
        value.validate()?;
        let mut openers = BTreeMap::new();
        let mut members = BTreeMap::new();
        for (index, source) in value.openers.iter().enumerate() {
            let id = u32::try_from(index + 1)?;
            let owner = opener_names.target(id)?.to_owned();
            let opener = Opener {
                duration: source.duration,
                recovery: source.recovery,
                contact_ticks: (source.contact_ticks != source.duration)
                    .then_some(source.contact_ticks),
                events: source
                    .events
                    .iter()
                    .cloned()
                    .map(|(at, event)| Ok((at, map_hits(event, |hit| Ok(Hit::Inline(hit)))?)))
                    .collect::<Result<_>>()?,
            };
            if let Some(existing) = openers.get(&owner) {
                ensure!(existing == &opener, "shared Unison opener {owner} differs");
            } else {
                openers.insert(owner.clone(), opener);
            }
            members.insert(
                characters.target(id)?.to_owned(),
                Character {
                    opener: owner,
                    voice: value.voices[index],
                },
            );
        }
        let mut hit_values = Vec::<(ActionHit, usize)>::new();
        for opener in openers.values() {
            for hit in opener.events.iter().filter_map(|(_, event)| event.hit()) {
                let Hit::Inline(hit) = hit else {
                    unreachable!()
                };
                if let Some((_, count)) = hit_values.iter_mut().find(|(value, _)| value == hit) {
                    *count += 1;
                } else {
                    hit_values.push((*hit, 1));
                }
            }
        }
        let mut hits = BTreeMap::new();
        for opener in openers.values_mut() {
            opener.events = opener
                .events
                .iter()
                .cloned()
                .map(|(at, event)| {
                    Ok((
                        at,
                        map_hits(event, |hit| {
                            let Hit::Inline(hit) = hit else {
                                unreachable!()
                            };
                            let (index, (_, count)) = hit_values
                                .iter()
                                .enumerate()
                                .find(|(_, (value, _))| *value == hit)
                                .unwrap();
                            if *count == 1 {
                                return Ok(Hit::Inline(hit));
                            }
                            let name = if index == 0 {
                                "opening-hit".into()
                            } else {
                                format!("opening-hit-{}", index + 1)
                            };
                            hits.insert(name.clone(), hit);
                            Ok(Hit::Shared(name))
                        })?,
                    ))
                })
                .collect::<Result<_>>()?;
        }
        let mut placements = Vec::<Placement>::new();
        for (&id, row) in &value.placements {
            let name = techniques.target(u32::from(id))?.to_owned();
            let group = groups.target(u32::from(row.native_id))?.to_owned();
            let selection = if name == group {
                Technique::Named(name)
            } else {
                Technique::Shared {
                    technique: name,
                    matches: group,
                }
            };
            if let Some(p) = placements.iter_mut().find(|p| {
                (p.altitude, p.distance, p.duration) == (row.altitude, row.distance, row.duration)
            }) {
                p.techniques.push(selection);
            } else {
                placements.push(Placement {
                    techniques: vec![selection],
                    altitude: row.altitude,
                    distance: row.distance,
                    duration: row.duration,
                });
            }
        }
        let combinations = value
            .combinations
            .iter()
            .map(|row| {
                Ok(Combination {
                    name: row.name.clone(),
                    scene: scenes.target(u32::from(row.native_id))?.to_owned(),
                    recipes: row
                        .recipes
                        .iter()
                        .map(|recipe| {
                            recipe
                                .iter()
                                .map(|&id| Ok(groups.target(u32::from(id))?.to_owned()))
                                .collect::<Result<_>>()
                        })
                        .collect::<Result<_>>()?,
                    participants: row.recipes.is_empty().then_some(row.participants),
                    duration: row.duration,
                    camera_pitch: row.camera_pitch,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            host: value.host.clone(),
            characters: members,
            openers,
            hits,
            mappings: groups,
            placements,
            combinations,
        })
    }

    pub fn prepare(
        &self,
        techniques: &Mappings,
        characters: &Mappings,
        scenes: &Mappings,
    ) -> Result<runtime::Catalogue> {
        let technique_ids = ids(techniques)?;
        let group_ids = ids(&self.mappings)?;
        let scene_ids = ids(scenes)?;
        ensure!(
            characters.iter().map(|(id, _)| id).eq(1..=9),
            "Unison requires all playable characters"
        );
        let mut openers = Vec::new();
        let mut voices = [None; 9];
        for (id, name) in characters.iter() {
            let member = self
                .characters
                .get(name)
                .with_context(|| format!("missing Unison character {name}"))?;
            let opener = self
                .openers
                .get(&member.opener)
                .with_context(|| format!("missing Unison opener {}", member.opener))?;
            openers.push(runtime::Opener {
                duration: opener.duration,
                recovery: opener.recovery,
                contact_ticks: opener.contact_ticks.unwrap_or(opener.duration),
                events: opener
                    .events
                    .iter()
                    .cloned()
                    .map(|(at, event)| {
                        Ok((
                            at,
                            map_hits(event, |hit| match hit {
                                Hit::Inline(value) => Ok(value),
                                Hit::Shared(name) => self
                                    .hits
                                    .get(&name)
                                    .copied()
                                    .with_context(|| format!("missing Unison hit {name}")),
                            })?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            });
            voices[id as usize - 1] = member.voice;
        }
        let mut placements = BTreeMap::new();
        for group in &self.placements {
            ensure!(!group.techniques.is_empty(), "empty Unison placement group");
            for technique in &group.techniques {
                let (name, matches) = technique.names();
                let id = *technique_ids
                    .get(name)
                    .with_context(|| format!("unknown Unison technique {name}"))?;
                let native_id = *group_ids
                    .get(matches)
                    .with_context(|| format!("unknown Unison matching group {matches}"))?;
                ensure!(
                    id != 0
                        && placements
                            .insert(
                                id,
                                runtime::Placement {
                                    native_id,
                                    altitude: group.altitude,
                                    distance: group.distance,
                                    duration: group.duration
                                }
                            )
                            .is_none(),
                    "duplicate Unison placement for {name}"
                );
            }
        }
        let combinations = self
            .combinations
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let participants = row
                    .participants
                    .or_else(|| {
                        row.recipes
                            .first()
                            .and_then(|recipe| u8::try_from(recipe.len()).ok())
                    })
                    .context("a dormant Unison combination needs its participant count")?;
                Ok(runtime::Combination {
                    id: u8::try_from(index + 1)?,
                    name: row.name.clone(),
                    native_id: *scene_ids
                        .get(row.scene.as_str())
                        .with_context(|| format!("unknown Unison scene {}", row.scene))?,
                    participants,
                    duration: row.duration,
                    camera_pitch: row.camera_pitch,
                    recipes: row
                        .recipes
                        .iter()
                        .map(|recipe| {
                            recipe
                                .iter()
                                .map(|name| {
                                    group_ids.get(name.as_str()).copied().with_context(|| {
                                        format!("unknown Unison ingredient {name}")
                                    })
                                })
                                .collect::<Result<_>>()
                        })
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        let value = runtime::Catalogue {
            host: self.host.clone(),
            voices,
            openers,
            placements,
            combinations,
        };
        value.validate()?;
        Ok(value)
    }
}
