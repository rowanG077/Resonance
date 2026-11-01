//! Named normal-attack roles and character-local hit recipes.
use super::{Attack, Continuation, Event};
use crate::{
    battle_action::{ActionHit, MeleeContact},
    library::Mappings,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Neutral,
    Rising,
    Thrust,
    Low,
    Finisher,
    AerialSlash,
    AerialThrust,
}

impl Role {
    pub const ALL: [Self; 7] = [
        Self::Neutral,
        Self::Rising,
        Self::Thrust,
        Self::Low,
        Self::Finisher,
        Self::AerialSlash,
        Self::AerialThrust,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Rising => "rising",
            Self::Thrust => "thrust",
            Self::Low => "low",
            Self::Finisher => "finisher",
            Self::AerialSlash => "aerial-slash",
            Self::AerialThrust => "aerial-thrust",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choices {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub directions: BTreeSet<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Role>,
}

impl TryFrom<Continuation> for Choices {
    type Error = anyhow::Error;
    fn try_from(value: Continuation) -> Result<Self> {
        ensure!(
            value.directions & 0x80 == 0,
            "invalid normal-attack continuation mask"
        );
        Ok(Self {
            directions: Role::ALL
                .into_iter()
                .filter(|role| value.directions & (1 << *role as u8) != 0)
                .collect(),
            fallback: value
                .fallback
                .map(|index| {
                    Role::ALL
                        .get(usize::from(index))
                        .copied()
                        .context("invalid normal-attack fallback")
                })
                .transpose()?,
        })
    }
}

impl From<Choices> for Continuation {
    fn from(value: Choices) -> Self {
        Self {
            directions: value
                .directions
                .into_iter()
                .fold(0, |bits, role| bits | (1 << role as u8)),
            fallback: value.fallback.map(|role| role as u8),
        }
    }
}

/// A repeated hit is named locally. A one-off hit stays beside its contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Hit {
    Shared(String),
    Inline(ActionHit),
}

type NamedAttack = Attack<Event<Hit>, Choices>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Track {
    Shared(Role),
    Inline(NamedAttack),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub attacks: BTreeMap<Role, Track>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hits: BTreeMap<String, ActionHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalAttacks {
    pub mappings: Mappings,
    pub characters: BTreeMap<String, Character>,
}

impl NormalAttacks {
    pub fn prepare(self) -> Result<Vec<[Attack; 7]>> {
        ensure!(
            self.mappings.iter().map(|(id, _)| id).eq(1..=9),
            "normal attacks require character mappings 1 through 9"
        );
        self.mappings
            .iter()
            .map(|(id, name)| {
                self.characters
                    .get(name)
                    .with_context(|| format!("missing normal attacks for character {id}: {name}"))?
                    .prepare()
                    .with_context(|| format!("normal attacks for {name}"))
            })
            .collect()
    }
}

/// Map only hit settings. The sequence and timestamps of every event stay intact.
pub(crate) fn map_hits<H, T>(
    event: Event<H>,
    mut map: impl FnMut(H) -> Result<T>,
) -> Result<Event<T>> {
    Ok(match event {
        Event::Contact(MeleeContact {
            duration,
            attachments,
            shape,
            radius,
            height,
            hit,
        }) => Event::Contact(MeleeContact {
            duration,
            attachments,
            shape,
            radius,
            height,
            hit: map(hit)?,
        }),
        Event::Projectile {
            member,
            hit,
            feedback,
        } => Event::Projectile {
            member,
            hit: hit.map(&mut map).transpose()?,
            feedback,
        },
        Event::Throw {
            slot,
            flight,
            radius,
            hit,
        } => Event::Throw {
            slot,
            flight,
            radius,
            hit: map(hit)?,
        },
        Event::Effect(v) => Event::Effect(v),
        Event::Pose(v) => Event::Pose(v),
        Event::AnimationRate(v) => Event::AnimationRate(v),
        Event::Forward(v) => Event::Forward(v),
        Event::Vertical(v) => Event::Vertical(v),
        Event::Gravity(v) => Event::Gravity(v),
        Event::Turn(v) => Event::Turn(v),
        Event::TraverseTarget {
            duration,
            clearance,
        } => Event::TraverseTarget {
            duration,
            clearance,
        },
        Event::ReactionArmor(v) => Event::ReactionArmor(v),
        Event::SecondaryWind { enabled, duration } => Event::SecondaryWind { enabled, duration },
        Event::CameraDistance { distance, duration } => {
            Event::CameraDistance { distance, duration }
        }
        Event::PassThrough(v) => Event::PassThrough(v),
        Event::Braking(v) => Event::Braking(v),
        Event::WeaponVisibility { slot, visible } => Event::WeaponVisibility { slot, visible },
        Event::Trail { slot, duration } => Event::Trail { slot, duration },
        Event::Sound(v) => Event::Sound(v),
        Event::Voice(v) => Event::Voice(v),
    })
}

impl Character {
    pub fn from_attacks(attacks: [Attack; 7]) -> Result<Self> {
        // Compare typed recipes only within this character during import. No hashes,
        // runtime content scans or cross-character sharing inferred from equal data.
        let mut recipes = Vec::<(ActionHit, usize, String)>::new();
        for (role, attack) in Role::ALL.into_iter().zip(&attacks) {
            for hit in attack.events.iter().filter_map(|(_, event)| event.hit()) {
                if let Some((_, count, _)) = recipes.iter_mut().find(|(other, ..)| other == hit) {
                    *count += 1;
                } else {
                    let mut name = role.name().to_owned();
                    let mut suffix = 2;
                    while recipes.iter().any(|(_, _, previous)| previous == &name) {
                        name = format!("{}-{suffix}", role.name());
                        suffix += 1;
                    }
                    recipes.push((*hit, 1, name));
                }
            }
        }
        let mut result = Self {
            attacks: BTreeMap::new(),
            hits: BTreeMap::new(),
        };
        for (hit, count, name) in &recipes {
            if *count > 1 {
                result.hits.insert(name.clone(), *hit);
            }
        }
        for (role, attack) in Role::ALL.into_iter().zip(attacks) {
            let attack = NamedAttack {
                continuation: attack.continuation.try_into()?,
                duration: attack.duration,
                chain_at: attack.chain_at,
                chain_retry_at: attack.chain_retry_at,
                recovery: attack.recovery,
                reach: attack.reach,
                events: attack
                    .events
                    .into_iter()
                    .map(|(at, event)| {
                        Ok((
                            at,
                            map_hits(event, |hit| {
                                let (_, count, name) =
                                    recipes.iter().find(|(other, ..)| *other == hit).unwrap();
                                Ok(if *count > 1 {
                                    Hit::Shared(name.clone())
                                } else {
                                    Hit::Inline(hit)
                                })
                            })?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            };
            let shared = result.attacks.iter().find_map(|(role, track)| match track {
                Track::Inline(previous) if previous == &attack => Some(*role),
                _ => None,
            });
            result.attacks.insert(
                role,
                shared.map(Track::Shared).unwrap_or(Track::Inline(attack)),
            );
        }
        Ok(result)
    }

    pub fn prepare(&self) -> Result<[Attack; 7]> {
        Role::ALL
            .into_iter()
            .map(|role| {
                let track = self
                    .attacks
                    .get(&role)
                    .with_context(|| format!("missing {} attack", role.name()))?;
                let attack = match track {
                    Track::Inline(attack) => attack,
                    Track::Shared(other) => match self.attacks.get(other) {
                        Some(Track::Inline(attack)) => attack,
                        _ => {
                            anyhow::bail!("{} must refer directly to an inline attack", role.name())
                        }
                    },
                };
                Ok(Attack {
                    continuation: attack.continuation.clone().into(),
                    duration: attack.duration,
                    chain_at: attack.chain_at,
                    chain_retry_at: attack.chain_retry_at,
                    recovery: attack.recovery,
                    reach: attack.reach,
                    events: attack
                        .events
                        .iter()
                        .cloned()
                        .map(|(at, event)| {
                            Ok((
                                at,
                                map_hits(event, |hit| match hit {
                                    Hit::Inline(hit) => Ok(hit),
                                    Hit::Shared(name) => self
                                        .hits
                                        .get(&name)
                                        .copied()
                                        .with_context(|| format!("missing hit recipe {name}")),
                                })?,
                            ))
                        })
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid normal attack count"))
    }
}

#[cfg(test)]
mod tests;
