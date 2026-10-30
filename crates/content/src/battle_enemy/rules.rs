//! Enemy-local definitions: edit an attack, contact or projectile once.
use super::*;
use crate::battle_action::{self as action, EnemyActions, EnemyAttack, EnemyEvent, EnemyExecution};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub null_motions: BTreeSet<u16>,
    pub profile: Profile,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weapon_visibility: BTreeMap<u8, WeaponVisibility>,
    pub actions: EnemyActions<EnemyExecution<String>>,
    pub attacks: BTreeMap<String, EnemyAttack<Event>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projectiles: BTreeMap<String, Shot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contacts: BTreeMap<String, action::MeleeContact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_strategy: Option<TargetPolicy>,
    pub entry_row: EntryRow,
    #[serde(default, skip_serializing_if = "crate::defaults::is_default")]
    pub guard_recovery_bonus: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    pub definition: crate::battle_projectile::Projectile,
    pub hit: action::ActionHit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<crate::battle_martial::ProjectileFeedback>,
}

/// Small commands remain inline. Large reusable definitions use local names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Event {
    Reference(Reference),
    Command(EnemyEvent),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    Projectile(String),
    Contact(String),
    ContinueContact(String),
    GroundProjectiles {
        projectiles: [String; 2],
        retreat: f32,
    },
}

fn intern<T: PartialEq>(definitions: &mut BTreeMap<String, T>, kind: &str, value: T) -> String {
    // Equality is local to this enemy during cooking. No hashes or runtime scan.
    for (name, previous) in definitions.iter() {
        if previous == &value {
            return name.clone();
        }
    }
    let name = format!("{kind}-{}", definitions.len() + 1);
    definitions.insert(name.clone(), value);
    name
}

fn map_attack<A, B>(
    attack: EnemyAttack<A>,
    mut f: impl FnMut(A) -> Result<B>,
) -> Result<EnemyAttack<B>> {
    Ok(EnemyAttack {
        duration: attack.duration,
        chain_at: attack.chain_at,
        recovery_ticks: attack.recovery_ticks,
        recovery_pose: attack.recovery_pose,
        armor: attack.armor,
        vulnerable: attack.vulnerable,
        contact_recovery: attack.contact_recovery,
        events: attack
            .events
            .into_iter()
            .map(|(at, event)| Ok((at, f(event)?)))
            .collect::<Result<_>>()?,
    })
}

impl TryFrom<Definition> for Rules {
    type Error = anyhow::Error;
    fn try_from(value: Definition) -> Result<Self> {
        let mut result = Self {
            null_motions: value.null_motions,
            profile: value.profile,
            weapon_visibility: value.weapon_visibility,
            actions: EnemyActions {
                rows: vec![],
                policy: value.actions.policy,
            },
            attacks: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            contacts: BTreeMap::new(),
            target_strategy: value.target_strategy,
            entry_row: value.entry_row,
            guard_recovery_bonus: value.guard_recovery_bonus,
            presentation: value.presentation,
        };
        for row in value.actions.rows {
            let row = row.try_map_execution(|execution| {
                Ok::<_, anyhow::Error>(match execution {
                    EnemyExecution::Attack(attack) => {
                        EnemyExecution::Attack(result.store_attack(attack)?)
                    }
                    EnemyExecution::Physical { technique, attack } => EnemyExecution::Physical {
                        technique,
                        attack: result.store_attack(attack)?,
                    },
                    EnemyExecution::Spell(id) => EnemyExecution::Spell(id),
                    EnemyExecution::LeaveArena => EnemyExecution::LeaveArena,
                })
            })?;
            result.actions.rows.push(row);
        }
        Ok(result)
    }
}

