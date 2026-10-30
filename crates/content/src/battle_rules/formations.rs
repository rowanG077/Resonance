//! Named encounter definitions. Numeric request IDs only occur in mappings and pools.
use crate::{battle_formation as runtime, library::Mappings};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Formations {
    pub mappings: Mappings,
    pub formations: BTreeMap<String, Formation>,
    pub pools: Vec<Pool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub ids: Vec<u16>,
    /// Order is significant: the same random draw must select the same request ID.
    pub choices: Vec<runtime::WeightedFormation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Formation {
    pub actors: Vec<Actor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reserves: Vec<Reserve>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub play_music: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub celebrate: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub entry_voice: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub escape_restricted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reserve {
    pub enemy: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden_name: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub enemy: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub count: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden_name: bool,
    #[serde(default, skip_serializing_if = "zero")]
    #[serde(rename = "statistics_variant")]
    pub variant: u8,
    #[serde(default, skip_serializing_if = "zero")]
    pub texture_variant: u8,
    #[serde(default, skip_serializing_if = "zero")]
    pub weapon_variant: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[i16; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

fn yes() -> bool {
    true
}
fn one() -> u8 {
    1
}
fn is_true(value: &bool) -> bool {
    *value
}
fn is_false(value: &bool) -> bool {
    !value
}
fn zero(value: &u8) -> bool {
    *value == 0
}
fn is_one(value: &u8) -> bool {
    *value == 1
}

impl Formations {
    pub fn from_table(
        table: &runtime::Formations,
        enemies: &Mappings,
        names: &Mappings,
    ) -> Result<Self> {
        let mut mappings = BTreeMap::new();
        let mut formations = BTreeMap::<String, Formation>::new();
        for (id, row) in table.records.iter().enumerate() {
            row.validate()?;
            let name = |enemy: u16| -> Result<String> {
                Ok(enemies
                    .target(u32::from(enemy))
                    .context("formation enemy has no name")?
                    .to_owned())
            };
            let mut actors = Vec::<Actor>::new();
            for actor in &row.actors {
                let resource = &row.resources[usize::from(actor.resource)];
                let entry = Actor {
                    enemy: name(resource.enemy)?,
                    count: 1,
                    hidden_name: resource.hidden_name,
                    variant: actor.variant,
                    texture_variant: actor.texture_variant,
                    weapon_variant: actor.weapon_variant,
                    position: actor.position,
                    unsupported_reason: actor.unsupported_reason.clone(),
                };
                if let Some(last) = actors.last_mut() {
                    let mut single = last.clone();
                    single.count = 1;
                    if single == entry && last.count < u8::MAX {
                        last.count += 1;
                        continue;
                    }
                }
                actors.push(entry);
            }
            let reserves = row
                .resources
                .iter()
                .enumerate()
                .filter(|(slot, _)| !row.actors.iter().any(|a| usize::from(a.resource) == *slot))
                .map(|(_, resource)| {
                    Ok(Reserve {
                        enemy: name(resource.enemy)?,
                        hidden_name: resource.hidden_name,
                    })
                })
                .collect::<Result<_>>()?;
            let definition = Formation {
                actors,
                reserves,
                play_music: row.settings.play_music,
                celebrate: row.settings.celebrate,
                entry_voice: row.settings.entry_voice,
                escape_restricted: row.settings.escape_restricted,
            };
            let owner = names.target(u32::try_from(id)?)?.to_owned();
            if let Some(existing) = formations.get(&owner) {
                ensure!(
                    *existing == definition,
                    "shared formation {owner} differs at request {id}"
                );
            } else {
                formations.insert(owner.clone(), definition);
            }
            mappings.insert(u32::try_from(id)?, owner);
        }
        let mut pools = Vec::<Pool>::new();
        for (id, choices) in table.pools.iter().enumerate() {
            let id = u16::try_from(id)?;
            if let Some(pool) = pools.iter_mut().find(|pool| pool.choices == *choices) {
                pool.ids.push(id);
            } else {
                pools.push(Pool {
                    ids: vec![id],
                    choices: choices.clone(),
                });
            }
        }
        Ok(Self {
            mappings: Mappings::new(mappings)?,
            formations,
            pools,
        })
    }

    pub fn prepare(&self, enemies: &Mappings) -> Result<runtime::Formations> {
        let names = enemies
            .iter()
            .map(|(id, name)| Ok((name, u16::try_from(id)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        ensure!(
            names.len() == enemies.iter().count(),
            "ambiguous formation enemy names"
        );
        let count = self
            .mappings
            .iter()
            .map(|(id, _)| id)
            .max()
            .context("empty formation catalogue")? as usize
            + 1;
        ensure!(
            count <= usize::from(u16::MAX) + 1 && count == self.mappings.iter().count(),
            "formation mappings must cover every request ID"
        );
        let mut prepared = BTreeMap::new();
        for (name, row) in &self.formations {
            let mut resources = Vec::<runtime::Resource>::new();
            let mut resource = |name: &str, hidden_name| -> Result<u8> {
                let enemy = *names
                    .get(name)
                    .with_context(|| format!("unknown formation enemy {name}"))?;
                let value = runtime::Resource { enemy, hidden_name };
                let slot = match resources.iter().position(|r| r == &value) {
                    Some(slot) => slot,
                    None => {
                        resources.push(value);
                        resources.len() - 1
                    }
                };
                Ok(u8::try_from(slot).context("too many formation enemy kinds")?)
            };
            let mut actors = Vec::new();
            for actor in &row.actors {
                ensure!(
                    actor.count > 0,
                    "formation {name} has zero actors in a group"
                );
                let slot = resource(&actor.enemy, actor.hidden_name)?;
                actors.extend((0..actor.count).map(|_| runtime::Actor {
                    resource: slot,
                    variant: actor.variant,
                    texture_variant: actor.texture_variant,
                    weapon_variant: actor.weapon_variant,
                    position: actor.position,
                    unsupported_reason: actor.unsupported_reason.clone(),
                }));
            }
            for reserve in &row.reserves {
                resource(&reserve.enemy, reserve.hidden_name)?;
            }
            prepared.insert(
                name.as_str(),
                runtime::Formation {
                    actors,
                    resources,
                    settings: runtime::Settings {
                        play_music: row.play_music,
                        celebrate: row.celebrate,
                        entry_voice: row.entry_voice,
                        escape_restricted: row.escape_restricted,
                    },
                },
            );
        }
        let records = (0..count)
            .map(|id| {
                let owner = self
                    .mappings
                    .target(id as u32)
                    .context("missing formation mapping")?;
                prepared
                    .get(owner)
                    .cloned()
                    .with_context(|| format!("missing formation {owner}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let pool_count = self
            .pools
            .iter()
            .flat_map(|p| &p.ids)
            .copied()
            .max()
            .map_or(0, |id| usize::from(id) + 1);
        let mut pools = vec![None; pool_count];
        for pool in &self.pools {
            ensure!(
                !pool.ids.is_empty() && !pool.choices.is_empty(),
                "empty encounter pool"
            );
            ensure!(
                pool.choices.iter().all(
                    |choice| choice.weight > 0 && usize::from(choice.formation) < records.len()
                ),
                "invalid encounter pool choice"
            );
            for &id in &pool.ids {
                ensure!(
                    pools[usize::from(id)]
                        .replace(pool.choices.clone())
                        .is_none(),
                    "duplicate encounter pool {id}"
                );
            }
        }
        let pools = pools
            .into_iter()
            .enumerate()
            .map(|(id, pool)| pool.with_context(|| format!("missing encounter pool {id}")))
            .collect::<Result<_>>()?;
        Ok(runtime::Formations { records, pools })
    }
}
