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
    FollowPath = 3,
    RandomPath = 11,
    WatchPlayer = 4,
    ApproachPlayer = 5,
    Player = 10,
    ChasePlayer = 12,
}
impl Behavior {
    pub(crate) fn enemy(mode: u8) -> Self {
        match mode {
            0 => Self::WanderNearHome,
            1 | 2 => Self::Wander,
            3 => Self::FollowPath,
            4 | 5 => Self::ApproachPlayer,
            6 => Self::RandomPath,
            _ => Self::Wander,
        }
    }
}
impl TryFrom<i32> for Behavior {
    type Error = anyhow::Error;
    fn try_from(value: i32) -> Result<Self> {
        Ok(match value {
            0 => Self::Stationary,
            1 => Self::Wander,
            2 => Self::WanderNearHome,
            3 => Self::FollowPath,
            11 => Self::RandomPath,
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
    }
    pub(crate) fn set_behavior(&mut self, behavior: Behavior) {
        self.behavior = behavior;
        self.conversing = false;
        self.select(Activity::Select);
    }
    /// A rejected floor probe requests a new decision on the next update.
    pub fn resolve_floor(&mut self, available: bool) {
        self.floor_available = available;
        if !available && !self.conversing {
            self.remaining = -1;
        }
    }
}

/// Native actors retain up to twelve patrol destinations, independent of AI state.
#[derive(Debug, Clone, Default)]
pub struct Path {
    pub points: [[f32; 3]; 12],
    pub count: u8,
    pub next: u8,
    pub reverse_at_end: bool,
    pub reverse: bool,
}

#[derive(Default)]
pub(crate) struct AmbientMotion {
    pub walking: bool,
    pub paused: bool,
    pub selecting: bool,
}
impl Actor {
    pub(crate) fn step_autonomy(
        &mut self,
        free_control: bool,
        conversation_active: bool,
        player: Option<[f32; 3]>,
        random: &mut impl FnMut() -> u32,
    ) -> AmbientMotion {
        let mut intent = AmbientMotion::default();
        if self.pushable {
            return intent;
        }
        let Some(ai) = &mut self.autonomy else {
            return intent;
        };
        if let Some(enemy) = &mut self.enemy {
            if enemy.pause_ticks != 0 && self.motion.is_none() {
                if free_control && enemy.pause_ticks > 0 {
                    enemy.pause_ticks -= 1;
                }
                intent.paused = true;
                return intent;
            }
            let alert = enemy.alerted;
            ai.speed = if alert {
                enemy.alert_speed
            } else {
                enemy.normal_speed
            };
            if alert
                && enemy.chase_on_sight
                && let Some(p) = player
            {
                self.target_heading = heading(self.position, p);
            }
        }
        if self.motion.is_some() {
            ai.conversing = false;
            ai.select(Activity::Select);
            return intent;
        }
        if ai.conversing {
            if !conversation_active {
                ai.conversing = false;
                ai.select(Activity::Select);
            }
            // Conversation leaves the decision timer alone. Resume through a
            // separate selection update before initializing another activity.
            return intent;
        }
        if matches!(ai.behavior, Behavior::FollowPath | Behavior::RandomPath) {
            let path = &mut self.path;
            if path.count == 0 {
                return intent;
            }
            intent.paused = !free_control;
            if intent.paused {
                return intent;
            }
            path.next = path.next.min(path.count - 1);
            let target = path.points[usize::from(path.next)];
            let delta: [f32; 3] = std::array::from_fn(|i| {
                if i == 2 && self.grounded {
                    0.
                } else {
                    target[i] - self.position[i]
                }
            });
            let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
            if distance < 1. {
                if path.count == 1 {
                    return intent;
                }
                if ai.behavior == Behavior::RandomPath {
                    path.next = (random() % u32::from(path.count)) as u8;
                } else if path.reverse {
                    if path.next == 0 {
                        path.reverse = false;
                    } else {
                        path.next -= 1;
                    }
                } else if path.next + 1 < path.count {
                    path.next += 1;
                } else if path.reverse_at_end {
                    path.reverse = true;
                } else {
                    path.next = 0;
                }
                return intent;
            }
            self.target_heading = heading(self.position, target);
            intent.walking = true;
            let fraction = (ai.speed / distance).min(1.);
            for (position, delta) in self.position.iter_mut().zip(delta) {
                *position += delta * fraction;
            }
            return intent;
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
            // Free player input selects idle before the actor update. Events
            // leave selection and initialization to separate actor updates.
            if ai.behavior != Behavior::Player || !free_control {
                intent.selecting = true;
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
                if ai.remaining < -1 {
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
                let outside_home = ai.behavior == Behavior::WanderNearHome
                    && (ai.radius < 0.
                        || self
                            .position
                            .iter()
                            .zip(ai.home)
                            .map(|(a, b)| (a - b).powi(2))
                            .sum::<f32>()
                            > ai.radius * ai.radius);
                if self.enemy.is_some() && outside_home {
                    ai.remaining = 120;
                    self.target_heading = heading(self.position, ai.home);
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
                        let mask = if self
                            .enemy
                            .as_ref()
                            .is_some_and(|enemy| enemy.behavior == 1 && enemy.random_turns == 0)
                        {
                            127
                        } else {
                            63
                        };
                        self.target_heading += (random() & mask) as f32 - (mask / 2 + 1) as f32;
                        match ai.behavior {
                            Behavior::WanderNearHome if outside_home => {
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
                let angle = if self.enemy.is_some() {
                    self.heading
                } else {
                    self.target_heading
                }
                .to_radians();
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

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn enemy_returns_home_before_its_random_turn_timer_expires() {
        let mut actor = Actor::new(164, [301., 0., 0.]);
        actor.face(90.);
        actor.turn_speed = 10.;
        let mut ai = Autonomy::new(Behavior::WanderNearHome, 3., [0.; 3]);
        ai.radius = 300.;
        ai.activity = Activity::Walk;
        ai.initialized = true;
        ai.remaining = 500;
        actor.autonomy = Some(ai);
        actor.enemy = Some(crate::world::Enemy {
            event: 0,
            behavior: 0,
            normal_speed: 3.,
            alert_speed: 6.,
            random_turns: 0,
            chase_on_sight: false,
            sight_angle: 90.,
            sight_distance: 600.,
            alerted: false,
            event_parameters: [0; 2],
            pause_ticks: 0,
            reaction: crate::effect::StunEffect::None,
        });
        let mut seed = 1;
        let mut random = || crate::world::random(&mut seed);
        for _ in 0..120 {
            actor.step_autonomy(true, false, None, &mut random);
            actor.step_heading(false, true);
        }
        assert!(actor.position[0].hypot(actor.position[1]) < 300.);
        let before = actor.position;
        actor.step_autonomy(false, false, None, &mut random);
        assert_eq!(actor.position, before);
        actor.enemy.as_mut().unwrap().pause_ticks = 60;
        for _ in 0..120 {
            actor.step_autonomy(false, false, None, &mut random);
        }
        assert_eq!(actor.position, before);
        assert_eq!(actor.enemy.as_ref().unwrap().pause_ticks, 60);
        actor.step_autonomy(true, false, None, &mut random);
        assert_eq!(actor.enemy.as_ref().unwrap().pause_ticks, 59);
    }

    #[test]
    fn patrol_reverses_at_end_and_pauses_for_dialogue() {
        let mut actor = Actor::new(1, [0.; 3]);
        actor.autonomy = Some(Autonomy::new(Behavior::FollowPath, 2., actor.position));
        actor.path.count = 2;
        actor.path.points[0] = [4., 0., 0.];
        actor.path.points[1] = [8., 0., 0.];
        actor.path.reverse_at_end = true;
        let mut random = || panic!("a fixed patrol must not consume random state");
        actor.step_autonomy(true, false, None, &mut random);
        assert_eq!(actor.position[0], 2.);
        actor.step_autonomy(false, true, None, &mut random);
        assert_eq!(actor.position[0], 2.);
        for _ in 0..8 {
            actor.step_autonomy(true, false, None, &mut random);
        }
        assert!(actor.path.reverse);
        assert_eq!(actor.position[0], 4.);
    }
}
