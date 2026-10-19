//! Persistent enemy behavior that is independent of the selected attack.
use crate::{Activity, ActorId, Battle, PreparedBattle};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    animation::Matrix,
    battle_action::{EnemyStation, WeaponDrop},
};
use std::sync::Arc;

const STATION_ACTION: usize = 2;
const STATION_REARM_TICKS: u64 = 900;

#[derive(Debug, Clone)]
pub enum EnemyBehavior {
    /// Leave when either the health threshold or elapsed gameplay time is reached.
    Retreat {
        hp_percent: u8,
        after_ticks: u32,
    },
    /// Permanently change choices and release turning, push and Unison restrictions.
    HealthStage {
        hp_percent: u8,
        initial_station: EnemyStation,
        initial_actions: std::ops::Range<u8>,
        final_actions: std::ops::Range<u8>,
    },
    CarriedAnimationRate {
        slot: u8,
        rate: f32,
    },
    ReplaceWeapon {
        action: u8,
        hide_at: u32,
        replace_at: u32,
        source: u8,
        destination: u8,
        disabled_action: u8,
    },
    /// Show the selected attack's weapons and hide them on leaving the attack.
    ActionWeapons {
        groups: Vec<Vec<Arc<crate::WeaponDefinition>>>,
        actions: Vec<Option<u8>>,
        idle_group: Option<u8>,
        effect: crate::EffectAppearance,
        anchors: Vec<crate::MeleeAnchor>,
        drop_on_hurt: Option<(WeaponDrop, crate::MeleeAnchor)>,
    },
    ThreeStations {
        stations: [EnemyStation; 3],
        change_at: u32,
    },
    ModelParts {
        bone_scales: Vec<(u16, [f32; 3])>,
    },
    Transformation {
        reveal_hp_percent: u8,
        revealed_bones: Vec<u16>,
        airborne_hp_percent: u8,
        airborne_walk_speed: f32,
        airborne_motions: Vec<(u16, Option<u16>)>,
        hover_height: f32,
        initial_actions: std::ops::Range<u8>,
        final_actions: std::ops::Range<u8>,
    },
}

#[derive(Debug, Default)]
pub(crate) struct State {
    initialized: bool,
    station: usize,
    pub(crate) transformation: crate::enemy_transformation::State,
    pub(crate) health_stage_shifted: bool,
    pub(crate) weapon_replaced: bool,
    weapon_group: Option<u8>,
    dropped_weapon: Option<DroppedWeapon>,
    /// A fresh hurt pose can already have entered the knockdown controller.
    pub(crate) hurt_pending: bool,
}

#[derive(Debug)]
struct DroppedWeapon {
    world: Matrix,
    vertical_speed: f32,
}