impl Rules {
    fn store_attack(&mut self, attack: EnemyAttack) -> Result<String> {
        let attack = map_attack(attack, |event| {
            Ok(match event {
                EnemyEvent::Projectile {
                    definition,
                    hit,
                    feedback,
                } => Event::Reference(Reference::Projectile(intern(
                    &mut self.projectiles,
                    "projectile",
                    Shot {
                        definition: *definition,
                        hit,
                        feedback,
                    },
                ))),
                EnemyEvent::GroundProjectiles {
                    definitions,
                    hit,
                    retreat,
                } => {
                    let [first, second] = *definitions;
                    let mut name = |definition| {
                        intern(
                            &mut self.projectiles,
                            "projectile",
                            Shot {
                                definition,
                                hit,
                                feedback: None,
                            },
                        )
                    };
                    Event::Reference(Reference::GroundProjectiles {
                        projectiles: [name(first), name(second)],
                        retreat,
                    })
                }
                EnemyEvent::Contact(contact) => Event::Reference(Reference::Contact(intern(
                    &mut self.contacts,
                    "contact",
                    contact,
                ))),
                EnemyEvent::ContinueContact(contact) => Event::Reference(
                    Reference::ContinueContact(intern(&mut self.contacts, "contact", contact)),
                ),
                event => Event::Command(event),
            })
        })?;
        Ok(intern(&mut self.attacks, "attack", attack))
    }

    fn attack(&self, name: &str) -> Result<EnemyAttack> {
        let attack = self
            .attacks
            .get(name)
            .with_context(|| format!("unknown enemy attack {name}"))?
            .clone();
        map_attack(attack, |event| {
            Ok(match event {
                Event::Reference(Reference::Projectile(name)) => {
                    let shot = self.shot(&name)?;
                    EnemyEvent::Projectile {
                        definition: Box::new(shot.definition),
                        hit: shot.hit,
                        feedback: shot.feedback,
                    }
                }
                Event::Reference(Reference::GroundProjectiles {
                    projectiles: [first, second],
                    retreat,
                }) => {
                    let first = self.shot(&first)?;
                    let second = self.shot(&second)?;
                    ensure!(
                        first.hit == second.hit
                            && first.feedback.is_none()
                            && second.feedback.is_none(),
                        "paired ground projectiles must share hit settings and have no martial feedback"
                    );
                    EnemyEvent::GroundProjectiles {
                        definitions: Box::new([first.definition, second.definition]),
                        hit: first.hit,
                        retreat,
                    }
                }
                Event::Reference(Reference::Contact(name)) => {
                    EnemyEvent::Contact(self.contact(&name)?)
                }
                Event::Reference(Reference::ContinueContact(name)) => {
                    EnemyEvent::ContinueContact(self.contact(&name)?)
                }
                Event::Command(
                    EnemyEvent::Projectile { .. }
                    | EnemyEvent::GroundProjectiles { .. }
                    | EnemyEvent::Contact(_)
                    | EnemyEvent::ContinueContact(_),
                ) => {
                    anyhow::bail!("enemy contacts and projectiles must use local definition names")
                }
                Event::Command(command) => command,
            })
        })
    }

    fn shot(&self, name: &str) -> Result<Shot> {
        self.projectiles
            .get(name)
            .cloned()
            .with_context(|| format!("unknown enemy projectile {name}"))
    }

    fn contact(&self, name: &str) -> Result<action::MeleeContact> {
        self.contacts
            .get(name)
            .cloned()
            .with_context(|| format!("unknown enemy contact {name}"))
    }

