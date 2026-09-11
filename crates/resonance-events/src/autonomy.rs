//! Ambient actor decisions. Scripted destinations take priority over wandering.
use crate::Actor;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Behavior {
    Stationary = 0,
    Wander = 1,
    WanderNearHome = 2,
    WatchPlayer = 4,
    ApproachPlayer = 5,
    Player = 10,
    ChasePlayer = 12,
}
impl TryFrom<i32> for Behavior {
    type Error = anyhow::Error;
    fn try_from(value: i32) -> Result<Self> {
        Ok(match value {
            0 => Self::Stationary,
            1 => Self::Wander,
            2 => Self::WanderNearHome,
            4 => Self::WatchPlayer,
            5 => Self::ApproachPlayer,
            10 => Self::Player,
            12 => Self::ChasePlayer,
            _ => bail!("actor movement behavior {value} is not implemented"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Select,
    Idle,
    Walk,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Autonomy {
    pub behavior: Behavior,
    pub speed: f32,
    pub home: [f32; 3],
    pub radius: f32,
    pub activity: Activity,
    pub remaining: i32,
    pub initialized: bool,
    pub floor_available: bool,
    #[serde(default)]
    pub conversing: bool,
}
impl Autonomy {
    pub fn new(behavior: Behavior, speed: f32, home: [f32; 3]) -> Self {
        Self {
            behavior,
            speed,
            home,
            radius: 600.,
            activity: Activity::Select,
            remaining: 0,
            initialized: false,
            floor_available: true,
            conversing: false,
        }
    }
    fn select(&mut self, activity: Activity) {
        self.activity = activity;
        self.initialized = false;
    }
    pub fn begin_conversation(&mut self) {
        self.conversing = true;
        self.select(Activity::Idle);
    }
    /// A rejected floor probe requests a new direction on the next update.
    pub fn resolve_floor(&mut self, available: bool) {
        self.floor_available = available;
        if !available {
            self.remaining = -1;
        }
    }
}

#[derive(Default)]
pub(crate) struct AmbientMotion {
    pub walking: bool,
    pub paused: bool,
}
impl Actor {
    pub(crate) fn step_autonomy(
        &mut self,
        free_control: bool,
        player: Option<[f32; 3]>,
        random: &mut impl FnMut() -> u32,
    ) -> AmbientMotion {
        let mut intent = AmbientMotion::default();
        let Some(ai) = &mut self.autonomy else {
            return intent;
        };
        if self.motion.is_some() {
            ai.select(Activity::Select);
            return intent;
        }
        if ai.conversing && free_control {
            ai.conversing = false;
            ai.select(Activity::Select);
        }
        if ai.activity == Activity::Select {
            ai.select(match ai.behavior {
                Behavior::Stationary | Behavior::Player => Activity::Idle,
                Behavior::WatchPlayer => {
                    if let Some(player) = player {
                        self.target_heading = heading(self.position, player);
                    }
                    Activity::Idle
                }
                _ => Activity::Walk,
            });
            // Player input selects the idle action before the actor update.
            if ai.behavior != Behavior::Player {
                return intent;
            }
        }
        match ai.activity {
            Activity::Idle => {
                if !ai.initialized {
                    ai.remaining = (random() & 63) as i32 + 120;
                    if ai.behavior == Behavior::ChasePlayer {
                        ai.remaining = (random() & 63) as i32 + 10;
                    }
                    ai.initialized = true;
                }
                ai.remaining -= 1;
                if ai.remaining < -1 && !ai.conversing {
                    ai.select(Activity::Select);
                }
            }
            Activity::Walk => {
                if !ai.initialized {
                    ai.remaining = 0;
                    ai.initialized = true;
                }
                intent.walking = true;
                intent.paused = !free_control;
                if intent.paused {
                    return intent;
                }
                ai.remaining -= 1;
                if ai.remaining < -1 {
                    if random() & 15 == 0 {
                        ai.select(Activity::Idle);
                        return intent;
                    }
                    ai.remaining = (random() & 63) as i32 + 32;
                    if !ai.floor_available {
                        self.target_heading += if ai.remaining & 1 != 0 { 90. } else { -90. };
                        ai.remaining = (random() & 31) as i32 + 8;
                    } else {
                        self.target_heading += (random() & 63) as f32 - 32.;
                        match ai.behavior {
                            Behavior::WanderNearHome
                                if self
                                    .position
                                    .iter()
                                    .zip(ai.home)
                                    .map(|(a, b)| (a - b).powi(2))
                                    .sum::<f32>()
                                    > ai.radius * ai.radius =>
                            {
                                ai.remaining = 60;
                                self.target_heading = heading(self.position, ai.home);
                            }
                            Behavior::ApproachPlayer | Behavior::ChasePlayer => {
                                if let Some(player) = player {
                                    self.target_heading = heading(self.position, player);
                                }
                                if ai.behavior == Behavior::ApproachPlayer {
                                    self.target_heading += (random() & 63) as f32 - 32.;
                                } else {
                                    ai.remaining = (random() & 15) as i32;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let angle = self.target_heading.to_radians();
                let mut delta = [angle.sin() * ai.speed, -angle.cos() * ai.speed];
                if self.heading.trunc() != self.target_heading.trunc() {
                    delta = delta.map(|v| (f64::from(v) / 1.5) as f32);
                }
                for (p, delta) in self.position.iter_mut().zip(delta) {
                    *p += delta;
                }
            }
            Activity::Select => unreachable!(),
        }
        intent
    }
}

fn heading(from: [f32; 3], to: [f32; 3]) -> f32 {
    (to[0] - from[0]).atan2(from[1] - to[1]).to_degrees()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorOrigin {
    pub autonomy: Autonomy,
    pub position: [f32; 3],
    pub heading: f32,
    pub target_heading: f32,
    pub animation_slot: Option<u16>,
    pub animation_sample: f32,
    pub animation_repeat: bool,
}
