//! Persistent enemy behavior that is independent of the selected attack.
use crate::{Activity, ActorId, Battle, PreparedBattle};
use anyhow::{Result, ensure};
use resonance_content::battle_action::EnemyStation;

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
        initial_actions: std::ops::Range<u8>,
        final_actions: std::ops::Range<u8>,
    },
    CarriedAnimationRate {
        slot: u8,
        rate: f32,
    },
    ThreeStations {
        stations: [EnemyStation; 3],
        change_at: u32,
    },
    ModelParts {
        bone_scales: Vec<(u16, [f32; 3])>,
        hidden_weapons: Vec<u8>,
    },
    Transformation {
        reveal_hp_percent: u8,
        revealed_bones: Vec<u16>,
        airborne_hp_percent: u8,
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
                    initial_actions,
                    final_actions,
                }) => ensure!(
                    *hp_percent <= 100
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
                Some(EnemyBehavior::ModelParts { bone_scales, .. }) => ensure!(
                    bone_scales.iter().all(|(_, scale)| scale
                        .iter()
                        .all(|value| value.is_finite() && *value >= 0.)),
                    "invalid enemy part scale"
                ),
                Some(EnemyBehavior::Transformation {
                    reveal_hp_percent,
                    airborne_hp_percent,
                    hover_height,
                    initial_actions,
                    final_actions,
                    ..
                }) => ensure!(
                    *reveal_hp_percent <= 100
                        && airborne_hp_percent <= reveal_hp_percent
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
    pub(crate) fn advance_enemy_behavior(&mut self, index: usize) {
        let Some(decision) = &self.prepared.actor_setup[index].enemy_decision else {
            return;
        };
        let Some(behavior) = &decision.behavior else {
            return;
        };
        let runtime = &mut self.runtime[index];
        match behavior {
            EnemyBehavior::Retreat { .. } | EnemyBehavior::HealthStage { .. } => {}
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
            EnemyBehavior::ModelParts {
                bone_scales,
                hidden_weapons,
            } => {
                if !runtime.enemy_behavior.initialized {
                    let actor = ActorId(index as u8);
                    self.model_requests.push(crate::ModelRequest::BoneScales {
                        actor,
                        scales: bone_scales.clone(),
                    });
                    self.model_requests
                        .extend(
                            hidden_weapons
                                .iter()
                                .map(|&slot| crate::ModelRequest::Weapon {
                                    actor,
                                    slot,
                                    visible: false,
                                }),
                        );
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
                    return;
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
                            && sequence.age == *change_at
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
    }
}
