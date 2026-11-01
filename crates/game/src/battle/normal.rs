//! Bind authored normal attacks to the actor's model and encounter resources.
use super::{ActionDefinition, voice::Sound};
use anyhow::{Context, Result};
use resonance_battle::WeaponSlot;
use resonance_battle::{AttackEvent, NormalAttack};
use resonance_content::battle_normal::{Attack, Event, WeaponFlight};
use std::sync::Arc;

pub(super) fn controls(
    attacks: &[Attack; 7],
    actions: [resonance_battle::ActionKey; 7],
) -> Result<[resonance_battle::NormalControl; 7]> {
    let mut fallbacks = [None; 7];
    for (fallback, attack) in fallbacks.iter_mut().zip(attacks) {
        *fallback = attack
            .continuation
            .fallback
            .map(|index| {
                NormalAttack::ALL
                    .get(usize::from(index))
                    .copied()
                    .context("invalid normal fallback")
            })
            .transpose()?;
    }
    Ok(std::array::from_fn(|index| {
        resonance_battle::NormalControl {
            action: actions[index],
            continuation: resonance_battle::NormalContinuation {
                directions: attacks[index].continuation.directions,
                fallback: fallbacks[index],
            },
            reach: f32::from(attacks[index].reach[0]),
            minimum_reach: f32::from(attacks[index].reach[1]),
        }
    }))
}

/// Prepared actor inputs shared by encounter and focused action preparation.
pub struct Resources<'a> {
    pub character: super::party::Character,
    pub model: Option<&'a resonance_battle::ModelDefinition>,
    pub common: Option<&'a super::EffectResource>,
    pub tints: Option<&'a resonance_content::battle_effect::Tints>,
    pub recoil: &'a super::recoil::Parameters,
}

pub fn prepare_resources(
    resolver: &super::voice::Resolver<'_>,
    actor: &Resources<'_>,
    resources: &mut super::ActionResources,
    sound: &mut impl FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
) -> Result<Vec<ActionDefinition>> {
    let index = actor.character.index();
    let attacks = resolver
        .party_table()
        .normal_attacks
        .get(index)
        .context("missing normal attack table")?;
    let mut bindings = Vec::with_capacity(attacks.len());
    for (index, attack) in attacks.iter().enumerate() {
        let mut events = Vec::new();
        if attack
            .events
            .iter()
            .any(|(_, event)| matches!(event, Event::WeaponVisibility { .. }))
        {
            let model = actor.model.context("missing normal attack model")?;
            events.push((
                0,
                AttackEvent::WeaponVisibility(
                    model
                        .weapons
                        .iter()
                        .map(|weapon| (weapon.slot, true))
                        .collect(),
                ),
            ));
        }
        for (at, event) in &attack.events {
            let event = match event {
                Event::Pose(source) => {
                    let Some(pose) = super::model::action_pose(actor.model, *source) else {
                        continue;
                    };
                    AttackEvent::Pose(pose)
                }
                Event::Contact(contact) => AttackEvent::Contact {
                    definition: Arc::new(resonance_battle::MeleeDefinition {
                        hit: super::hit::authored(&contact.hit, actor.recoil)?,
                        volume: resonance_battle::MeleeVolume::Attached {
                            anchors: actor
                                .model
                                .context("missing normal melee model")?
                                .contact_anchors(&contact.attachments)?,
                            shape: contact.shape,
                            radius: contact.radius,
                            half_height: contact.height,
                        },
                        trail: None,
                    }),
                    duration: contact.duration,
                },
                Event::Projectile { .. } => {
                    anyhow::bail!("normal projectile recipe is not prepared")
                }
                Event::Throw {
                    slot,
                    flight: profile,
                    radius,
                    hit,
                } => {
                    let mut hit = super::hit::authored(hit, actor.recoil)?;
                    hit.overlimit_pause = false;
                    AttackEvent::Throw(Arc::new(flight(*slot, *profile, *radius, hit)))
                }
                Event::Sound(cue) => AttackEvent::Sound(sound(Sound::Cue(*cue))?),
                Event::Voice(voice) => {
                    let Some(line) = resolver.absolute(
                        super::model::ModelSource::Party(actor.character),
                        *voice,
                        &mut *sound,
                    )?
                    else {
                        continue;
                    };
                    AttackEvent::Voice {
                        sound: line,
                        priority: resonance_battle::VoicePriority::Action,
                    }
                }
                Event::Effect(member) => {
                    let common = actor.common.context("missing normal attack effect bank")?;
                    let appearance = resources.effect_member(common, *member)?;
                    AttackEvent::Effect {
                        centered: false,
                        appearance,
                        tints: std::array::from_fn(|element| {
                            actor
                                .tints
                                .and_then(|tints| tints.effect(element))
                                .unwrap_or_default()
                        }),
                    }
                }
                other => crate::battle::normal::movement_event(other)?,
            };
            events.push((*at, event));
        }
        events.sort_by_key(|(at, _)| *at);
        bindings.push(ActionDefinition {
            normal: Some(NormalAttack::ALL[index]),
            execution: super::ActionExecution::Attack(resonance_battle::PreparedAttack {
                contact_recovery: None,
                opening: None,
                events,
                chain_at: attack.chain_at,
                chain_retry_at: attack.chain_retry_at,
                end_at: attack.duration,
                recovery: attack.recovery,
            }),
            tp_cost: 0,
        });
    }
    Ok(bindings)
}

