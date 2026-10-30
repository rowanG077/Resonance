//! Named technique owners, local variants and reusable contact definitions.
use super::{Attack, Binding, Hammer, HammerPattern, Member, Steal};
use crate::{
    battle_action::MeleeContact, battle_normal::Event as Command, battle_scene::SceneAction,
    library::Mappings,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub mappings: Mappings,
    pub bindings: BTreeMap<u16, BTreeMap<u8, Binding>>,
    pub techniques: BTreeMap<String, Technique>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Technique {
    pub mappings: Mappings,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variants: BTreeMap<String, Variant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contacts: BTreeMap<String, MeleeContact>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projectiles: BTreeMap<String, Projectile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Variant {
    Prepared(Box<Attack<Event>>),
    Unsupported(Unavailable),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unavailable {
    pub unsupported_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    Reference(Reference),
    Command(Command),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    Contact(String),
    Projectile(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projectile {
    pub member: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit: Option<crate::battle_action::ActionHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<super::ProjectileFeedback>,
}

/// Alternate hammer and successful theft tracks are part of the same editing
/// scope as the main track. Every scalar and same-tick event order is preserved.
fn map_events<A, B>(attack: Attack<A>, mut map: impl FnMut(A) -> Result<B>) -> Result<Attack<B>> {
    fn track<A, B>(
        events: Vec<(u16, A)>,
        map: &mut impl FnMut(A) -> Result<B>,
    ) -> Result<Vec<(u16, B)>> {
        events
            .into_iter()
            .map(|(at, event)| Ok((at, map(event)?)))
            .collect()
    }
    Ok(Attack {
        duration: attack.duration,
        chain_at: attack.chain_at,
        recovery: attack.recovery,
        effect: attack.effect,
        events: track(attack.events, &mut map)?,
        callback_hits: attack.callback_hits,
        pose_sequence: attack.pose_sequence,
        flight: attack.flight,
        hammer: attack
            .hammer
            .map(|hammer| -> Result<_> {
                Ok(Hammer {
                    hits: hammer.hits,
                    impact_effect: hammer.impact_effect,
                    impact_sound: hammer.impact_sound,
                    pattern: match hammer.pattern {
                        HammerPattern::Single { alternate_events } => HammerPattern::Single {
                            alternate_events: track(alternate_events, &mut map)?,
                        },
                        HammerPattern::Volley {
                            offset,
                            forward,
                            jitter,
                        } => HammerPattern::Volley {
                            offset,
                            forward,
                            jitter,
                        },
                        HammerPattern::Ring {
                            offset,
                            step,
                            radians,
                            speed,
                            speed_jitter,
                            lift,
                            lift_jitter,
                            opening_hit,
                        } => HammerPattern::Ring {
                            offset,
                            step,
                            radians,
                            speed,
                            speed_jitter,
                            lift,
                            lift_jitter,
                            opening_hit,
                        },
                    },
                })
            })
            .transpose()?,
        steal: attack
            .steal
            .map(|steal| -> Result<_> {
                Ok(Steal {
                    attempt_at: steal.attempt_at,
                    bonus_at: steal.bonus_at,
                    success_duration: steal.success_duration,
                    success_events: track(steal.success_events, &mut map)?,
                    bonus_hit: steal.bonus_hit,
                    bonus_offset: steal.bonus_offset,
                    bonus_forward_speed: steal.bonus_forward_speed,
                    bonus_velocity_jitter: steal.bonus_velocity_jitter,
                })
            })
            .transpose()?,
        fierce_fang: attack.fierce_fang,
        hurricane_hit: attack.hurricane_hit,
    })
}

impl Catalogue {
    pub fn prepare(self) -> Result<super::Catalogue> {
        let members = self
            .mappings
            .iter()
            .map(|(id, name)| {
                let member = self
                    .techniques
                    .get(name)
                    .with_context(|| format!("missing technique {name}"))?
                    .prepare()
                    .with_context(|| format!("technique {name}"))?;
                Ok((
                    u16::try_from(id).context("technique member ID exceeds limit")?,
                    member,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(super::Catalogue {
            members,
            bindings: self.bindings,
        })
    }
}

impl Technique {
    pub fn from_member(member: Member, names: BTreeMap<u8, String>) -> Result<Self> {
        let mut result = Self {
            mappings: Mappings::new(
                names
                    .iter()
                    .map(|(&id, name)| (u32::from(id), name.clone()))
                    .collect(),
            )?,
            variants: BTreeMap::new(),
            contacts: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            unsupported_reason: member.unsupported_reason,
        };
        // Equality is local to this technique at import, never a runtime scan.
        let mut uses = Vec::<(Command, usize, String)>::new();
        let mut contacts = 0;
        let mut projectiles = 0;
        for variant in member.variants.values() {
            if let SceneAction::Prepared(attack) = variant {
                let alternate = attack
                    .hammer
                    .as_ref()
                    .and_then(|hammer| match &hammer.pattern {
                        HammerPattern::Single { alternate_events } => Some(alternate_events),
                        _ => None,
                    });
                let success = attack.steal.as_ref().map(|steal| &steal.success_events);
                for (_, event) in attack
                    .events
                    .iter()
                    .chain(alternate.into_iter().flatten())
                    .chain(success.into_iter().flatten())
                {
                    if matches!(event, Command::Contact(_) | Command::Projectile { .. }) {
                        if let Some((_, count, _)) =
                            uses.iter_mut().find(|(other, ..)| other == event)
                        {
                            *count += 1;
                        } else {
                            let (kind, count) = if matches!(event, Command::Contact(_)) {
                                ("strike", &mut contacts)
                            } else {
                                ("projectile", &mut projectiles)
                            };
                            *count += 1;
                            let name = if *count == 1 {
                                kind.to_owned()
                            } else {
                                format!("{kind}-{count}")
                            };
                            uses.push((event.clone(), 1, name));
                        }
                    }
                }
            }
        }
        for (id, variant) in member.variants {
            let name = names
                .get(&id)
                .with_context(|| format!("missing name for technique variant {id}"))?;
            let variant = match variant {
                SceneAction::Unsupported { reason } => Variant::Unsupported(Unavailable {
                    unsupported_reason: reason,
                }),
                SceneAction::Prepared(attack) => {
                    Variant::Prepared(Box::new(map_events(*attack, |event| {
                        let shared = uses
                            .iter()
                            .find(|(other, count, _)| *count > 1 && *other == event)
                            .map(|(_, _, name)| name.clone());
                        Ok(match (event, shared) {
                            (Command::Contact(contact), Some(name)) => {
                                result.contacts.insert(name.clone(), contact);
                                Event::Reference(Reference::Contact(name))
                            }
                            (
                                Command::Projectile {
                                    member,
                                    hit,
                                    feedback,
                                },
                                Some(name),
                            ) => {
                                result.projectiles.insert(
                                    name.clone(),
                                    Projectile {
                                        member,
                                        hit,
                                        feedback,
                                    },
                                );
                                Event::Reference(Reference::Projectile(name))
                            }
                            (event, _) => Event::Command(event),
                        })
                    })?))
                }
            };
            ensure!(
                result.variants.insert(name.clone(), variant).is_none(),
                "duplicate technique variant name {name}"
            );
        }
        Ok(result)
    }

    fn prepare(&self) -> Result<Member> {
        let variants = self
            .mappings
            .iter()
            .map(|(id, name)| {
                let variant = self
                    .variants
                    .get(name)
                    .with_context(|| format!("missing variant {name}"))?;
                let variant = match variant {
                    Variant::Unsupported(unavailable) => SceneAction::Unsupported {
                        reason: unavailable.unsupported_reason.clone(),
                    },
                    Variant::Prepared(attack) => {
                        SceneAction::Prepared(Box::new(map_events(*attack.clone(), |event| {
                            Ok(match event {
                                Event::Command(event) => event,
                                Event::Reference(Reference::Contact(name)) => Command::Contact(
                                    self.contacts
                                        .get(&name)
                                        .with_context(|| format!("missing contact {name}"))?
                                        .clone(),
                                ),
                                Event::Reference(Reference::Projectile(name)) => {
                                    let shot = self
                                        .projectiles
                                        .get(&name)
                                        .with_context(|| format!("missing projectile {name}"))?;
                                    Command::Projectile {
                                        member: shot.member,
                                        hit: shot.hit,
                                        feedback: shot.feedback,
                                    }
                                }
                            })
                        })?))
                    }
                };
                Ok((
                    u8::try_from(id).context("technique variant ID exceeds limit")?,
                    variant,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Member {
            variants,
            unsupported_reason: self.unsupported_reason.clone(),
        })
    }
}

#[cfg(test)]
mod tests;