impl PreparedBattle {
    pub(crate) fn validate_enemy_behaviors(&self) -> Result<()> {
        for setup in &self.resources.actor_setup {
            let Some(decision) = &setup.enemy_decision else {
                continue;
            };
            match &decision.behavior {
                Some(EnemyBehavior::Retreat {
                    hp_percent,
                    after_ticks,
                }) => {
                    ensure!(
                        *hp_percent <= 100 && *after_ticks > 0,
                        "invalid enemy retreat threshold"
                    );
                }
                Some(EnemyBehavior::HealthStage {
                    hp_percent,
                    initial_station,
                    initial_actions,
                    final_actions,
                }) => ensure!(
                    *hp_percent <= 100
                        && initial_station
                            .position
                            .iter()
                            .all(|value| value.is_finite())
                        && initial_station.heading.is_finite()
                        && !initial_actions.is_empty()
                        && !final_actions.is_empty()
                        && usize::from(initial_actions.end) <= decision.choices.len()
                        && usize::from(final_actions.end) <= decision.choices.len()
                        && (initial_actions.end <= final_actions.start
                            || final_actions.end <= initial_actions.start),
                    "invalid enemy health stage"
                ),
                Some(EnemyBehavior::CarriedAnimationRate { rate, .. }) => {
                    ensure!(
                        rate.is_finite() && *rate > 0.,
                        "invalid carried animation rate"
                    );
                }
                Some(EnemyBehavior::ReplaceWeapon {
                    action,
                    hide_at,
                    replace_at,
                    source,
                    destination,
                    disabled_action,
                }) => ensure!(
                    usize::from(*action) < decision.choices.len()
                        && usize::from(*disabled_action) < decision.choices.len()
                        && source != destination
                        && hide_at < replace_at,
                    "invalid enemy weapon replacement"
                ),
                Some(EnemyBehavior::ThreeStations {
                    stations,
                    change_at,
                }) => {
                    ensure!(
                        decision.choices.len() > STATION_ACTION
                            && *change_at > 0
                            && stations.iter().all(|station| {
                                station.position.iter().all(|value| value.is_finite())
                                    && station.heading.is_finite()
                            }),
                        "invalid enemy station behavior"
                    );
                }
                Some(EnemyBehavior::ActionWeapons {
                    groups,
                    actions,
                    idle_group,
                    anchors,
                    drop_on_hurt,
                    ..
                }) => ensure!(
                    actions.len() == decision.choices.len()
                        && !anchors.is_empty()
                        && drop_on_hurt.is_none_or(|(drop, _)| drop.slot < 2
                            && drop.hp_percent <= 100
                            && drop.upward_speed.is_finite()
                            && drop.upward_speed > 0.
                            && drop.gravity.is_finite()
                            && drop.gravity < 0.
                            && drop.pitch.is_finite())
                        && actions
                            .iter()
                            .chain(std::iter::once(idle_group))
                            .flatten()
                            .all(|group| usize::from(*group) < groups.len())
                        && groups.iter().all(|group| !group.is_empty()
                            && group.iter().all(|weapon| weapon.slot < 2)
                            && group
                                .iter()
                                .enumerate()
                                .all(|(index, weapon)| group[..index]
                                    .iter()
                                    .all(|other| other.slot != weapon.slot))),
                    "invalid enemy action weapon groups"
                ),
                Some(EnemyBehavior::ModelParts { bone_scales, .. }) => ensure!(
                    bone_scales.iter().all(|(_, scale)| scale
                        .iter()
                        .all(|value| value.is_finite() && *value >= 0.)),
                    "invalid enemy part scale"
                ),
                Some(EnemyBehavior::Transformation {
                    reveal_hp_percent,
                    airborne_hp_percent,
                    airborne_walk_speed,
                    hover_height,
                    initial_actions,
                    final_actions,
                    ..
                }) => ensure!(
                    *reveal_hp_percent <= 100
                        && airborne_hp_percent <= reveal_hp_percent
                        && airborne_walk_speed.is_finite()
                        && *airborne_walk_speed > 0.
                        && hover_height.is_finite()
                        && *hover_height >= 0.
                        && !initial_actions.is_empty()
                        && !final_actions.is_empty()
                        && usize::from(initial_actions.end) <= decision.choices.len()
                        && usize::from(final_actions.end) <= decision.choices.len()
                        && (initial_actions.end <= final_actions.start
                            || final_actions.end <= initial_actions.start),
                    "invalid enemy transformation"
                ),
                None => {}
            }
        }
        Ok(())
    }
}