fn flight(
    slot: WeaponSlot,
    profile: WeaponFlight,
    radius: f32,
    hit: resonance_battle::HitRule,
) -> resonance_battle::WeaponFlightDefinition {
    resonance_battle::WeaponFlightDefinition {
        slot,
        origin: [0., 70., 25.],
        outbound_ticks: u32::from(profile.outbound_ticks),
        speed: profile.speed,
        return_speed: profile.return_speed,
        direction_y: profile.direction_y,
        hit,
        radius,
    }
}

/// Movement and visibility commands have the same meaning for normal attacks,
/// martial artes, and combined Unison attacks.
pub(super) fn movement_event(event: &Event) -> Result<AttackEvent> {
    Ok(match event {
        Event::Braking(value) => AttackEvent::Braking(*value),
        Event::Gravity(value) => AttackEvent::Gravity(*value),
        Event::AnimationRate(value) => AttackEvent::AnimationRate(*value),
        Event::CameraDistance { distance, duration } => AttackEvent::CameraDistance {
            distance: *distance,
            duration: *duration,
        },
        Event::Turn(value) => AttackEvent::Turn(*value),
        Event::TraverseTarget {
            duration,
            clearance,
        } => AttackEvent::TraverseTarget {
            duration: *duration,
            clearance: *clearance,
        },
        Event::ReactionArmor(duration) => AttackEvent::ReactionArmor(*duration),
        Event::SecondaryWind { enabled, duration } => AttackEvent::SecondaryWind {
            enabled: *enabled,
            duration: *duration,
        },
        Event::PassThrough(value) => AttackEvent::PassThrough {
            enabled: *value,
            duration: 0,
        },
        Event::Forward(value) => AttackEvent::Move {
            forward: Some(*value),
            vertical: None,
        },
        Event::Vertical(value) => AttackEvent::Move {
            forward: None,
            vertical: Some(*value),
        },
        Event::WeaponVisibility { slot, visible } => {
            AttackEvent::WeaponVisibility(vec![(Some(*slot), *visible)])
        }
        Event::Trail { slot, duration } => AttackEvent::Trail {
            slot: *slot,
            duration: *duration,
        },
        Event::Pose(_)
        | Event::Contact(_)
        | Event::Projectile { .. }
        | Event::Throw { .. }
        | Event::Sound(_)
        | Event::Voice(_)
        | Event::Effect(_) => {
            anyhow::bail!("attack event requires actor resources")
        }
    })
}
