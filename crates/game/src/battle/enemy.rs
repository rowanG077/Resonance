//! Prepare native enemy actions and hit volumes.

use super::ActionDefinition;
use anyhow::{Context, Result};
use resonance_battle::Difficulty;
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
pub fn statistics(source: &MonsterStats, difficulty: Difficulty) -> EnemyStatistics {
    let vital_numerator = difficulty as u64 + 2;
    let combat_numerator = difficulty as i32 + 4;
    let hp = |value: u32| (u64::from(value) * vital_numerator / 2).min(i32::MAX as u64) as i32;
    let tp = |value: u16| (u64::from(value) * vital_numerator / 2).min(u64::from(u16::MAX)) as u16;
    let combat = |value: i32| value.max(0) * combat_numerator / 4;
    let max_hp = hp(source.hp);
    let max_tp = tp(source.tp);
    EnemyStatistics {
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
    }
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
        overlimit: resonance_battle::OverLimit::new(
            u16::try_from(profile.initial_overlimit)
                .context("negative initial Over Limit charge")?,
        )?,
        guard: resonance_battle::Guard {
            recovery_bonus: guard_recovery_bonus,
            ..Default::default()
        },
        hp: stats.initial_hp,
        tp: stats.initial_tp,
        body: Body {
            collider: Some(match monster.id {
                SEWER_RAT => Collider::standing(35., 70.),
                PUMPKIN_TREE => Collider::standing(65., 190.),
                _ => Collider::standing(35., 160.),
            }),
            ..Default::default()
        },
        ..Actor::new(
            Side::Enemy,
            Control::Enemy,
            resonance_battle::EquipmentAttributes {
                luck: stats.luck,
                affinities: monster.affinities.map(|code| match code {
                    1 => Affinity::Weak,
                    2 => Affinity::Resistant,
                    3 => Affinity::Absorb,
                    4 => Affinity::Immune,
                    _ => Affinity::Normal,
                }),
                base_element: monster.attack_element,
                ..resonance_battle::EquipmentAttributes::new(
                    stats.hp,
                    stats.tp,
                    CombatStats {
                        slash: stats.attack,
                        thrust: stats.thrust,
                        defense: stats.defense,
                        intelligence: stats.intelligence,
                        accuracy: stats.accuracy,
                        evasion: stats.evasion,
                        level: stats.level,
                    },
                )
            },
        )
    };
    // Prepare braking before an enemy can time out during approach.
    actor.movement.braking = 0.55;
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
    weapons: &[std::sync::Arc<resonance_battle::WeaponDefinition>],
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
        let selected = model.contact_group_anchors_with_weapons(group, weapons);
        anyhow::ensure!(
            group < BODY_CONTACT_SLOTS || !selected.is_empty(),
            "missing enemy melee attachment group {group}"
        );
        anchors.extend(selected);
    }
    Ok(anchors)
}

