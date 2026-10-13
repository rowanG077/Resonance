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
            guaranteed_steal: false,
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
        secondary_wind: Default::default(),
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

pub(super) fn catalogue_id(
    catalogue: &resonance_content::battle_unison::Catalogue,
    native: u16,
) -> Option<u16> {
    catalogue
        .placements
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(id, placement)| (placement.native_id == native).then_some(id as u16))
}

pub(super) fn technique_ids(source: &battle_enemy::Definition) -> std::collections::BTreeSet<u16> {
    use resonance_content::battle_action::{EnemyEvent, EnemyExecution};
    let mut techniques = std::collections::BTreeSet::new();
    for row in &source.actions.rows {
        let attack = match &row.execution {
            Some(EnemyExecution::Spell(technique)) => {
                techniques.insert(*technique);
                continue;
            }
            Some(EnemyExecution::Physical { technique, attack }) => {
                techniques.insert(*technique);
                attack
            }
            Some(EnemyExecution::Attack(attack)) => attack,
            Some(EnemyExecution::LeaveArena) | None => continue,
        };
        techniques.extend(attack.events.iter().filter_map(|(_, event)| match event {
            EnemyEvent::ReleaseSpell { technique, .. } => Some(*technique),
            _ => None,
        }));
    }
    techniques
}
pub const GHOST: u8 = 49;
pub const PHANTOM: u8 = 50;

fn contact_anchors(
    model: &resonance_battle::ModelDefinition,
    attachments: &resonance_content::battle_action::ContactAttachments,
) -> Result<Vec<resonance_battle::MeleeAnchor>> {
    use resonance_content::battle_action::ContactAttachments;
    let ContactAttachments::Groups(groups) = attachments else {
        return model.contact_anchors(attachments);
    };
    // Enemy bodies reserve these slots even when they contain no contact markers.
    const BODY_CONTACT_SLOTS: u8 = 12;
    let mut anchors = Vec::new();
    for &group in groups {
        let selected = model.contact_group_anchors(group);
        anyhow::ensure!(
            group < BODY_CONTACT_SLOTS || !selected.is_empty(),
            "missing enemy melee attachment group {group}"
        );
        anchors.extend(selected);
    }
    Ok(anchors)
}

