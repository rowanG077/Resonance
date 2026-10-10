//! Each technique owns its complete native attack and optional feedback.
use super::{ActionDefinition, Character, Sequence, Sound, Technique};
use anyhow::{Context, Result, bail};
use resonance_battle::{
    AttackEvent as Event, AttackPose, EffectAppearance, MeleeVolume, PreparedAttack,
};
use std::sync::Arc;

fn movement(forward: Option<f32>, vertical: Option<f32>) -> Event {
    Event::Move { forward, vertical }
}

impl Technique {
    fn hit(self) -> resonance_battle::HitRule {
        use Sequence::*;
        let power = match self.sequence {
            DemonFang => 150,
            RayThrust | CrescentMoon => 180,
            Infliction => 200,
            SpinKick => 170,
            PyreSeal | PowerSeal | Destruction => 160,
            Beast => 140,
            EagleDive => 220,
            _ => unreachable!("non-damaging technique has no hit"),
        };
        let mut hit = crate::battle::hit::physical(power);
        hit.arte = true;
        hit.reaction.recoil.knock_down = matches!(self.sequence, Infliction | Beast | EagleDive);
        hit.reaction.hits_down = hit.reaction.recoil.knock_down;
        if self.sequence == CrescentMoon {
            hit.reaction.recoil.launch = true;
            hit.reaction.recoil.impulse[1] = 8.;
        }
        if self.sequence == RayThrust {
            hit.kind = resonance_battle::DamageKind::Thrust;
        }
        if self.sequence == PyreSeal {
            hit.element = resonance_battle::HitElement::Element(resonance_battle::Element::Fire);
        }
        if self.sequence == PowerSeal {
            hit.condition = Some(resonance_battle::HitCondition {
                condition: resonance_content::battle_action::Condition::DefenseDown,
                chance: 100,
                value: -10,
            });
        }
        hit
    }