impl super::encounter::resources::Resources<'_> {
    pub(crate) fn enemy(
        &mut self,
        enemy: u8,
        model: Option<&resonance_battle::ModelDefinition>,
        source: &battle_enemy::Definition,
        weapon_groups: &[Vec<std::sync::Arc<resonance_battle::WeaponDefinition>>],
        bindings: &mut resonance_battle::ActionDefinitions,
    ) -> Result<Vec<resonance_battle::ActionKey>> {
        use resonance_battle::{AttackEvent as Event, PreparedAttack};
        use resonance_content::battle_action::{EnemyEvent, EnemyExecution};
        use std::sync::Arc;
        let body_motion = !source.profile.traits.body_motion_disabled;
        let pose = |source: resonance_content::battle_action::ActionPose| -> Result<_> {
            super::model::action_pose(model, source)
                .with_context(|| format!("missing enemy {enemy} action clip {}", source.clip))
        };
        let placements = self.unison;
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
                    let catalogue = catalogue_id(placements, *technique).with_context(|| {
                        format!("enemy technique {technique} has no catalogue binding")
                    })?;
                    anyhow::ensure!(
                        self.catalogue
                            .definition(usize::from(catalogue))?
                            .capabilities
                            .spell,
                        "enemy technique {technique} requires a physical action"
                    );
                    let (mut cast, mut feedback) = self.casting(
                        super::model::ModelSource::Enemy(enemy),
                        catalogue,
                        *technique,
                        metadata.cast_ticks.map(u32::from),
                    )?;
                    Arc::make_mut(&mut cast).armor = metadata.cast_armor;
                    if cast.release.transition_ticks > 0 && cast.release.summon.is_none() {
                        if body_motion {
                            Arc::make_mut(&mut cast).release_pose =
                                super::encounter::resources::stored_cast_pose(
                                    model.context("stored spell requires its caster model")?,
                                    source.profile.cast_poses,
                                    &source.null_motions,
                                    cast.release.transition_ticks,
                                )?;
                        }
                    } else {
                        Arc::make_mut(&mut cast).release_pose =
                            super::encounter::resources::anticipated_cast_pose(
                                model,
                                source.profile.cast_poses,
                                body_motion,
                            )?;
                    }
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
            if let EnemyExecution::Physical { technique, .. } = execution {
                events.push((0, Event::Notice { duration: 90 }));
                let common = self.voices.party_table().martial_attacks.variant(
                    resonance_content::battle_martial::Binding::enemy(*technique),
                )?;
                let effect = common.effect;
                let lightning_hit = if matches!(*technique, 85 | 87) {
                    Some(
                        *common
                            .callback_hits
                            .get(&1)
                            .context("missing Lightning Blade follow-up hit")?,
                    )
                } else {
                    None
                };
                let fierce_hit = if *technique == 4 {
                    Some(
                        common
                            .fierce_fang
                            .as_ref()
                            .context("missing Fierce Demon Fang recipe")?
                            .hits[0],
                    )
                } else {
                    None
                };
                const ARTE_BRAKING: f32 = 0.55;
                events.push((0, Event::Braking(ARTE_BRAKING)));
                if let Some(member) = effect {
                    events.push((
                        0,
                        Event::ActionEffect {
                            appearance: self.prepared.effect_member(&self.techniques, member)?,
                            origin: resonance_battle::AttackOrigin::ActorRoot,
                        },
                    ));
                }
                if matches!(*technique, 63..=71) {
                    let catalogue = catalogue_id(placements, *technique)
                        .context("missing enemy seal catalogue binding")?;
                    const SEAL_FOLLOW_UP: u16 = 15;
                    let mut at = SEAL_FOLLOW_UP;
                    for (start, event) in &attack.events {
                        if let EnemyEvent::Pose(pose) = event
                            && *start < at
                        {
                            at += u16::from(if *start == 0 {
                                pose.blend
                            } else {
                                pose.blend.saturating_sub(1)
                            });
                        }
                    }
                    events.push((at, Event::Seal(self.seal(catalogue)?)));
                }
                if let Some(hit) = lightning_hit {
                    events.push(self.lightning_blade_follow_up(&hit, *technique == 87)?);
                }
                if let Some(hit) = fierce_hit {
                    events.extend(self.fierce_fang_shockwaves(&hit, 29)?);
                }
            }
            let weapons =
                if let Some(resonance_content::battle_action::EnemyBehavior::ActionWeapons {
                    actions,
                    ..
                }) = &source.actions.policy.behavior
                {
                    match actions.get(index).context("missing enemy weapon choice")? {
                        Some(group) => weapon_groups
                            .get(usize::from(*group))
                            .context("missing enemy weapon group")?
                            .as_slice(),
                        None => model.map_or(&[][..], |model| model.weapons.as_slice()),
                    }
                } else {
                    model.map_or(&[][..], |model| model.weapons.as_slice())
                };
            for (at, event) in &attack.events {
                if *at > attack.duration {
                    continue;
                }
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
                    EnemyEvent::Effect(member) => Event::ActionEffect {
                        appearance: self.enemy_effect(enemy, *member)?,
                        origin: resonance_battle::AttackOrigin::ActorRoot,
                    },
                    EnemyEvent::CarriedEffect { slot, bone, member } => {
                        let weapon = weapons
                            .iter()
                            .find(|weapon| weapon.slot == Some(*slot))
                            .context("carried effect weapon is not prepared")?;
                        anyhow::ensure!(
                            usize::from(*bone) < weapon.bone_count(),
                            "carried effect bone is not prepared"
                        );
                        Event::InitializedEffect {
                            appearance: self.enemy_effect(enemy, *member)?,
                            origin: resonance_battle::AttackOrigin::Bone(
                                resonance_battle::MeleeAnchor {
                                    resource: weapon.primary_resource(),
                                    bone: *bone,
                                },
                            ),
                        }
                    }
                    EnemyEvent::CommonEffect(member) => Event::InitializedEffect {
                        appearance: self.prepared.effect_member(&self.common, *member)?,
                        origin: resonance_battle::AttackOrigin::ActorAim,
                    },
                    EnemyEvent::ReleaseSpell { technique, slot } => {
                        let catalogue =
                            catalogue_id(placements, *technique).with_context(|| {
                                format!("enemy direct spell {technique} has no catalogue binding")
                            })?;
                        let definition = self.spell(catalogue)?;
                        let rule = self.catalogue.definition(usize::from(catalogue))?;
                        let tint = self
                            .tints
                            .and_then(|tints| tints.effect(usize::from(rule.element)))
                            .unwrap_or_default();
                        let member = if definition.transition_ticks > 0 {
                            rule.casting.effects.opening_member()
                        } else {
                            rule.casting.effects.members()[1]
                        };
                        let effect = Some(self.prepared.effect_member(&self.common, member)?);
                        let sound = (self.sound)(super::voice::Sound::Cue(
                            if definition.transition_ticks > 0 {
                                super::audio::STORED_SPELL_RELEASE
                            } else {
                                super::audio::SPELL_RELEASE
                            },
                        ))?;
                        Event::ReleaseSpell {
                            definition,
                            slot: *slot,
                            effect,
                            tint,
                            sound,
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
                                weapon.slot == Some(*slot) && weapon.has_carried_motion(*clip)
                            })),
                            "enemy {enemy} has no independent carried clip {clip} in slot {slot:?}"
                        );
                        Event::CarriedPose {
                            slot: *slot,
                            clip: *clip,
                            rate: *rate,
                            repeat: *repeat,
                        }
                    }
                    EnemyEvent::Contact(contact) | EnemyEvent::ContinueContact(contact) => {
                        let resonance_content::battle_action::MeleeContact {
                            duration,
                            attachments,
                            shape,
                            radius,
                            height,
                            hit,
                        } = contact;
                        let volume = match attachments {
                            resonance_content::battle_action::ContactAttachments::BodyCenter {} => {
                                resonance_battle::MeleeVolume::BodyCenter {
                                    shape: *shape,
                                    radius: *radius,
                                    half_height: *height,
                                }
                            }
                            _ => {
                                let model = model.context("missing enemy melee model")?;
                                let anchors = contact_anchors(model, weapons, attachments)
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
                                }
                            }
                        };
                        let definition = Arc::new(resonance_battle::MeleeDefinition {
                            hit: self.enemy_hit(enemy, hit)?,
                            volume,
                            trail: None,
                        });
                        if matches!(event, EnemyEvent::ContinueContact(_)) {
                            let (previous_at, previous_duration) = events
                                .iter_mut()
                                .rev()
                                .find_map(|(at, event)| match event {
                                    Event::Contact { duration, .. }
                                    | Event::ContinueContact { duration, .. } => {
                                        Some((*at, duration))
                                    }
                                    _ => None,
                                })
                                .context("enemy contact continuation has no previous window")?;
                            anyhow::ensure!(
                                u32::from(previous_at) + u32::from(*previous_duration)
                                    == u32::from(*at),
                                "enemy contact continuation is not contiguous"
                            );
                            // Keep the window alive through the definition update.
                            *previous_duration = at
                                .checked_add(*duration)
                                .and_then(|end| end.checked_sub(previous_at))
                                .context("continued enemy contact exceeds tick range")?;
                            Event::ContinueContact {
                                definition,
                                duration: *duration,
                            }
                        } else {
                            Event::Contact {
                                definition,
                                duration: *duration,
                            }
                        }
                    }
                    EnemyEvent::Projectile {
                        definition,
                        hit,
                        feedback,
                    } => {
                        let mut projectile =
                            self.enemy_projectile(enemy, model, definition, hit)?;
                        if let Some(feedback) = feedback {
                            Arc::make_mut(&mut projectile).effects.contact =
                                Some(resonance_battle::ProjectileContactFeedback {
                                    appearance: self
                                        .prepared
                                        .effect_member(&self.techniques, feedback.effect)?,
                                    on_guard: feedback.on_guard,
                                    sound: if feedback.sound == 0 {
                                        None
                                    } else {
                                        (self.sound)(super::voice::Sound::Cue(feedback.sound))?
                                    },
                                });
                        }
                        Event::Projectile(projectile)
                    }
                    EnemyEvent::GroundProjectiles {
                        definitions,
                        hit,
                        retreat,
                    } => Event::GroundProjectiles {
                        definitions: [
                            self.enemy_projectile(enemy, model, &definitions[0], hit)?,
                            self.enemy_projectile(enemy, model, &definitions[1], hit)?,
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
                    EnemyEvent::AfflictOpponents { groups, dispel } => Event::AfflictOpponents {
                        groups: groups.clone(),
                        dispel: *dispel,
                    },
                    EnemyEvent::Stun => Event::Stun,
                    EnemyEvent::WeaponVisibility { slot, visible } => {
                        Event::WeaponVisibility(vec![(Some(*slot), *visible)])
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
            // Recovery keeps an already selected clip running.
            let last_pose = events.iter().rev().find_map(|(at, event)| match event {
                Event::Pose(pose) | Event::PoseKeepingParts(pose) if *at <= attack.duration => {
                    Some(pose.motion.clip)
                }
                _ => None,
            });
            // An ordinary attack without pose commands retains its live animation.
            let keeps_live_pose =
                last_pose.is_none() && matches!(execution, EnemyExecution::Attack(_));
            if body_motion && !keeps_live_pose && last_pose != Some(attack.recovery_pose.clip) {
                events.push((attack.duration, Event::Pose(pose(attack.recovery_pose)?)));
            }
            events.sort_by_key(|(at, _)| *at);
            let mut prepared = PreparedAttack {
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
            };
            if matches!(execution, EnemyExecution::Physical { technique: 121, .. }) {
                super::martial::prepare_eagle_landing(
                    &mut prepared,
                    model,
                    self.prepared.effect_member(&self.techniques, 92)?,
                    None,
                )?;
            }
            let key = bindings.insert(ActionDefinition {
                normal: None,
                tp_cost: u16::from(metadata.tp),
                execution: super::ActionExecution::Attack(prepared),
            });
            if let EnemyExecution::Physical { technique, .. } = execution {
                let catalogue = catalogue_id(placements, *technique)
                    .context("enemy physical technique has no catalogue binding")?;
                self.feedback
                    .action_catalogues
                    .insert(key, resonance_battle::TechniqueId(catalogue));
            }
            actions.push(key);
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
        let rule = self.enemy_hit(enemy, hit)?;
        const OWNER_EFFECT_BANK: u8 = 2;
        let mut definition = definition.clone();
        // Enemy projectile birth and trail artwork belongs to its owner.
        definition.birth_effect.bank = OWNER_EFFECT_BANK;
        definition.trail_effect.bank = OWNER_EFFECT_BANK;
        let mut projectile = self.prepared.projectile(
            self.files,
            &definition,
            rule,
            &[
                (0, &self.common),
                (
                    OWNER_EFFECT_BANK,
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
    ) -> Result<resonance_battle::HitRule> {
        let mut hit = super::hit::authored(source, self.recoil)?;
        if let Some(sound) = hit.sound {
            hit.sound = (self.sound)(sound)?;
        }
        if let Some(member) = source.effect {
            hit.effect = Some(self.enemy_effect(enemy, member)?);
        }
        Ok(hit)
    }

    fn enemy_effect(
        &mut self,
        enemy: u8,
        member: u16,
    ) -> Result<resonance_battle::EffectAppearance> {
        self.prepared.effect_member(
            self.enemy_effects
                .get(&enemy)
                .context("missing enemy effect bank")?,
            member,
        )
    }
}

#[cfg(test)]
mod statistics_tests {
    use super::*;

    #[test]
    fn difficulty_scales_offense_and_vitals_without_changing_source_or_defense() {
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
        let scaled = [Difficulty::Normal, Difficulty::Hard, Difficulty::Mania]
            .map(|rank| statistics(&source, rank));
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
        source.initial_hp = 0;
        source.initial_tp = 0;
        let full = statistics(&source, Difficulty::Hard);
        assert_eq!((full.initial_hp, full.initial_tp), (full.hp, full.tp));
        source.hp = u32::MAX;
        source.initial_hp = u32::MAX;
        source.tp = 40_000;
        source.initial_tp = u16::MAX;
        source.attack = u16::MAX;
        source.thrust = -3;
        source.intelligence = -1;
        let bounded = statistics(&source, Difficulty::Mania);
        assert_eq!((bounded.hp, bounded.initial_hp), (i32::MAX, i32::MAX));
        assert_eq!((bounded.tp, bounded.initial_tp), (u16::MAX, u16::MAX));
        assert!(bounded.attack > i32::from(u16::MAX));
        assert_eq!((bounded.thrust, bounded.intelligence), (0, 0));
    }
}
