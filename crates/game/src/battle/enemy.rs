//! Prepare native enemy actions and hit volumes.

use super::ActionDefinition;
use anyhow::{Context, Result};
use resonance_content::{battle_enemy, monster::MonsterStats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyStatistics {
    pub hp: i32,
    pub initial_hp: i32,
    pub tp: u16,
    pub initial_tp: u16,
    pub attack: i32,
    pub thrust: i32,
    pub defense: i32,
    pub intelligence: i32,
    pub accuracy: i32,
    pub evasion: i32,
    pub luck: u16,
    pub level: u8,
}

/// Difficulty increases health, TP and offensive statistics. Imported storage
/// widths never limit combat arithmetic; only the actor's vital bounds apply.
pub fn statistics(source: &MonsterStats, difficulty: u8) -> Result<EnemyStatistics> {
    anyhow::ensure!(difficulty <= 2, "invalid battle difficulty {difficulty}");
    let vital_numerator = u64::from(difficulty) + 2;
    let combat_numerator = i32::from(difficulty) + 4;
    let hp = |value: u32| (u64::from(value) * vital_numerator / 2).min(i32::MAX as u64) as i32;
    let tp = |value: u16| (u64::from(value) * vital_numerator / 2).min(u64::from(u16::MAX)) as u16;
    let combat = |value: i32| value.max(0) * combat_numerator / 4;
    let max_hp = hp(source.hp);
    let max_tp = tp(source.tp);
    Ok(EnemyStatistics {
        hp: max_hp,
        initial_hp: if source.initial_hp == 0 {
            max_hp
        } else {
            hp(source.initial_hp).min(max_hp)
        },
        tp: max_tp,
        initial_tp: if source.initial_tp == 0 {
            max_tp
        } else {
            tp(source.initial_tp).min(max_tp)
        },
        attack: combat(i32::from(source.attack)),
        thrust: combat(i32::from(source.thrust)),
        defense: i32::from(source.defense),
        intelligence: combat(i32::from(source.intelligence)),
        accuracy: combat(i32::from(source.accuracy)),
        evasion: combat(i32::from(source.evasion)),
        luck: u16::from(source.luck),
        level: source.level,
    })
}

/// Construct gameplay state without loading a body, motion, or attachment.
pub(super) fn actor(
    monster: &resonance_content::monster::Monster,
    profile: &resonance_content::battle_profile::Profile,
    guard_recovery_bonus: u8,
    stats: EnemyStatistics,
    recoil: &super::recoil::Parameters,
) -> Result<resonance_battle::Actor> {
    use resonance_battle::{Actor, Affinity, Body, Collider, CombatStats, Control, Side};
    let mut actor = Actor {
        side: Side::Enemy,
        species: 0,
        equipment: resonance_battle::EquipmentAttributes {
            max_hp: stats.hp,
            max_tp: stats.tp,
            tp_cost_reduction: false,
            quick_escape: false,
            taunt_enabled: false,
            taunt_guard: false,
            taunt_cancel: false,
            control_ex: Default::default(),
            quick_turn: false,
            backstep_guard: false,
            casting: Default::default(),
            dagger_reach: false,
            contact: Default::default(),
            normal_combo_limit: 1,
            luck: stats.luck,
            stats: CombatStats {
                slash: stats.attack,
                thrust: stats.thrust,
                defense: stats.defense,
                intelligence: stats.intelligence,
                accuracy: stats.accuracy,
                evasion: stats.evasion,
                level: stats.level,
            },
            // Leave unrecognized direction modes unchanged.
            affinities: monster.affinities.map(|code| match code {
                1 => Affinity::Weak,
                2 => Affinity::Resistant,
                3 => Affinity::Absorb,
                4 => Affinity::Immune,
                _ => Affinity::Normal,
            }),
            damage: Default::default(),
            recovery: Default::default(),
            base_element: monster.attack_element,
            combo_traits: Default::default(),
            normal_guard: false,
            speed_multiplier: 1.,
            reaction_ex: Default::default(),
            stun_ex_bonus: false,
            spell_revenge: false,
        },
        control: Control::Enemy,
        availability: Default::default(),
        overlimit: resonance_battle::OverLimit::new(
            u16::try_from(profile.initial_overlimit)
                .context("negative initial Over Limit charge")?,
        )?,
        proficiency: 0,
        input: Default::default(),
        guard: resonance_battle::Guard {
            recovery_bonus: guard_recovery_bonus,
            ..Default::default()
        },
        hp: stats.initial_hp,
        tp: stats.initial_tp,
        control_ex_state: Default::default(),
        casting_state: Default::default(),
        stored_spell: None,
        control_slot: 0,
        elements: Default::default(),
        attack_power: 100,
        conditions: Default::default(),
        position: [0.; 3],
        heading: 0.,
        facing_direction: [0., 0., 1.],
        effect_scale: 1.,
        body: Body {
            collider: Some(match monster.id {
                SEWER_RAT => Collider::standing(35., 70.),
                PUMPKIN_TREE => Collider::standing(65., 190.),
                _ => Collider::standing(35., 160.),
            }),
            ..Default::default()
        },
        // Prepare braking before an enemy can time out during approach.
        movement: resonance_battle::Movement {
            braking: 0.55,
            ..Default::default()
        },
        reaction: Default::default(),
        hit_stop: 0,
        time_stop: 0,
    };
    super::profile::apply(recoil, profile, &mut actor)?;
    actor.position[1] = profile.ground_offset;
    Ok(actor)
}

pub const PUMPKIN_TREE: u8 = 12;
pub const SEWER_RAT: u8 = 34;
pub const ZOMBIE: u8 = 36;
pub const GHOUL: u8 = 37;
pub const GHOST: u8 = 49;
pub const PHANTOM: u8 = 50;

impl<S: FnMut(super::voice::Sound) -> Result<Option<resonance_battle::Sound>>>
    super::encounter::resources::Resources<'_, S>
{
    /// Native attacks and spells share one list and the encounter's resource owner.
    pub(crate) fn enemy(
        &mut self,
        enemy: u8,
        model: Option<&resonance_battle::ModelDefinition>,
        source: &battle_enemy::Definition,
        bindings: &mut resonance_battle::ActionDefinitions,
    ) -> Result<Vec<resonance_battle::ActionKey>> {
        use resonance_battle::{AttackEvent as Event, AttackPose, PreparedAttack};
        use resonance_content::battle_action::EnemyAttack::*;
        use std::sync::Arc;
        let model_source = super::model::ModelSource::Enemy(enemy);
        let pose = |clip| {
            super::model::motion(model, clip).map(|motion| AttackPose {
                motion,
                start: 0.,
                rate: 0.5,
                blend: 4,
            })
        };
        let movement = |forward, vertical| Event::Move {
            forward: Some(forward),
            vertical,
        };
        let mut actions = Vec::new();
        for metadata in &source.actions.rows {
            let attack = metadata
                .attack
                .context("enemy action has no native definition")?;
            if attack == FireBall {
                let (cast, feedback) = self.casting(
                    model_source,
                    super::fire_ball::CATALOGUE,
                    super::fire_ball::CATALOGUE,
                )?;
                let key = bindings.insert(ActionDefinition {
                    normal: None,
                    tp_cost: u16::from(metadata.tp),
                    execution: super::ActionExecution::Casting(cast),
                });
                self.feedback.casting.insert(key, feedback);
                actions.push(key);
                continue;
            }
            let mut hit = super::hit::physical(match attack {
                Double => 75,
                Triple => 60,
                Cross => 130,
                Pounce => 140,
                Swing | Spit | Lob => 90,
                Scatter => 50,
                _ => 100,
            });
            hit.reaction.recoil.knock_down = matches!(attack, Pounce | Cross);
            hit.reaction.hits_down = hit.reaction.recoil.knock_down;
            if attack == Spit {
                hit.kind = resonance_battle::DamageKind::Magic;
            }
            let contact = |at, duration| {
                (
                    at,
                    Event::Contact {
                        definition: Arc::new(resonance_battle::MeleeDefinition {
                            hit,
                            volume: match enemy {
                                SEWER_RAT => resonance_battle::MeleeVolume {
                                    offset: [0., 25., 50.],
                                    radius: 45.,
                                    half_height: 40.,
                                },
                                PUMPKIN_TREE => resonance_battle::MeleeVolume {
                                    offset: [0., 95., 100.],
                                    radius: 100.,
                                    half_height: 95.,
                                },
                                _ => resonance_battle::MeleeVolume {
                                    offset: [0., 70., 70.],
                                    radius: 60.,
                                    half_height: 60.,
                                },
                            },
                            trail: Some(0),
                        }),
                        duration,
                    },
                )
            };
            let mut events = Vec::new();
            if enemy == GHOUL {
                // Heavy swipes withstand three armor points before a later hit can interrupt.
                events.push((0, Event::Armor(3)));
            }
            let (opening, end_at) = match attack {
                Right | Push | Counter => {
                    let opening = match attack {
                        Push => 33,
                        Counter => 38,
                        _ => 30,
                    };
                    let swing = if matches!(attack, Push) { 34 } else { 31 };
                    if let Some(pose) = pose(swing) {
                        events.push((12, Event::Pose(pose)));
                    }
                    events.push(contact(16, 8));
                    events.push((
                        16,
                        Event::Sound((self.sound)(super::voice::Sound::Cue(60))?),
                    ));
                    (Some(opening), 32)
                }
                Double | Triple | Cross => {
                    let strikes: &[u16] = if matches!(attack, Triple) {
                        &[16, 40, 64]
                    } else {
                        &[16, 40]
                    };
                    for (hit, &at) in strikes.iter().enumerate() {
                        if !matches!(attack, Cross)
                            && let Some(pose) = pose(if hit == 1 { 34 } else { 31 })
                        {
                            events.push((at - 4, Event::Pose(pose)));
                        }
                        events.push(contact(at, 8));
                        events.push((at, movement(4., None)));
                        events.push((
                            at,
                            Event::Sound((self.sound)(super::voice::Sound::Cue(60))?),
                        ));
                    }
                    (
                        Some(if matches!(attack, Cross) { 37 } else { 30 }),
                        strikes.last().unwrap() + 16,
                    )
                }
                Tail => {
                    events.push(contact(16, 8));
                    (Some(30), 32)
                }
                Pounce => {
                    events.push((4, movement(12., Some(10.))));
                    if let Some(pose) = pose(32) {
                        events.push((12, Event::Pose(pose)));
                    }
                    events.push(contact(12, 20));
                    events.push((
                        32,
                        Event::Land {
                            pose: pose(9),
                            effect: None,
                        },
                    ));
                    (Some(31), 32)
                }
                Strike => {
                    events.push((8, movement(6., None)));
                    events.push(contact(16, 8));
                    if enemy == GHOST {
                        events.push((
                            16,
                            Event::Sound((self.sound)(super::voice::Sound::Cue(60))?),
                        ));
                        events.push((
                            8,
                            Event::Trail {
                                slot: 0,
                                duration: 24,
                            },
                        ));
                    }
                    (if enemy == GHOST { Some(30) } else { None }, 32)
                }
                Swing => {
                    for at in [12, 28] {
                        events.push(contact(at, 8));
                        events.push((
                            at,
                            Event::Sound((self.sound)(super::voice::Sound::Cue(61))?),
                        ));
                    }
                    (Some(31), 44)
                }
                Spit | Scatter | Lob => {
                    let (times, opening): (&[u16], _) = match attack {
                        Spit => (&[24], if enemy == GHOST { 31 } else { 2 }),
                        Scatter => (&[12, 20, 28], 30),
                        Lob => (&[12, 28], 33),
                        _ => unreachable!(),
                    };
                    let projectile = metadata
                        .projectile
                        .as_ref()
                        .context("missing enemy projectile")?;
                    let projectile = self.prepared.projectile(
                        self.files,
                        projectile,
                        hit,
                        &[
                            (0, &self.common),
                            (
                                2,
                                self.enemy_effects
                                    .get(&enemy)
                                    .context("missing enemy effect bank")?,
                            ),
                        ],
                    )?;
                    for &at in times {
                        events.push((at, Event::Projectile(Arc::clone(&projectile))));
                    }
                    if enemy == GHOST {
                        events.push((
                            24,
                            Event::Sound((self.sound)(super::voice::Sound::Cue(61))?),
                        ));
                    } else if matches!(attack, Lob) {
                        events.push((0, Event::Sound((self.sound)(super::voice::Sound::Cue(55))?)));
                        if let Some(pose) = pose(34) {
                            events.push((32, Event::Pose(pose)));
                        }
                    }
                    (Some(opening), times.last().unwrap() + 16)
                }
                FireBall => unreachable!(),
            };
            if !matches!(attack, Pounce)
                && let Some(pose) = pose(0)
            {
                events.push((end_at, Event::Pose(pose)));
            }
            events.sort_by_key(|(at, _)| *at);
            actions.push(bindings.insert(ActionDefinition {
                normal: None,
                tp_cost: u16::from(metadata.tp),
                execution: super::ActionExecution::Attack(PreparedAttack {
                    opening: opening.and_then(pose),
                    events,
                    chain_at: None,
                    end_at,
                    recovery: 16,
                }),
            }));
        }
        Ok(actions)
    }
}

#[cfg(test)]
mod statistics_tests {
    use super::*;

    #[test]
    fn difficulty_scales_offense_and_vitals_without_changing_source_or_defense() -> Result<()> {
        let mut source = MonsterStats {
            hp: 501,
            initial_hp: 120,
            tp: 91,
            initial_tp: 20,
            attack: 101,
            thrust: 83,
            defense: 30,
            intelligence: 43,
            accuracy: 53,
            evasion: 23,
            luck: 25,
            level: 10,
            experience: 17,
            gald: 41,
        };
        let before = source.clone();
        let scaled = [0, 1, 2].map(|rank| statistics(&source, rank).unwrap());
        assert_eq!(
            scaled.map(|s| (s.hp, s.tp)),
            [(501, 91), (751, 136), (1002, 182)]
        );
        assert_eq!(
            scaled.map(|s| (s.initial_hp, s.initial_tp)),
            [(120, 20), (180, 30), (240, 40)]
        );
        assert_eq!(
            scaled.map(|s| (s.attack, s.thrust)),
            [(101, 83), (126, 103), (151, 124)]
        );
        assert!(
            scaled
                .iter()
                .all(|s| s.defense == 30 && s.luck == 25 && s.level == 10)
        );
        assert_eq!(source, before);
        assert!(statistics(&source, 3).is_err());
        source.initial_hp = 0;
        source.initial_tp = 0;
        let full = statistics(&source, 1)?;
        assert_eq!((full.initial_hp, full.initial_tp), (full.hp, full.tp));
        source.hp = u32::MAX;
        source.initial_hp = u32::MAX;
        source.tp = 40_000;
        source.initial_tp = u16::MAX;
        source.attack = u16::MAX;
        source.thrust = -3;
        source.intelligence = -1;
        let bounded = statistics(&source, 2)?;
        assert_eq!((bounded.hp, bounded.initial_hp), (i32::MAX, i32::MAX));
        assert_eq!((bounded.tp, bounded.initial_tp), (u16::MAX, u16::MAX));
        assert!(bounded.attack > i32::from(u16::MAX));
        assert_eq!((bounded.thrust, bounded.intelligence), (0, 0));
        Ok(())
    }
}