impl Battle {
    pub(crate) fn advance_enemy_behavior(
        &mut self,
        index: usize,
        action_advanced: bool,
    ) -> Result<()> {
        let Some(decision) = &self.prepared.actor_setup[index].enemy_decision else {
            return Ok(());
        };
        let Some(behavior) = &decision.behavior else {
            return Ok(());
        };
        let combat = self.phase() == crate::BattlePhase::Combat;
        let runtime = &mut self.runtime[index];
        let hurt_pending = std::mem::take(&mut runtime.enemy_behavior.hurt_pending);
        match behavior {
            EnemyBehavior::Retreat { .. } => {}
            EnemyBehavior::HealthStage {
                initial_station, ..
            } => {
                if combat && !runtime.enemy_behavior.health_stage_shifted {
                    self.actors[index].position = initial_station.position;
                    self.actors[index].heading = initial_station.heading;
                }
            }
            EnemyBehavior::ActionWeapons { drop_on_hurt, .. } => {
                let Some((drop, anchor)) = drop_on_hurt else {
                    return Ok(());
                };
                let owner = ActorId(index as u8);
                let actor = &self.actors[index];
                if let Some(falling) = &mut runtime.enemy_behavior.dropped_weapon {
                    if falling.world[3][1] <= 0. {
                        return Ok(());
                    }
                    falling.world[3][1] = (falling.world[3][1] + falling.vertical_speed).max(0.);
                    falling.vertical_speed += drop.gravity;
                } else if actor.hp > 0
                    && actor.hp_percent() <= i16::from(drop.hp_percent)
                    && (hurt_pending
                        || matches!(runtime.task(), crate::state::ActorTask::Hurt { .. }))
                {
                    let position = self
                        .models
                        .as_ref()
                        .context("dropped weapon has no models")?
                        .anchor(owner, *anchor)?;
                    runtime.enemy_behavior.dropped_weapon = Some(DroppedWeapon {
                        world: glam::Mat4::from_rotation_translation(
                            glam::Quat::from_euler(
                                glam::EulerRot::YXZ,
                                actor.heading.to_radians(),
                                drop.pitch.to_radians(),
                                0.,
                            ),
                            glam::Vec3::from_array(position),
                        )
                        .to_cols_array_2d(),
                        vertical_speed: drop.upward_speed,
                    });
                }
                if let Some(falling) = &runtime.enemy_behavior.dropped_weapon {
                    self.model_requests.push(crate::ModelRequest::DropWeapon {
                        actor: owner,
                        slot: drop.slot,
                        world: falling.world,
                    });
                }
            }
            EnemyBehavior::CarriedAnimationRate { slot, rate } => {
                if !runtime.enemy_behavior.initialized {
                    self.model_requests
                        .push(crate::ModelRequest::WeaponAnimationRate {
                            actor: ActorId(index as u8),
                            slot: *slot,
                            rate: *rate,
                        });
                    runtime.enemy_behavior.initialized = true;
                }
            }
            EnemyBehavior::ReplaceWeapon {
                action,
                hide_at,
                replace_at,
                source,
                destination,
                ..
            } => {
                if runtime.enemy_behavior.weapon_replaced {
                    return Ok(());
                }
                let age = match runtime.task() {
                    crate::state::ActorTask::Action { sequence, .. }
                        if sequence.action == decision.choices[usize::from(*action)].action
                            && sequence.activity() == Activity::Action =>
                    {
                        sequence.age
                    }
                    _ => return Ok(()),
                };
                let actor = ActorId(index as u8);
                if age == *hide_at {
                    self.model_requests.push(crate::ModelRequest::Weapon {
                        actor,
                        slot: *destination,
                        visible: false,
                    });
                }
                if age == *replace_at {
                    self.model_requests.push(crate::ModelRequest::MoveWeapon {
                        actor,
                        source: *source,
                        destination: *destination,
                    });
                    runtime.enemy_behavior.weapon_replaced = true;
                }
            }
            EnemyBehavior::ModelParts { bone_scales } => {
                if !runtime.enemy_behavior.initialized {
                    let actor = ActorId(index as u8);
                    self.model_requests.push(crate::ModelRequest::BoneScales {
                        actor,
                        scales: bone_scales.clone(),
                    });
                    runtime.enemy_behavior.initialized = true;
                }
            }
            EnemyBehavior::Transformation { revealed_bones, .. } => {
                if !runtime.enemy_behavior.initialized {
                    self.model_requests.push(crate::ModelRequest::BoneScales {
                        actor: ActorId(index as u8),
                        scales: revealed_bones.iter().map(|&bone| (bone, [0.; 3])).collect(),
                    });
                    runtime.enemy_behavior.initialized = true;
                }
            }
            EnemyBehavior::ThreeStations {
                stations,
                change_at,
            } => {
                if !runtime.enemy_behavior.initialized {
                    self.actors[index].movement.steering.unrestricted_arena = true;
                    runtime.enemy_behavior.initialized = true;
                    return Ok(());
                }
                if self.update.is_multiple_of(STATION_REARM_TICKS) {
                    runtime
                        .used_enemy_choices
                        .retain(|&row| row != STATION_ACTION);
                }
                let changing = match runtime.task() {
                    crate::state::ActorTask::Action { sequence, .. } => {
                        sequence.action == decision.choices[STATION_ACTION].action
                            && sequence.activity() == Activity::Action
                            // Common behavior sees the completed action tick, whose
                            // contact-owned transition commits later in this update.
                            && sequence.age + u32::from(action_advanced) == *change_at
                    }
                    _ => false,
                };
                // A held attack tick keeps the station transition active through hit stop.
                if changing {
                    runtime.enemy_behavior.station =
                        (runtime.enemy_behavior.station + 1) % stations.len();
                }
                let station = stations[runtime.enemy_behavior.station];
                self.actors[index].position = station.position;
                self.actors[index].heading = station.heading;
            }
        }
        Ok(())
    }

