//! Prepared attacks run resolved events on the actor's gameplay clock.
use crate::{
    ActionExecution, ActionId, Battle, Cue, MeleeDefinition, MotionBinding, Sound,
    WeaponFlightDefinition,
};
use anyhow::{Result, ensure};
use std::sync::Arc;

const ATTACK_BRAKING: f32 = 0.4125;
const RECOVERY_PER_LINK: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub struct AttackPose {
    pub motion: MotionBinding,
    pub start: f32,
    pub rate: f32,
    pub blend: u8,
}

impl AttackPose {
    fn play(self, battle: &mut Battle, index: usize) {
        battle.request_pose(
            crate::ActorId(index as u8),
            Some(self.motion),
            crate::Pose {
                frame: self.start,
                rate: self.rate,
                blend: self.blend,
                ..Default::default()
            },
        );
    }
}

#[derive(Debug, Clone)]
pub enum AttackEvent {
    Contact {
        definition: Arc<MeleeDefinition>,
        /// Number of active gameplay updates, including this event's update.
        duration: u16,
    },
    Throw(Arc<WeaponFlightDefinition>),
    Projectile(Arc<crate::ProjectileDefinition>),
    Notice {
        duration: u16,
    },
    Trail {
        slot: u8,
        duration: u16,
    },
    Voice(Sound),
    Sound(Option<Sound>),
    Move {
        forward: Option<f32>,
        vertical: Option<f32>,
    },
    PassThrough(bool),
    SpecialGuard,
    Armor(u8),
    /// Wait for the ground before consuming this event or those after it.
    Land {
        pose: Option<AttackPose>,
        effect: Option<crate::EffectAppearance>,
    },
    Pose(AttackPose),
    WeaponVisibility(Vec<(u8, bool)>),
    Effect {
        appearance: crate::EffectAppearance,
        tints: [crate::effect::EffectTint; 10],
        /// Attach to the body center instead of the actor root.
        centered: bool,
    },
}

#[derive(Debug, Clone)]
pub struct PreparedAttack {
    /// Technique chaining opens at this age; ordinary combos come from the controller.
    pub chain_at: Option<u16>,
    /// Gameplay age at which recovery may begin, after the last event.
    pub end_at: u16,
    /// None keeps the actor's current pose.
    pub opening: Option<AttackPose>,
    /// Ordered by gameplay age. Hit-stop pauses the clock; animation does not.
    pub events: Vec<(u16, AttackEvent)>,
    pub recovery: u16,
}

impl PreparedAttack {
    pub fn contacts(&self) -> impl Iterator<Item = &Arc<MeleeDefinition>> {
        self.events.iter().filter_map(|(_, event)| match event {
            AttackEvent::Contact { definition, .. } => Some(definition),
            _ => None,
        })
    }