impl<S: FnMut(super::voice::Sound) -> Result<Option<resonance_battle::Sound>>>
    super::encounter::resources::Resources<'_, S>
{
    pub(crate) fn enemy(
        &mut self,
        enemy: u8,
        model: Option<&resonance_battle::ModelDefinition>,
        source: &battle_enemy::Definition,
        bindings: &mut resonance_battle::ActionDefinitions,
    ) -> Result<Vec<resonance_battle::ActionKey>> {
        use resonance_battle::{AttackEvent as Event, AttackPose, PreparedAttack};
        use resonance_content::battle_action::{EnemyEvent, EnemyExecution};
        use std::sync::Arc;
        let recoil = super::recoil::Parameters::load(self.files)?;
        let body_motion = !source.profile.traits.body_motion_disabled;
        let pose = |source: resonance_content::battle_action::ActionPose| -> Result<_> {
            Ok(AttackPose {
                motion: super::model::motion(model, source.clip).with_context(|| {
                    format!("missing enemy {enemy} action clip {}", source.clip)
                })?,
                start: source.start,
                end: source.end,
                repeat: source.repeat,
                rate: source.rate,
                blend: source.blend,
            })
        };
        let placements: resonance_content::battle_unison::Catalogue =
            self.files.json(resonance_content::battle_unison::PATH)?;
        let mut actions = Vec::new();
        for (index, metadata) in source.actions.rows.iter().enumerate() {
            if let Some(reason) = &metadata.unsupported_reason {
                anyhow::bail!("enemy {enemy} action {index}: {reason}");
            }
            let execution = metadata
                .execution
                .as_ref()
                .context("missing enemy action timeline")?;
            let attack = match execution {
                EnemyExecution::LeaveArena => {
                    actions.push(bindings.insert(ActionDefinition {
                        normal: None,
                        tp_cost: u16::from(metadata.tp),
                        execution: super::ActionExecution::LeaveArena,
                    }));
                    continue;
                }
                EnemyExecution::Spell(technique) => {
                    let catalogue = catalogue_id(&placements, *technique).with_context(|| {
                        format!("enemy technique {technique} has no catalogue binding")
                    })?;
                    anyhow::ensure!(
                        self.catalogue
                            .definition(usize::from(catalogue))?
                            .capabilities
                            .spell,
                        "enemy technique {technique} requires a physical action"
                    );
                    let (cast, mut feedback) = self.casting(
                        super::model::ModelSource::Enemy(enemy),
                        catalogue,
                        *technique,
                        metadata.cast_ticks.map(u32::from),
                    )?;
                    feedback.release_voice = metadata
                        .voices
                        .release
                        .map(&mut self.sound)
                        .transpose()?
                        .flatten();
                    let key = bindings.insert(ActionDefinition {
                        normal: None,
                        tp_cost: u16::from(if metadata.tp == 0 {
                            self.catalogue.definition(usize::from(catalogue))?.tp_cost
                        } else {
                            metadata.tp
                        }),
                        execution: super::ActionExecution::Casting(cast),
                    });
                    self.feedback.casting.insert(key, feedback);
                    actions.push(key);
                    continue;
                }
                EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. } => attack,
            };
            let mut events = vec![(0, Event::Armor(attack.armor))];
            for (at, event) in &attack.events {
                if !body_motion
                    && matches!(
                        event,
                        EnemyEvent::Pose(_)
                            | EnemyEvent::PartialPose { .. }
                            | EnemyEvent::PoseKeepingParts(_)
                            | EnemyEvent::AnimationRate(_)
                    )
                {
                    continue;
                }
                if let EnemyEvent::Pose(pose)
                | EnemyEvent::PartialPose { pose, .. }
                | EnemyEvent::PoseKeepingParts(pose) = event
                    && source.null_motions.contains(&pose.clip)
                    && super::model::motion(model, pose.clip).is_none()
                {
                    // An authored empty visual slot keeps the current pose.
                    continue;
                }
                let event = match event {
                    EnemyEvent::Effect(member) => {
                        let mut resource = self
                            .enemy_effects
                            .get(&enemy)
                            .context("missing enemy effect bank")?
                            .clone();
                        resource.members = vec![*member];
                        let appearance = resonance_battle::EffectAppearance {
                            resource: resource.resource,
                            member: *member,
                        };
                        self.prepared.effect(resource)?;
                        Event::ActionEffect {
                            appearance,
                            origin: resonance_battle::AttackOrigin::ActorRoot,
                        }
                    }
                    EnemyEvent::CommonEffect(member) => {
                        self.effect(false, &[*member])?;
                        Event::Effect {
                            appearance: resonance_battle::EffectAppearance {
                                resource: self.common.resource,
                                member: *member,
                            },
                            tints: Default::default(),
                            centered: true,
                        }
                    }
                    EnemyEvent::ReleaseSpell { technique, slot } => {
                        let catalogue =
                            catalogue_id(&placements, *technique).with_context(|| {
                                format!("enemy direct spell {technique} has no catalogue binding")
                            })?;
                        let definition = self.spell(catalogue)?;
                        let rule = self.catalogue.definition(usize::from(catalogue))?;
                        let tint = self
                            .tints
                            .and_then(|tints| tints.effect(usize::from(rule.element)))
                            .unwrap_or_default();
                        let effect = if definition.transition_ticks == 0 {
                            let member = rule.casting.effects.members()[1];
                            self.effect(false, &[member])?;
                            Some(resonance_battle::EffectAppearance {
                                resource: self.common.resource,
                                member,
                            })
                        } else {
                            None
                        };
                        Event::ReleaseSpell {
                            definition,
                            slot: match slot {
                                resonance_content::battle_action::SpellSlot::Primary => {
                                    resonance_battle::SpellSlot::Primary
                                }
                                resonance_content::battle_action::SpellSlot::Secondary => {
                                    resonance_battle::SpellSlot::Secondary
                                }
                            },
                            effect,
                            tint,
                            sound: (self.sound)(super::voice::Sound::Cue(123))?,
                        }
                    }
                    EnemyEvent::UseItem { item, party } => {
                        let policy = super::items::policies()
                            .get(item)
                            .copied()
                            .context("enemy item has no prepared benefit")?;
                        use resonance_battle::item::Effect;
                        anyhow::ensure!(
                            if *party {
                                matches!(policy.effect, Effect::Hourglass)
                            } else {
                                matches!(
                                    policy.effect,
                                    Effect::Recover { .. } | Effect::Revive | Effect::Buff(_)
                                )
                            },
                            "enemy item recipient scope is not prepared"
                        );
                        Event::UseItem(*item)
                    }
                    EnemyEvent::Tint(color) => Event::Tint(*color),
                    EnemyEvent::Defeat { rewards } => Event::Defeat { rewards: *rewards },
                    EnemyEvent::WaitForHit { deadline } => Event::WaitForHit {
                        deadline: *deadline,
                        recovery: body_motion
                            .then(|| pose(attack.recovery_pose))
                            .transpose()?,
                    },
                    EnemyEvent::WaitForEffectiveHit { deadline } => Event::WaitForEffectiveHit {
                        deadline: *deadline,
                    },
                    EnemyEvent::ReleaseCaptured { hitstun } => {
                        Event::ReleaseCaptured { hitstun: *hitstun }
                    }
                    EnemyEvent::CapturedVisibility(visible) => Event::CapturedVisibility(*visible),
                    EnemyEvent::CapturedForward(value) => Event::CapturedForward(*value),
                    EnemyEvent::CapturedVertical(value) => Event::CapturedVertical(*value),
                    EnemyEvent::Pose(source) => Event::Pose(pose(*source)?),
                    EnemyEvent::PartialPose {
                        first_bone,
                        pose: source,
                    } => {
                        let count = model
                            .context("missing enemy partial-pose model")?
                            .skeleton
                            .bones
                            .len();
                        anyhow::ensure!(
                            usize::from(*first_bone) < count,
                            "partial-pose bone exceeds enemy model"
                        );
                        Event::PartialPose {
                            pose: pose(*source)?,
                            bones: (usize::from(*first_bone)..count)
                                .map(|bone| bone as u16)
                                .collect(),
                        }
                    }
                    EnemyEvent::PoseKeepingParts(source) => Event::PoseKeepingParts(pose(*source)?),
                    EnemyEvent::AnimationRate(rate) => Event::AnimationRate(*rate),
                    EnemyEvent::CarriedPose {
                        slot,
                        clip,
                        rate,
                        repeat,
                    } => {
                        anyhow::ensure!(
                            model.is_some_and(|model| model.weapons.iter().any(|weapon| {
                                weapon.slot == *slot && weapon.has_carried_motion(*clip)
                            })),
                            "enemy {enemy} has no independent carried clip {clip} in slot {slot}"
                        );
                        Event::CarriedPose {
                            slot: *slot,
                            clip: *clip,
                            rate: *rate,
                            repeat: *repeat,
                        }
                    }
                    EnemyEvent::Contact(resonance_content::battle_action::MeleeContact {
                        duration,
                        attachments,
                        shape,
                        radius,
                        height,
                        hit,
                    }) => {
                        let volume = match attachments {
                            resonance_content::battle_action::ContactAttachments::BodyCenter {} => {
                                resonance_battle::MeleeVolume::BodyCenter {
                                    shape: *shape,
                                    radius: *radius,
                                    half_height: *height,
                                    cooldown: hit.cooldown,
                                }
                            }
                            _ => {
                                let anchors = contact_anchors(
                                    model.context("missing enemy melee model")?,
                                    attachments,
                                )
                                .with_context(|| {
                                    format!("enemy {enemy} action {index} contact at {at}")
                                })?;
                                if anchors.is_empty() {
                                    continue;
                                }
                                resonance_battle::MeleeVolume::Attached {
                                    anchors,
                                    shape: *shape,
                                    radius: *radius,
                                    half_height: *height,
                                    cooldown: hit.cooldown,
                                }
                            }
                        };
                        Event::Contact {
                            definition: Arc::new(resonance_battle::MeleeDefinition {
                                hit: self.enemy_hit(enemy, hit, &recoil)?,
                                volume,
                                trail: None,
                            }),
                            duration: *duration,
                        }
                    }
                    EnemyEvent::Projectile { definition, hit } => Event::Projectile(
                        self.enemy_projectile(enemy, model, definition, hit, &recoil)?,
                    ),
                    EnemyEvent::GroundProjectiles {
                        definitions,
                        hit,
                        retreat,
                    } => Event::GroundProjectiles {
                        definitions: [
                            self.enemy_projectile(enemy, model, &definitions[0], hit, &recoil)?,
                            self.enemy_projectile(enemy, model, &definitions[1], hit, &recoil)?,
                        ],
                        retreat: *retreat,
                    },
                    EnemyEvent::Forward(value) => Event::Move {
                        forward: Some(*value),
                        vertical: None,
                    },
                    EnemyEvent::Vertical(value) => Event::Move {
                        forward: None,
                        vertical: Some(*value),
                    },
                    EnemyEvent::Acceleration(value) => Event::Acceleration(*value),
                    EnemyEvent::Gravity(value) => Event::Gravity(*value),
                    EnemyEvent::SetPosition(value) => Event::SetPosition(value.map(f32::from)),
                    EnemyEvent::PositionFromTarget { height, retreat } => {
                        Event::PositionFromTarget {
                            height: f32::from(*height),
                            retreat: f32::from(*retreat),
                        }
                    }
                    EnemyEvent::OffsetPosition { height, retreat } => Event::OffsetPosition {
                        height: f32::from(*height),
                        retreat: f32::from(*retreat),
                    },
                    EnemyEvent::TurnMotion(degrees) => Event::TurnMotion(*degrees),
                    EnemyEvent::FaceTarget => Event::FaceTarget,
                    EnemyEvent::SecondaryWind { enabled, duration } => Event::SecondaryWind {
                        enabled: *enabled,
                        duration: *duration,
                    },
                    EnemyEvent::Braking(value) => Event::Braking(*value),
                    EnemyEvent::ReactionArmor(duration) => Event::ReactionArmor(*duration),
                    EnemyEvent::StatUp { condition, amount } => Event::StatUp {
                        condition: *condition,
                        amount: *amount,
                    },
                    EnemyEvent::Stun => Event::Stun,
                    EnemyEvent::WeaponVisibility { slot, visible } => {
                        Event::WeaponVisibility(vec![(*slot, *visible)])
                    }
                    EnemyEvent::PassThrough { enabled, duration } => Event::PassThrough {
                        enabled: *enabled,
                        duration: *duration,
                    },
                    EnemyEvent::Trail { slot, duration } => Event::Trail {
                        slot: *slot,
                        duration: *duration,
                    },
                    EnemyEvent::Sound(cue) => {
                        Event::Sound((self.sound)(super::voice::Sound::Cue(*cue))?)
                    }
                    EnemyEvent::Voice {
                        sound,
                        announcement,
                    } => {
                        let Some(sound) = sound.map(&mut self.sound).transpose()?.flatten() else {
                            continue;
                        };
                        Event::Voice {
                            sound,
                            priority: if *announcement {
                                resonance_battle::VoicePriority::Announcement
                            } else {
                                resonance_battle::VoicePriority::Action
                            },
                        }
                    }
                };
                events.push((*at, event));
            }
            if body_motion {
                events.push((attack.duration, Event::Pose(pose(attack.recovery_pose)?)));
            }
            events.sort_by_key(|(at, _)| *at);
            actions.push(
                bindings.insert(ActionDefinition {
                    normal: None,
                    tp_cost: u16::from(metadata.tp),
                    execution: super::ActionExecution::Attack(PreparedAttack {
                        opening: None,
                        events,
                        chain_at: attack.chain_at,
                        end_at: attack.duration,
                        recovery: u16::from(attack.recovery_ticks),
                        contact_recovery: attack
                            .contact_recovery
                            .as_ref()
                            .map(|recovery| -> Result<_> {
                                Ok(resonance_battle::ContactRecovery {
                                    pose: recovery
                                        .pose
                                        .filter(|_| body_motion)
                                        .map(pose)
                                        .transpose()?,
                                    movement: recovery.movement,
                                    braking: recovery.braking,
                                    pass_through_ticks: recovery.pass_through_ticks,
                                    voice: recovery
                                        .voice
                                        .map(|voice| (self.sound)(voice))
                                        .transpose()?
                                        .flatten(),
                                    expressions: recovery.expressions,
                                })
                            })
                            .transpose()?,
                    }),
                }),
            );
        }
        if let Some(resource) = self
            .enemy_effects
            .get(&enemy)
            .and_then(|resource| self.prepared.effects.get(&resource.resource))
        {
            super::effect_program::validate_owner(resource, model)
                .with_context(|| format!("enemy {enemy} effect origins"))?;
        }
        Ok(actions)
    }
    fn enemy_projectile(
        &mut self,
        enemy: u8,
        model: Option<&resonance_battle::ModelDefinition>,
        definition: &resonance_content::battle_projectile::Projectile,
        hit: &resonance_content::battle_action::ActionHit,
        recoil: &super::recoil::Parameters,
    ) -> Result<std::sync::Arc<resonance_battle::ProjectileDefinition>> {
        use resonance_content::battle_projectile::ProjectileOrigin;
        use std::sync::Arc;
        let bones = [
            match definition.motion.origin {
                ProjectileOrigin::Bone(bone) => Some(bone),
                _ => None,
            },
            definition
                .motion
                .bone_direction
                .map(|direction| direction.from),
            definition
                .motion
                .bone_direction
                .map(|direction| direction.to),
        ];
        if bones.iter().any(Option::is_some) {
            let count = model
                .context("enemy projectile bone binding has no body model")?
                .skeleton
                .bones
                .len();
            for bone in bones.into_iter().flatten() {
                anyhow::ensure!(
                    usize::from(bone) < count,
                    "enemy {enemy} projectile bone {bone} exceeds body model"
                );
            }
        }
        let rule = self.enemy_hit(enemy, hit, recoil)?;
        let mut projectile = self.prepared.projectile(
            self.files,
            definition,
            rule,
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
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("enemy projectile has no contact")?
            .cooldown = hit.cooldown;
        Ok(projectile)
    }
    fn enemy_hit(
        &mut self,
        enemy: u8,
        source: &resonance_content::battle_action::ActionHit,
        recoil: &super::recoil::Parameters,
    ) -> Result<resonance_battle::HitRule> {
        let mut hit = super::hit::authored(source, recoil)?;
        if let Some(sound) = hit.sound {
            hit.sound = (self.sound)(sound)?;
        }
        if let Some(member) = source.effect {
            let mut resource = self
                .enemy_effects
                .get(&enemy)
                .context("missing enemy contact effect bank")?
                .clone();
            resource.members = vec![member];
            hit.effect = Some(resonance_battle::EffectAppearance {
                resource: resource.resource,
                member,
            });
            self.prepared.effect(resource)?;
        }
        Ok(hit)
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
