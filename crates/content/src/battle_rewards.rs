//! Items a mod awards for winning a particular battle or defeating a
//! particular enemy, in addition to the original drops. Every reward is its
//! own owner, so mods add to the same battle without replacing one another.
use crate::library::Mappings;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Absent from the original game; a library without it awards nothing extra.
pub const PATH: &str = "battle/rewards.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Document {
    /// Formation ID, then a name the mod chooses for its reward.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub formations: BTreeMap<u16, BTreeMap<String, Reward>>,
    /// Monster List ID, then a name the mod chooses for its reward.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub monsters: BTreeMap<u8, BTreeMap<String, Reward>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reward {
    pub item: String,
    /// Percent chance, rolled once per victory or per defeated enemy. Drop
    /// bonuses from skills and equipment apply to original drops only.
    #[serde(default = "certain", skip_serializing_if = "is_certain")]
    pub chance: u8,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub count: u8,
}

fn certain() -> u8 {
    100
}
fn is_certain(chance: &u8) -> bool {
    *chance == 100
}
fn one() -> u8 {
    1
}
fn is_one(count: &u8) -> bool {
    *count == 1
}

/// A reward bound to the current item IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Award {
    pub item: u16,
    pub chance: u8,
    pub count: u8,
}

/// Rewards in the stable order of their authored names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prepared {
    pub formations: BTreeMap<u16, Vec<Award>>,
    pub monsters: BTreeMap<u8, Vec<Award>>,
}

impl Document {
    pub fn prepare(&self, items: &Mappings) -> Result<Prepared> {
        let bind = |scope: &str, rewards: &BTreeMap<String, Reward>| -> Result<Vec<Award>> {
            rewards
                .iter()
                .map(|(name, reward)| {
                    let scope = || format!("battle reward {name} of {scope}");
                    ensure!(
                        (1..=100).contains(&reward.chance) && (1..=99).contains(&reward.count),
                        "{}: chance is 1-100 and count 1-99",
                        scope()
                    );
                    let item = items
                        .id(&reward.item)
                        .with_context(|| format!("{}: unknown item {}", scope(), reward.item))?;
                    ensure!(item != 0, "{}: the empty item cannot be awarded", scope());
                    Ok(Award {
                        item: u16::try_from(item)?,
                        chance: reward.chance,
                        count: reward.count,
                    })
                })
                .collect()
        };
        Ok(Prepared {
            formations: self
                .formations
                .iter()
                .map(|(&id, rewards)| Ok((id, bind(&format!("formation {id}"), rewards)?)))
                .collect::<Result<_>>()?,
            monsters: self
                .monsters
                .iter()
                .map(|(&id, rewards)| {
                    ensure!(
                        usize::from(id) < crate::monster::MONSTER_COUNT,
                        "battle rewards name unknown monster {id}"
                    );
                    Ok((id, bind(&format!("monster {id}"), rewards)?))
                })
                .collect::<Result<_>>()?,
        })
    }
}

/// The library's extra rewards, or none when no mod declares any.
pub fn load(files: &crate::prepared::Files, items: &Mappings) -> Result<Prepared> {
    if !files.contains_key(PATH) {
        return Ok(Prepared::default());
    }
    files.json::<Document>(PATH)?.prepare(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewards_bind_named_items_and_reject_impossible_ones() -> Result<()> {
        let items: Mappings =
            serde_json::from_value(serde_json::json!({"0":"no-item","7":"moon-berry"}))?;
        let document: Document = serde_json::from_value(serde_json::json!({
            "formations": {"12": {
                "b-second": {"item":"moon-berry","chance":25,"count":2},
                "a-first": {"item":"moon-berry"}
            }},
            "monsters": {"3": {"berry": {"item":"moon-berry","chance":5}}}
        }))?;
        let prepared = document.prepare(&items)?;
        assert_eq!(
            prepared.formations[&12],
            [
                Award {
                    item: 7,
                    chance: 100,
                    count: 1
                },
                Award {
                    item: 7,
                    chance: 25,
                    count: 2
                }
            ]
        );
        assert_eq!(prepared.monsters[&3][0].chance, 5);
        for broken in [
            serde_json::json!({"formations": {"1": {"x": {"item":"missing"}}}}),
            serde_json::json!({"formations": {"1": {"x": {"item":"no-item"}}}}),
            serde_json::json!({"formations": {"1": {"x": {"item":"moon-berry","chance":0}}}}),
            serde_json::json!({"monsters": {"251": {"x": {"item":"moon-berry"}}}}),
        ] {
            let document: Document = serde_json::from_value(broken.clone())?;
            assert!(document.prepare(&items).is_err(), "{broken}");
        }
        Ok(())
    }
}