    pub fn into_definition(self) -> Result<Definition> {
        let rows = self
            .actions
            .rows
            .iter()
            .cloned()
            .map(|row| {
                row.try_map_execution(|execution| {
                    Ok::<_, anyhow::Error>(match execution {
                        EnemyExecution::Attack(name) => EnemyExecution::Attack(self.attack(&name)?),
                        EnemyExecution::Physical { technique, attack } => {
                            EnemyExecution::Physical {
                                technique,
                                attack: self.attack(&attack)?,
                            }
                        }
                        EnemyExecution::Spell(id) => EnemyExecution::Spell(id),
                        EnemyExecution::LeaveArena => EnemyExecution::LeaveArena,
                    })
                })
            })
            .collect::<Result<_>>()?;
        Ok(Definition {
            null_motions: self.null_motions,
            profile: self.profile,
            weapon_visibility: self.weapon_visibility,
            actions: EnemyActions {
                rows,
                policy: self.actions.policy,
            },
            target_strategy: self.target_strategy,
            entry_row: self.entry_row,
            guard_recovery_bonus: self.guard_recovery_bonus,
            presentation: self.presentation,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn fixture() -> Result<Rules> {
        let pose = json!({"clip": "walk", "rate": 1.0, "blend": 0});
        let choice = json!({"weight": 100, "requirements": {"difficulty": {"start":0,"end":2}},
            "range":[0,100], "approach_range":100, "guard_chance":0,
            "execution":{"attack":"spit"}});
        serde_json::from_value(json!({
            "entry_row":"random",
            "profile": {
                "walk_speed":1.0,"run_speed":2.0,"turn_ticks":1,"idle_ticks":1,"idle_variation":0,
                "recoil_weight":"light","species":"unknown","stagger_threshold":1,"stagger_ticks":1,"guard_reduction":0,
                "guard_pressure_limit":1,"armor":0,"center_offset":[0.0,0.0,0.0],
                "target_anchor":"center", "head_anchor":{"bone":"root"},
                "shadow":null,"size":"small","death_motion":"idle","overlimit_bone":"root",
                "casting":{"base_ticks":1,"opening_clip":"walk","opening_rate":1.0,"opening_blend":0,
                    "release_clip":"walk","release_rate":1.0,"release_blend":0,"release_start":0.0}
            },
            "actions":{"rows":[choice,choice], "policy":{}},
            "attacks":{"spit":{"duration":20,"recovery_ticks":1,"recovery_pose":pose,"armor":0,
                "events":[[2,{"projectile":"seed"}],[5,{"projectile":"seed"}]]}},
            "projectiles":{"seed":{
                "definition":{"velocity":[0.0,0.0,1.0],"motion":{},
                    "contact":{"shape":{"kind":"box"},"recoil_direction":"travel","radius":1.0,"height":1.0},
                    "birth_effect":{"bank":0,"member":0},"ground_effect":{"bank":0,"member":0},
                    "trail_effect":{"bank":0,"member":0}},
                "hit":{"kind":"slash","power":"normal","element":"inherited","guard_pressure":1,
                    "hitstun":1,"cooldown":1,"stun_chance":0,"stagger":0,"recoil":1}
            }}
        }))
        .map_err(Into::into)
    }

    #[test]
    fn one_projectile_edit_reaches_every_attack_use_and_roundtrips() -> Result<()> {
        let mut rules = fixture()?;
        rules
            .projectiles
            .get_mut("seed")
            .unwrap()
            .definition
            .velocity[2] = 42.;
        let definition = rules.into_definition()?;
        for row in &definition.actions.rows {
            let Some(EnemyExecution::Attack(attack)) = &row.execution else {
                panic!("missing attack")
            };
            for (_, event) in &attack.events {
                let EnemyEvent::Projectile { definition, .. } = event else {
                    panic!("missing projectile")
                };
                assert_eq!(definition.velocity[2], 42.);
            }
        }
        let expected = serde_json::to_value(&definition)?;
        let cooked = Rules::try_from(definition)?;
        assert_eq!((cooked.attacks.len(), cooked.projectiles.len()), (1, 1));
        let bytes = serde_json::to_vec(&cooked)?;
        let decoded: Rules = serde_json::from_slice(&bytes)?;
        assert_eq!(serde_json::to_value(decoded.into_definition()?)?, expected);
        Ok(())
    }

    #[test]
    fn undefined_names_and_inline_projectile_copies_are_rejected() -> Result<()> {
        let mut rules = fixture()?;
        rules.projectiles.clear();
        assert!(
            rules
                .into_definition()
                .unwrap_err()
                .to_string()
                .contains("unknown enemy projectile seed")
        );
        let mut rules = fixture()?;
        let shot = rules.projectiles["seed"].clone();
        rules.attacks.get_mut("spit").unwrap().events[0].1 =
            Event::Command(EnemyEvent::Projectile {
                definition: Box::new(shot.definition),
                hit: shot.hit,
                feedback: None,
            });
        assert!(
            rules
                .into_definition()
                .unwrap_err()
                .to_string()
                .contains("local definition names")
        );
        Ok(())
    }
}