    pub(crate) fn prepare<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
    ) -> Result<ActionDefinition> {
        use Sequence::*;
        let source = crate::battle::model::ModelSource::Party(self.character as u8);
        let pose = |clip, blend| {
            crate::battle::model::motion(model, clip).map(|motion| AttackPose {
                motion,
                start: 0.,
                rate: 0.5,
                blend,
            })
        };
        let tp_cost = u16::from(
            resources
                .catalogue
                .definition(usize::from(self.catalogue))?
                .tp_cost,
        );
        if self.sequence == SpecialGuard {
            let guard = super::special_guard(self.character as u8).unwrap();
            resources.effect(true, &[guard.effect])?;
            const STANCE_TICKS: u16 = 60;
            let mut events = vec![
                (0, Event::SpecialGuard),
                (
                    0,
                    Event::Effect {
                        appearance: EffectAppearance {
                            resource: resources.techniques.resource,
                            member: guard.effect,
                        },
                        tints: Default::default(),
                        centered: true,
                    },
                ),
                (0, Event::Notice { duration: 90 }),
                (0, Event::Sound((resources.sound)(Sound::Cue(69))?)),
            ];
            if let Some(line) =
                resources
                    .voices
                    .absolute(source, Some(guard.voice), &mut resources.sound)?
            {
                events.push((0, Event::Voice(line)));
            }
            events.extend(pose(0, 4).map(|pose| (STANCE_TICKS, Event::Pose(pose))));
            return Ok(ActionDefinition {
                normal: None,
                tp_cost,
                execution: crate::battle::ActionExecution::Attack(PreparedAttack {
                    opening: pose(22, 4),
                    events,
                    chain_at: None,
                    end_at: STANCE_TICKS,
                    recovery: 12,
                }),
            });
        }
        let contact = |duration, volume| Event::Contact {
            definition: Arc::new(resonance_battle::MeleeDefinition {
                hit: self.hit(),
                volume,
                trail: Some(0),
            }),
            duration,
        };
        let mut projectile = |member: usize, hit, luminous: bool| -> Result<Event> {
            let mut row = resources
                .projectiles
                .records
                .get(member)
                .context("missing technique projectile artwork")?
                .clone();
            if luminous {
                // The luminous trail is the projectile's body.
                row.birth_effect.member = 0;
                row.shadow = None;
            }
            Ok(Event::Projectile(resources.prepared.projectile(
                resources.files,
                &row,
                hit,
                &[(1, &resources.techniques)],
            )?))
        };
        let mut sound = |cue| (resources.sound)(Sound::Cue(cue)).map(Event::Sound);
        let appearance = |member| EffectAppearance {
            resource: resources.techniques.resource,
            member,
        };
        // Artwork identifiers and gameplay tuning stay beside the events they serve.
        let (mut attack, voice, startup) = match self.sequence {
            DemonFang => {
                let (voice, startup) = match self.character {
                    Character::Lloyd => (59, 3),
                    Character::Zelos => (649, 4),
                    Character::Kratos => (968, 4),
                    _ => bail!("Demon Fang requires a swordsman"),
                };
                (
                    PreparedAttack {
                        opening: pose(43, 4),
                        chain_at: Some(28),
                        end_at: 36,
                        recovery: 8,
                        events: vec![
                            (
                                0,
                                Event::Trail {
                                    slot: u8::from(self.character == Character::Lloyd),
                                    duration: 36,
                                },
                            ),
                            (12, movement(Some(4.), None)),
                            (12, sound(60)?),
                            (12, projectile(2, self.hit(), false)?),
                        ],
                    },
                    voice,
                    Some(startup),
                )
            }
            RayThrust => (
                PreparedAttack {
                    opening: pose(43, 4),
                    chain_at: Some(32),
                    end_at: 40,
                    recovery: 8,
                    events: vec![
                        (20, movement(Some(6.), None)),
                        (20, sound(61)?),
                        (20, projectile(6, self.hit(), true)?),
                    ],
                },
                182,
                None,
            ),
            Infliction => (
                PreparedAttack {
                    opening: pose(60, 4),
                    chain_at: Some(24),
                    end_at: 36,
                    recovery: 8,
                    events: vec![
                        (8, movement(Some(3.), Some(10.))),
                        (8, sound(63)?),
                        (
                            8,
                            contact(
                                12,
                                MeleeVolume {
                                    offset: [0., 75., 90.],
                                    radius: 90.,
                                    half_height: 80.,
                                },
                            ),
                        ),
                    ],
                },
                769,
                Some(87),
            ),
            CrescentMoon => {
                let mut attack = PreparedAttack {
                    opening: pose(38, 4),
                    chain_at: Some(24),
                    end_at: 44,
                    recovery: 8,
                    events: vec![
                        (4, movement(Some(3.), Some(20.))),
                        (4, sound(64)?),
                        (
                            4,
                            contact(
                                10,
                                MeleeVolume {
                                    offset: [0., 85., 70.],
                                    radius: 65.,
                                    half_height: 100.,
                                },
                            ),
                        ),
                    ],
                };
                attack
                    .events
                    .extend(pose(16, 4).map(|pose| (40, Event::Pose(pose))));
                (attack, 866, Some(53))
            }
            SpinKick => (
                PreparedAttack {
                    opening: pose(44, 4),
                    chain_at: Some(24),
                    end_at: 32,
                    recovery: 8,
                    events: vec![
                        (12, movement(Some(5.), None)),
                        (12, sound(64)?),
                        (
                            12,
                            contact(
                                10,
                                MeleeVolume {
                                    offset: [0., 60., 0.],
                                    radius: 130.,
                                    half_height: 80.,
                                },
                            ),
                        ),
                    ],
                },
                867,
                None,
            ),
            PyreSeal | PowerSeal => {
                let voice = if self.sequence == PowerSeal { 530 } else { 529 };
                (
                    PreparedAttack {
                        opening: pose(30, 4),
                        chain_at: Some(24),
                        end_at: 36,
                        recovery: 8,
                        events: vec![
                            (
                                0,
                                Event::WeaponVisibility(vec![
                                    (0, true),
                                    (1, false),
                                    (2, true),
                                    (3, false),
                                ]),
                            ),
                            (8, movement(Some(5.), None)),
                            (8, sound(114)?),
                            (
                                8,
                                contact(
                                    12,
                                    MeleeVolume {
                                        offset: [0., 65., 65.],
                                        radius: 60.,
                                        half_height: 60.,
                                    },
                                ),
                            ),
                        ],
                    },
                    voice,
                    Some(49),
                )
            }
            Beast => {
                let volume = MeleeVolume {
                    offset: [0., 75., 90.],
                    radius: 90.,
                    half_height: 80.,
                };
                let mut attack = PreparedAttack {
                    opening: pose(65, 4),
                    chain_at: Some(36),
                    end_at: 44,
                    recovery: 8,
                    events: vec![
                        (12, movement(Some(5.), None)),
                        (12, sound(63)?),
                        (12, contact(8, volume)),
                        (24, movement(Some(5.), None)),
                        (24, sound(116)?),
                        (24, contact(8, volume)),
                    ],
                };
                attack
                    .events
                    .extend(pose(66, 4).map(|pose| (24, Event::Pose(pose))));
                (attack, 778, Some(119))
            }
            Mirage => {
                let mut attack = PreparedAttack {
                    opening: pose(59, 4),
                    chain_at: None,
                    end_at: 24,
                    recovery: 8,
                    events: vec![
                        (0, movement(Some(14.), None)),
                        (0, Event::PassThrough(true)),
                        (24, movement(Some(0.), None)),
                    ],
                };
                attack
                    .events
                    .extend(pose(60, 4).map(|pose| (6, Event::Pose(pose))));
                (attack, 891, None)
            }
            Destruction => {
                let rock = projectile(
                    9,
                    resonance_battle::HitRule {
                        arte: true,
                        ..crate::battle::hit::physical(60)
                    },
                    false,
                )?;
                let mut attack = PreparedAttack {
                    opening: pose(50, 4),
                    chain_at: Some(40),
                    end_at: 48,
                    recovery: 8,
                    events: vec![
                        (16, movement(Some(4.), Some(12.))),
                        (16, sound(63)?),
                        (
                            16,
                            contact(
                                10,
                                MeleeVolume {
                                    offset: [0., 65., 65.],
                                    radius: 60.,
                                    half_height: 60.,
                                },
                            ),
                        ),
                        (24, rock.clone()),
                        (28, rock.clone()),
                        (32, rock),
                    ],
                };
                attack
                    .events
                    .extend(pose(51, 4).map(|pose| (32, Event::Pose(pose))));
                (attack, 760, Some(89))
            }
            EagleDive => (
                PreparedAttack {
                    opening: pose(51, 4),
                    chain_at: None,
                    end_at: 12,
                    recovery: 24,
                    events: vec![
                        (0, movement(Some(4.), Some(12.))),
                        (8, sound(64)?),
                        (8, movement(None, Some(-20.))),
                        (
                            12,
                            Event::Land {
                                pose: pose(52, 4),
                                effect: Some(appearance(92)),
                            },
                        ),
                        (
                            12,
                            contact(
                                1,
                                MeleeVolume {
                                    offset: [0., 60., 0.],
                                    radius: 130.,
                                    half_height: 80.,
                                },
                            ),
                        ),
                    ],
                },
                875,
                Some(91),
            ),
            _ => bail!("technique does not use a direct attack"),
        };
        attack.events.push((0, Event::Notice { duration: 90 }));
        if let Some(member) = startup {
            attack.events.push((
                0,
                Event::Effect {
                    appearance: appearance(member),
                    tints: Default::default(),
                    centered: false,
                },
            ));
        }
        if let Some(line) =
            resources
                .voices
                .absolute(source, Some(Sound::Stream(voice)), &mut resources.sound)?
        {
            attack.events.push((0, Event::Voice(line)));
        }
        if self.sequence != EagleDive {
            attack
                .events
                .extend(pose(0, 8).map(|pose| (attack.end_at, Event::Pose(pose))));
        }
        let members = attack
            .events
            .iter()
            .filter_map(|(_, event)| match event {
                Event::Effect { appearance, .. } => Some(appearance.member),
                Event::Land {
                    effect: Some(effect),
                    ..
                } => Some(effect.member),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !members.is_empty() {
            resources.effect(true, &members)?;
        }
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(ActionDefinition {
            normal: None,
            tp_cost,
            execution: crate::battle::ActionExecution::Attack(attack),
        })
    }
}