    pub(crate) fn advance_action_weapons(&mut self, cues: &mut Vec<crate::Cue>) -> Result<()> {
        for (index, setup) in self.prepared.actor_setup.iter().enumerate() {
            let Some(decision) = &setup.enemy_decision else {
                continue;
            };
            let Some(EnemyBehavior::ActionWeapons {
                groups,
                actions,
                idle_group,
                effect,
                anchors,
                ..
            }) = &decision.behavior
            else {
                continue;
            };
            let runtime = &mut self.runtime[index];
            let group = match runtime.task() {
                crate::state::ActorTask::Action { sequence, .. }
                    if sequence.activity() == Activity::Action =>
                {
                    // Age already advanced for this update; admission keeps the old weapons.
                    if sequence.age <= 1 || sequence.contact_blend_ends.is_some() {
                        continue;
                    }
                    decision
                        .choices
                        .iter()
                        .position(|choice| choice.action == sequence.action)
                        .and_then(|row| actions[row])
                }
                _ => *idle_group,
            };
            if runtime.enemy_behavior.weapon_group == group {
                continue;
            }
            let owner = ActorId(index as u8);
            let actor = &self.actors[index];
            let models = self
                .models
                .as_mut()
                .context("action weapons have no models")?;
            let origins = if group.is_some() && group == *idle_group {
                Vec::new()
            } else {
                anchors
                    .iter()
                    .map(|&anchor| models.anchor(owner, anchor))
                    .collect::<Result<Vec<_>>>()?
            };
            models.select_primary_weapons(
                owner,
                group.map_or(&[], |group| groups[usize::from(group)].as_slice()),
            )?;
            runtime.enemy_behavior.weapon_group = group;
            for origin in origins {
                cues.push(crate::Cue::InitializedEffect(crate::EffectRequest {
                    owner,
                    target: owner,
                    appearance: *effect,
                    origin,
                    heading: actor.heading,
                    follow: None,
                    scale: actor.effect_scale,
                    tint: Default::default(),
                }));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionKey, ActionRequest, BattleClock, BattleInput, Models, Side};
    use resonance_content::animation::{Bone, Skeleton, Transform, TransformChannels};
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn station_change_uses_the_completed_action_tick_and_holds_through_hit_stop() -> Result<()> {
        let owner = ActorId(1);
        let mut prepared = crate::tests::prepared(
            vec![
                crate::tests::actor(Side::Party),
                crate::tests::actor(Side::Enemy),
            ],
            20,
        );
        prepared.actors[0].position = [-300., 0., 0.];
        let second = prepared.resources.actions.insert(crate::tests::action(20));
        let changing = prepared.resources.actions.insert(crate::tests::action(20));
        for action in [ActionKey(0), second, changing] {
            crate::tests::assign_action(&mut prepared, owner.index(), action);
        }
        let stations = [100., 200., 300.].map(|x| EnemyStation {
            position: [x, 0., 0.],
            heading: 0.,
        });
        prepared.resources.actor_setup[owner.index()]
            .enemy_decision
            .as_mut()
            .unwrap()
            .behavior = Some(EnemyBehavior::ThreeStations {
            stations,
            change_at: 3,
        });
        let mut battle = prepared.finish()?;
        battle.step(BattleInput::default())?;
        battle.start_admitted_action(
            ActionRequest {
                actor: owner,
                action: changing,
                target: ActorId(0),
            },
            &mut Vec::new(),
        )?;
        let step = |battle: &mut Battle, age: u32, station: usize| -> Result<()> {
            battle.step(BattleInput::default())?;
            let (_, sequence) = battle.runtime[owner.index()].task().action().unwrap();
            assert_eq!(sequence.age, age);
            assert_eq!(
                battle.actors[owner.index()].position,
                stations[station].position
            );
            Ok(())
        };
        step(&mut battle, 1, 0)?;
        step(&mut battle, 2, 0)?;
        battle.actors[owner.index()].hit_stop = 2;
        step(&mut battle, 2, 0)?;
        step(&mut battle, 2, 0)?;
        step(&mut battle, 3, 1)?;
        battle.actors[owner.index()].hit_stop = 2;
        step(&mut battle, 3, 2)?;
        step(&mut battle, 3, 0)?;
        step(&mut battle, 4, 0)?;
        Ok(())
    }

    #[test]
    fn broken_weapon_moves_the_spare_and_keeps_its_contacts_for_the_next_attack() -> Result<()> {
        let owner = ActorId(1);
        let mut prepared = crate::tests::prepared(
            vec![
                crate::tests::actor(Side::Party),
                crate::tests::actor(Side::Enemy),
            ],
            20,
        );
        let next = prepared.resources.actions.insert(crate::tests::action(20));
        let mut break_action = crate::tests::action(173);
        crate::tests::attack_mut(&mut break_action).recovery = 30;
        let breaking = prepared.resources.actions.insert(break_action);
        for action in [ActionKey(0), next, breaking] {
            crate::tests::assign_action(&mut prepared, owner.index(), action);
        }
        prepared.resources.actor_setup[owner.index()]
            .enemy_decision
            .as_mut()
            .unwrap()
            .behavior = Some(EnemyBehavior::ReplaceWeapon {
            action: 2,
            hide_at: 57,
            replace_at: 158,
            source: 1,
            destination: 0,
            disabled_action: 0,
        });
        let mut battle = prepared.finish()?;
        let bone = |name: &str, x| Bone {
            name: name.into(),
            parent: None,
            bind_channels: TransformChannels(8),
            bind: Transform {
                translation: [x, 0., 0.],
                ..Default::default()
            },
        };
        let mut model = crate::tests::model_definition(7, [(0, 100.)]);
        model.skeleton.bones.push(bone("hand", 20.));
        for (slot, resource, attachment, bones) in [
            (0, 8, 1, vec![bone("at00_near", 10.), bone("at00_far", 20.)]),
            (
                1,
                9,
                0,
                vec![
                    bone("root", 0.),
                    bone("at00_tip", 70.),
                    bone("at00_tip", 120.),
                ],
            ),
        ] {
            model.weapons.push(Arc::new(crate::WeaponDefinition::new(
                slot,
                attachment,
                vec![crate::WeaponLayerDefinition {
                    resource,
                    skeleton: Skeleton { bones },
                    motions: BTreeMap::new(),
                    playback: crate::WeaponPlayback::Rigid,
                    secondary_motion: vec![],
                    texture_animation: None,
                }],
                vec![],
            )?));
        }
        let anchors = model.contact_group_anchors(0);
        assert_eq!(anchors.len(), 2);
        let anchor = anchors[0];
        battle.set_models(Models::new(
            battle.actors(),
            vec![None, Some(Arc::new(model))],
            BTreeMap::new(),
            resonance_content::diagnostics::Diagnostics::new(true),
        )?);
        battle.start_admitted_action(
            ActionRequest {
                actor: owner,
                action: breaking,
                target: ActorId(0),
            },
            &mut Vec::new(),
        )?;
        let step = |battle: &mut Battle| -> Result<crate::BattleFrame> {
            let mut frame = battle.step(BattleInput::default())?;
            battle.advance_models(&mut frame, BattleClock::Running, false)?;
            Ok(frame)
        };
        for age in 0..=240 {
            let frame = step(&mut battle)?;
            if age < 158 {
                assert_eq!(frame.weapons.len(), 2);
                assert_eq!(frame.weapons[0].resource, 8);
                assert_eq!(frame.weapons[0].visible, age < 57);
                assert!(frame.weapons[1].visible);
                assert!(battle.enemy_behavior_allows_choice(owner, 0));
            } else {
                assert_eq!(frame.weapons.len(), 1);
                let replacement = &frame.weapons[0];
                assert_eq!((replacement.slot, replacement.resource), (0, 9));
                assert!(replacement.visible);
                assert_eq!(
                    replacement.world,
                    resonance_content::animation::multiply(
                        frame.models[0].world,
                        frame.models[0].bones[1],
                    ),
                );
                let expected =
                    resonance_content::animation::transform_point(replacement.world, [70., 0., 0.]);
                assert_eq!(battle.models().unwrap().anchor(owner, anchor)?, expected);
                assert_eq!(
                    battle.models().unwrap().anchor(owner, anchors[1])?,
                    resonance_content::animation::transform_point(
                        replacement.world,
                        [120., 0., 0.]
                    ),
                );
                assert!(!battle.enemy_behavior_allows_choice(owner, 0));
            }
            if age >= 173 {
                if battle.activity(owner) == Activity::Idle {
                    break;
                }
                assert_eq!(battle.activity(owner), Activity::Recovering);
            }
        }
        assert_eq!(battle.activity(owner), Activity::Idle);
        battle.start_admitted_action(
            ActionRequest {
                actor: owner,
                action: next,
                target: ActorId(0),
            },
            &mut Vec::new(),
        )?;
        for _ in 0..=20 {
            let mut frame = step(&mut battle)?;
            assert_eq!(frame.weapons.len(), 1);
            assert_eq!((frame.weapons[0].slot, frame.weapons[0].resource), (0, 9));
            assert!(frame.weapons[0].visible);
            let expected = resonance_content::animation::transform_point(
                frame.weapons[0].world,
                [70., 0., 0.],
            );
            assert_eq!(battle.models().unwrap().anchor(owner, anchor)?, expected);
            assert_eq!(
                battle.models().unwrap().anchor(owner, anchors[1])?,
                resonance_content::animation::transform_point(
                    frame.weapons[0].world,
                    [120., 0., 0.]
                ),
            );
            let weapons = frame.weapons.clone();
            battle.advance_models(&mut frame, BattleClock::Held, false)?;
            assert_eq!(frame.weapons, weapons);
        }
        Ok(())
    }
}