    pub fn weapon_flights(&self) -> impl Iterator<Item = &Arc<WeaponFlightDefinition>> {
        self.events.iter().filter_map(|(_, event)| match event {
            AttackEvent::Throw(definition) => Some(definition),
            _ => None,
        })
    }

    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.events.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "unordered attack events"
        );
        ensure!(
            self.chain_at.is_none_or(|at| at <= self.end_at),
            "attack chain opens after recovery"
        );
        let mut contact_end = None;
        for (at, event) in &self.events {
            match event {
                AttackEvent::Contact { duration, .. } => {
                    ensure!(*duration > 0, "empty melee window");
                    ensure!(
                        contact_end.is_none_or(|end| u32::from(*at) >= end),
                        "overlapping attack contacts"
                    );
                    contact_end = Some(u32::from(*at) + u32::from(*duration));
                }
                AttackEvent::Move {
                    forward, vertical, ..
                } => ensure!(
                    forward.is_none_or(f32::is_finite) && vertical.is_none_or(f32::is_finite),
                    "invalid attack movement"
                ),
                AttackEvent::Projectile(projectile) => projectile.validate()?,
                AttackEvent::Voice(_)
                | AttackEvent::Notice { .. }
                | AttackEvent::Trail { .. }
                | AttackEvent::Throw(_)
                | AttackEvent::Pose(_)
                | AttackEvent::WeaponVisibility(_)
                | AttackEvent::PassThrough(_)
                | AttackEvent::SpecialGuard
                | AttackEvent::Armor(_)
                | AttackEvent::Land { .. }
                | AttackEvent::Sound(_)
                | AttackEvent::Effect { .. } => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttackRun {
    Starting,
    Active { next_event: usize },
}

pub(crate) fn step(
    battle: &mut Battle,
    id: ActionId,
    sequence: &mut crate::action::Sequence,
    run: AttackRun,
    contacts: &mut crate::contact::Contacts,
    cues: &mut Vec<Cue>,
) -> Result<Option<u32>> {
    let definition = Arc::clone(&sequence.definition);
    let ActionExecution::Attack(attack) = &definition.execution else {
        unreachable!("attack lifecycle needs a prepared attack")
    };
    let actor = sequence.actor;
    let index = actor.index();
    let mut next_event = match run {
        AttackRun::Starting => {
            let movement = &mut battle.actors[index].movement;
            movement.braking = ATTACK_BRAKING;
            movement.gravity = if movement.flying {
                0.
            } else {
                crate::movement::GRAVITY
            };

            if let Some(pose) = attack.opening {
                pose.play(battle, index);
            }
            0
        }
        AttackRun::Active { next_event } => next_event,
    };
    let combo = battle.runtime[index].combo.normal_links;
    while let Some((at, event)) = attack.events.get(next_event) {
        if u32::from(*at) > sequence.age {
            break;
        }
        match event {
            AttackEvent::Contact {
                definition: contact,
                duration,
            } => {
                sequence.melee = Some(crate::melee::Window {
                    definition: Arc::clone(contact),
                    remaining: *duration,
                    struck: Vec::new(),
                });
            }
            AttackEvent::Throw(flight) => battle.throw_weapon(actor, id, Arc::clone(flight)),
            AttackEvent::Projectile(projectile) => {
                battle.emit(
                    Arc::clone(projectile),
                    id,
                    actor,
                    sequence.target,
                    battle.actors[index].position,
                )?;
            }
            AttackEvent::Notice { duration } => cues.push(Cue::Notice {
                actor,
                action: sequence.action,
                duration: *duration,
            }),
            AttackEvent::Trail { slot, duration } => {
                cues.push(Cue::WeaponTrail {
                    actor,
                    slot: *slot,
                    duration: *duration,
                });
            }
            AttackEvent::Voice(line) => {
                battle.request_voice(actor, *line, crate::VoicePriority::Action);
            }
            AttackEvent::Sound(sound) => {
                if let Some(sound) = sound {
                    cues.push(Cue::Sound {
                        actor,
                        sound: *sound,
                        position: battle.actors[index].position,
                        priority: 1,
                    });
                }
            }
            AttackEvent::Move { forward, vertical } => {
                let movement = &mut battle.actors[index].movement;
                if let Some(forward) = forward {
                    movement.forward = *forward;
                }
                if let Some(vertical) = vertical {
                    movement.vertical = *vertical;
                }
            }
            AttackEvent::PassThrough(enabled) => sequence.collision_bypass = *enabled,
            AttackEvent::Land { pose, effect } => {
                if battle.actors[index].airborne() {
                    break;
                }
                if let Some(pose) = pose {
                    pose.play(battle, index);
                }
                if let Some(appearance) = effect {
                    let owner = &battle.actors[index];
                    cues.push(Cue::Effect(crate::EffectRequest {
                        owner: actor,
                        target: actor,
                        appearance: *appearance,
                        origin: owner.position,
                        heading: owner.heading,
                        scale: owner.effect_scale,
                        follow: None,

                        tint: Default::default(),
                    }));
                }
            }
            AttackEvent::SpecialGuard => {
                battle.begin_special_guard(actor, sequence.action)?;
            }
            AttackEvent::Armor(threshold) => {
                let armor = &mut battle.actors[index].reaction.armor;
                armor.threshold = *threshold;
                armor.received = 0;
            }
            AttackEvent::Pose(pose) => pose.play(battle, index),
            AttackEvent::WeaponVisibility(slots) => {
                for &(slot, visible) in slots {
                    battle.model_requests.push(crate::ModelRequest::Weapon {
                        actor,
                        slot,
                        visible,
                    });
                }
            }
            AttackEvent::Effect {
                appearance,
                tints,
                centered,
            } => {
                let element = crate::damage::resolve_element(
                    crate::HitElement::Inherited,
                    &battle.actors[index],
                )
                .map_or(0, |element| element as usize + 1);
                cues.push(Cue::Effect(crate::EffectRequest {
                    owner: actor,
                    target: actor,
                    appearance: *appearance,
                    origin: if *centered {
                        battle.actors[index].effect_origin()
                    } else {
                        battle.actors[index].position
                    },
                    heading: battle.actors[index].heading,
                    follow: Some(if *centered {
                        crate::EffectFollow::Center(actor)
                    } else {
                        crate::EffectFollow::Actor(actor)
                    }),
                    scale: 1.,
                    tint: tints[element],
                }));
            }
        }
        next_event += 1;
    }
    sequence.execution = crate::action::Execution::Attack(AttackRun::Active { next_event });
    if let Some(window) = &mut sequence.melee {
        if let Some(slot) = window.definition.trail {
            cues.push(Cue::WeaponTrail {
                actor,
                slot,
                duration: 1,
            });
        }
        contacts.melee(actor, id, &window.definition, &window.struck)?;
        window.remaining -= 1;
        if window.remaining == 0 {
            sequence.melee = None;
        }
    }
    // Trailing feedback cannot extend gameplay; pending contacts and landing still can.
    if sequence.age >= u32::from(attack.end_at)
        && sequence.melee.is_none()
        && attack.events[next_event..].iter().all(|(_, event)| {
            matches!(
                event,
                AttackEvent::Notice { .. }
                    | AttackEvent::Trail { .. }
                    | AttackEvent::Voice(_)
                    | AttackEvent::Sound(_)
                    | AttackEvent::Pose(_)
                    | AttackEvent::WeaponVisibility(_)
                    | AttackEvent::Effect { .. }
            )
        })
    {
        let combo_recovery = if sequence.normal.is_some() && !battle.actors[index].airborne() {
            u32::from(combo) * RECOVERY_PER_LINK
        } else {
            0
        };
        return Ok(Some(u32::from(attack.recovery) + combo_recovery));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorId, ActorSetup, BattleInput, HitCondition, Side, conditions::Condition};

    #[test]
    fn landing_and_contacts_delay_recovery_but_trailing_feedback_does_not() {
        let mut owner = crate::tests::actor(Side::Party);
        owner.position[1] = 2000.;

        let mut target = crate::tests::actor(Side::Enemy);
        target.position[0] = 1000.;
        target.body.collider = Some(crate::Collider::sphere(1.));
        let contact = Arc::new(MeleeDefinition {
            hit: crate::HitRule {
                kind: crate::DamageKind::Slash,
                arte: true,
                overlimit_pause: false,
                power: crate::Power::Fixed(1),
                element: crate::HitElement::Neutral,
                prevents_defeat: false,
                guard: Default::default(),
                reaction: Default::default(),
                condition: None,
            },
            trail: None,
            volume: crate::MeleeVolume {
                offset: [0.; 3],
                radius: 5.,
                half_height: 5.,
            },
        });
        let mut prepared = crate::PreparedBattle::new(
            vec![(owner, Default::default()), (target, Default::default())],
            (vec![crate::ActionDefinition {
                normal: None,
                tp_cost: 8,
                execution: ActionExecution::Attack(PreparedAttack {
                    opening: None,
                    chain_at: None,
                    end_at: 0,
                    recovery: 3,
                    events: vec![
                        (
                            0,
                            AttackEvent::Land {
                                pose: None,
                                effect: None,
                            },
                        ),
                        (
                            0,
                            AttackEvent::Contact {
                                definition: contact,
                                duration: 3,
                            },
                        ),
                        (u16::MAX, AttackEvent::Sound(Some(Sound::Cue(1)))),
                    ],
                }),
            }])
            .into(),
            1,
        )
        .unwrap();
        crate::tests::assign_action(&mut prepared, 0, crate::ActionKey(0));
        prepared.resources.actor_setup[0].techniques[0]
            .capabilities
            .aerial = true;
        prepared.actors[0].equipment.combo_traits.aerial_arte = true;
        let mut battle = prepared.finish().unwrap();
        let first = battle
            .step(BattleInput {
                actions: vec![crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(0),
                    action: crate::ActionKey(0),
                }],
                ..Default::default()
            })
            .unwrap();
        let action = first.actions[0].0;
        for _ in 0..10 {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
            assert_eq!(battle.actors[1].hp, 50);
        }
        battle.actors[0].position[1] = 0.;
        battle
            .step(BattleInput {
                paused: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
        for _ in 0..2 {
            battle.step(BattleInput::default()).unwrap();
            assert_eq!(battle.activity(ActorId(0)), crate::Activity::Action);
            assert_eq!(battle.actors[1].hp, 50);
        }
        // Landing may happen after end_at, but the full strike window still runs.
        battle.actors[1].position = [0.; 3];
        battle.step(BattleInput::default()).unwrap();
        assert_eq!(battle.actors[1].hp, 49);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Recovering);
        let mut completions = 0;
        for _ in 0..10 {
            let frame = battle.step(BattleInput::default()).unwrap();
            completions += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Completed { action: id } if *id == action))
                .count();
        }
        assert_eq!(completions, 1);
        assert_eq!(battle.actors[1].hp, 49);
        assert_eq!(battle.actors[0].tp, 32);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
    }

    #[test]
    fn late_contact_applies_the_condition_to_the_hit_target_and_recovers_once() {
        let owner = crate::tests::actor(Side::Party);

        let mut enemy = crate::tests::actor(Side::Enemy);
        enemy.position[0] = 1000.;
        enemy.body.collider = Some(crate::Collider::sphere(1.));
        enemy.equipment.luck = 0;
        let hit = crate::HitRule {
            kind: crate::DamageKind::Slash,
            arte: true,
            overlimit_pause: false,
            power: crate::Power::Fixed(1),
            element: crate::HitElement::Neutral,
            prevents_defeat: false,
            guard: Default::default(),
            reaction: Default::default(),
            condition: Some(HitCondition {
                condition: resonance_content::battle_action::Condition::DefenseDown,
                chance: 100,
                value: -10,
            }),
        };
        let attack = PreparedAttack {
            chain_at: None,
            opening: None,
            end_at: 24,
            recovery: 2,
            events: vec![(
                8,
                AttackEvent::Contact {
                    definition: Arc::new(MeleeDefinition {
                        hit,
                        trail: None,
                        volume: crate::MeleeVolume {
                            offset: [0.; 3],
                            radius: 5.,
                            half_height: 5.,
                        },
                    }),
                    duration: 12,
                },
            )],
        };
        let mut battle = crate::PreparedBattle::new(
            vec![
                (
                    owner,
                    ActorSetup {
                        techniques: vec![crate::tests::technique(crate::ActionKey(0), 2)],
                        ..Default::default()
                    },
                ),
                (enemy.clone(), ActorSetup::default()),
                (enemy, ActorSetup::default()),
            ],
            (vec![crate::ActionDefinition {
                normal: None,
                tp_cost: 4,
                execution: ActionExecution::Attack(attack),
            }])
            .into(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        battle
            .step(BattleInput {
                actions: vec![crate::ActionRequest {
                    actor: ActorId(0),
                    target: ActorId(2),
                    action: crate::ActionKey(0),
                }],
                ..Default::default()
            })
            .unwrap();
        for _ in 0..17 {
            battle.step(BattleInput::default()).unwrap();
        }
        assert!(battle.actors.iter().all(|actor| {
            !actor
                .conditions
                .effective()
                .contains(Condition::DefenseDown)
        }));
        // A different enemy enters the active hit volume after the windup.
        battle.actors[1].position = [0.; 3];

        let contact = battle.step(BattleInput::default()).unwrap();
        assert!(contact.cues.iter().any(|cue| matches!(
            cue,
            Cue::Hit {
                actor: ActorId(1),
                ..
            }
        )));
        assert_eq!(
            battle.actors[1]
                .conditions
                .magnitude(Condition::DefenseDown),
            -10
        );
        assert!(
            !battle.actors[2]
                .conditions
                .effective()
                .contains(Condition::DefenseDown)
        );
        let duration = battle.actors[1]
            .conditions
            .remaining(Condition::DefenseDown)
            .unwrap();
        let mut completions = 0;
        for _ in 0..40 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert!(!frame.cues.iter().any(|cue| matches!(cue, Cue::Hit { .. })));
            completions += frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Completed { .. }))
                .count();
        }
        assert_eq!(completions, 1);
        assert_eq!(battle.actors[0].tp, 36);
        assert_eq!(battle.activity(ActorId(0)), crate::Activity::Idle);
        assert!(
            battle.actors[1]
                .conditions
                .remaining(Condition::DefenseDown)
                .unwrap()
                < duration
        );
    }
}
