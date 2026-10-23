use super::{Activity, AmbientMotion, Path, heading};
use crate::Actor;

const LONG_TURN_INTERVAL: i32 = 120;
const IDLE_TICKS: i32 = 180;
const FLOOR_LOOKAHEAD: f32 = 200.;
const AVOIDANCE_ANGLE: f32 = 45.;

pub(super) fn step(
    actor: &mut Actor,
    mode: u8,
    free_control: bool,
    player: Option<[f32; 3]>,
    random: &mut impl FnMut() -> u32,
    floor: &mut impl FnMut([f32; 3]) -> bool,
    speed_multiplier: f32,
) -> AmbientMotion {
    let mut intent = AmbientMotion::default();
    if !free_control {
        intent.paused = true;
        intent.selecting = true;
        return intent;
    }
    let Some(enemy) = actor.enemy.as_ref() else {
        return intent;
    };
    let ai = actor.autonomy.as_mut().unwrap();
    if ai.activity == Activity::Idle {
        if !ai.initialized {
            ai.remaining = IDLE_TICKS;
            ai.initialized = true;
        }
        let finished = ai.remaining < 0 || enemy.alerted;
        ai.remaining -= 1;
        if finished {
            ai.select(Activity::Walk);
        }
        return intent;
    }
    if !ai.initialized {
        ai.activity = Activity::Walk;
        ai.remaining = if mode == 0 {
            LONG_TURN_INTERVAL + (random() & 511) as i32
        } else {
            4
        };
        ai.initialized = true;
        if matches!(mode, 3 | 6) {
            actor.path.next = 0;
            actor.path.reverse = false;
        }
    }
    ai.speed = enemy.normal_speed * speed_multiplier;
    let toward_player = player.map(|position| heading(actor.position, position));
    let alert_speed = enemy.alert_speed * speed_multiplier;
    if matches!(mode, 3 | 6) {
        if mode == 3 && enemy.alerted && enemy.alert_response == 1 {
            if let Some(heading) = toward_player {
                actor.target_heading = heading;
            }
            intent.walking = true;
            intent.paused = true;
        } else {
            intent.walking = patrol(
                &mut actor.position,
                actor.heading,
                &mut actor.target_heading,
                &mut actor.path,
                ai.speed,
                mode == 6,
                random,
            );
        }
        return intent;
    }

    match mode {
        0..=2 => {
            if enemy.alerted {
                ai.speed = alert_speed;
                if (mode == 0 || enemy.alert_response != 0)
                    && let Some(heading) = toward_player
                {
                    actor.target_heading = heading;
                    ai.remaining = LONG_TURN_INTERVAL;
                }
            }
            if mode == 0 && (ai.radius < 0. || distance(actor.position, ai.home) > ai.radius) {
                actor.target_heading = heading(actor.position, ai.home);
                ai.remaining = LONG_TURN_INTERVAL;
            }
        }
        4 => {
            ai.speed = 0.;
            if enemy.alerted {
                if matches!(enemy.alert_response, 0 | 2)
                    && let Some(heading) = toward_player
                {
                    actor.target_heading = heading;
                }
                if enemy.alert_response == 2 {
                    ai.speed = alert_speed;
                }
                ai.remaining = LONG_TURN_INTERVAL;
            }
        }
        5 => {
            if let Some(player) = player
                && distance(actor.position, player) < enemy.sight_distance
            {
                actor.target_heading = heading(player, actor.position);
                ai.speed = alert_speed;
                ai.remaining = 60 + (random() & 31) as i32;
            }
        }
        _ => {}
    }

    if mode != 4 || ai.speed > 0. {
        avoid_edge(
            actor.position,
            &mut actor.target_heading,
            &mut ai.remaining,
            ai.floor_available,
            random,
            floor,
        );
    }
    let choose_direction = ai.remaining <= 0;
    ai.remaining -= 1;
    if choose_direction {
        if matches!(mode, 0 | 2) {
            ai.remaining = LONG_TURN_INTERVAL + (random() & 511) as i32;
        }
        match mode {
            0 => actor.target_heading += 10. + (random() & 63) as f32,
            1 => {
                if let Some(heading) = toward_player {
                    actor.target_heading = heading;
                }
                if enemy.random_turns != 0 {
                    actor.target_heading += (random() & 15) as f32 - 7.;
                    ai.remaining = LONG_TURN_INTERVAL + (random() & 31) as i32;
                } else {
                    actor.target_heading += (random() & 63) as f32 - 32.;
                }
            }
            2 => {
                let mask = if enemy.random_turns != 0 { 63 } else { 127 };
                actor.target_heading += (random() & mask) as f32 - (mask / 2 + 1) as f32;
            }
            4 => {
                if enemy.random_turns != 0 {
                    actor.target_heading += (random() & 127) as f32 - 64.;
                }
                ai.remaining = LONG_TURN_INTERVAL + (random() & 511) as i32;
            }
            5 if enemy.random_turns != 0 => {
                ai.speed = 0.;
                ai.remaining = LONG_TURN_INTERVAL + (random() & 511) as i32;
            }
            5 => {
                actor.target_heading += (random() & 63) as f32 - 32.;
                ai.remaining = LONG_TURN_INTERVAL + (random() & 31) as i32;
            }
            _ => {}
        }
        if (mode == 0 || mode == 2 || mode == 1 && enemy.random_turns == 0) && random() & 15 == 15 {
            ai.select(Activity::Idle);
        }
        if mode == 1 && enemy.random_turns == 0 {
            ai.remaining = LONG_TURN_INTERVAL + (random() & 511) as i32;
        }
    }
    intent.walking = mode != 4 || ai.speed > 0.;
    let speed = ai.speed
        / if turning(actor.heading, actor.target_heading) {
            1.5
        } else {
            1.
        };
    actor.position = ahead(actor.position, actor.heading, speed);
    intent
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}

fn turning(current: f32, target: f32) -> bool {
    current.rem_euclid(360.).trunc() != target.rem_euclid(360.).trunc()
}

fn ahead(mut position: [f32; 3], heading: f32, distance: f32) -> [f32; 3] {
    let angle = heading.to_radians();
    position[0] += angle.sin() * distance;
    position[1] -= angle.cos() * distance;
    position
}

fn avoid_edge(
    position: [f32; 3],
    heading: &mut f32,
    timer: &mut i32,
    standing_on_floor: bool,
    random: &mut impl FnMut() -> u32,
    floor: &mut impl FnMut([f32; 3]) -> bool,
) {
    if floor(ahead(position, *heading, FLOOR_LOOKAHEAD)) && standing_on_floor {
        return;
    }
    let turn = if random() & 1 == 0 {
        -AVOIDANCE_ANGLE
    } else {
        AVOIDANCE_ANGLE
    };
    *timer = 60 + (random() & 63) as i32;
    *heading += turn;
    if !floor(ahead(position, *heading, FLOOR_LOOKAHEAD)) {
        *heading -= turn * 2.;
    }
}

fn patrol(
    position: &mut [f32; 3],
    heading_now: f32,
    target_heading: &mut f32,
    path: &mut Path,
    speed: f32,
    random_path: bool,
    random: &mut impl FnMut() -> u32,
) -> bool {
    if path.count == 0 {
        return false;
    }
    path.next = path.next.min(path.count - 1);
    let target = path.points[usize::from(path.next)];
    *target_heading = heading(*position, target);
    let delta = [target[0] - position[0], target[1] - position[1]];
    let distance = delta[0].hypot(delta[1]);
    if distance < 1. {
        if random_path {
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
    } else {
        let fraction = speed.min(distance) / distance
            * if turning(heading_now, *target_heading) {
                0.5
            } else {
                1.
            };
        position[0] += delta[0] * fraction;
        position[1] += delta[1] * fraction;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Autonomy, Behavior};

    fn actor(mode: u8) -> Actor {
        let mut actor = Actor::new(164, [0.; 3]);
        actor.autonomy = Some(Autonomy::new(Behavior::enemy(mode), 3., actor.position));
        actor.enemy = Some(crate::world::Enemy {
            event: 0,
            behavior: mode,
            normal_speed: 3.,
            alert_speed: 6.,
            random_turns: 0,
            alert_response: 0,
            sight_angle: 90.,
            sight_distance: 600.,
            alerted: false,
            event_parameters: [0; 2],
            pause_ticks: 0,
            pause_outside_view: false,
            script_flag: false,
            reaction: crate::effect::StunEffect::None,
        });
        actor
    }

    fn update(actor: &mut Actor, player: [f32; 3]) -> AmbientMotion {
        let intent = actor.step_autonomy(true, false, Some(player), &mut || 0, &mut |_| true, 1.);
        actor.step_heading(false, intent.walking);
        intent
    }

    #[test]
    fn guard_responses_and_flee_have_distinct_movement() {
        let player = [100., 0., 0.];
        let mut guard = actor(4);
        guard.enemy.as_mut().unwrap().alerted = true;
        update(&mut guard, player);
        assert_eq!(guard.position, [0.; 3]);
        assert_eq!(guard.target_heading, 90.);
        guard.face(180.);
        guard.enemy.as_mut().unwrap().alert_response = 1;
        update(&mut guard, player);
        assert_eq!(guard.target_heading, 180.);
        assert_eq!(guard.position, [0.; 3]);
        guard.face(90.);
        guard.enemy.as_mut().unwrap().alert_response = 2;
        update(&mut guard, player);
        assert_eq!(guard.position[0], 6.);

        let mut fleeing = actor(5);
        fleeing.face(270.);
        update(&mut fleeing, player);
        assert_eq!(fleeing.position[0], -6.);
        assert!(!fleeing.enemy.as_ref().unwrap().alerted);
    }

    #[test]
    fn home_return_idle_and_stun_use_enemy_timing() {
        let mut enemy = actor(0);
        enemy.position = [301., 0., 0.];
        enemy.face(90.);
        let ai = enemy.autonomy.as_mut().unwrap();
        ai.radius = 300.;
        ai.activity = Activity::Walk;
        ai.initialized = true;
        ai.remaining = 500;
        for _ in 0..120 {
            update(&mut enemy, [1000.; 3]);
        }
        assert!(distance(enemy.position, [0.; 3]) < 300.);
        enemy.autonomy.as_mut().unwrap().select(Activity::Idle);
        let before = enemy.position;
        for _ in 0..130 {
            update(&mut enemy, [1000.; 3]);
        }
        assert_eq!(enemy.position, before);
        assert_eq!(enemy.autonomy.unwrap().activity, Activity::Idle);
        enemy.enemy.as_mut().unwrap().alerted = true;
        update(&mut enemy, [1000.; 3]);
        assert_eq!(enemy.autonomy.unwrap().activity, Activity::Walk);
        enemy.enemy.as_mut().unwrap().pause_ticks = 60;
        for _ in 0..60 {
            let result = enemy.step_autonomy(false, false, None, &mut || 0, &mut |_| true, 1.);
            assert!(result.paused);
        }
        assert_eq!(enemy.position, before);
        assert_eq!(enemy.enemy.unwrap().pause_ticks, 0);
    }

    #[test]
    fn edge_avoidance_checks_ahead_and_both_turn_directions() {
        let mut enemy = actor(2);
        let mut probes = Vec::new();
        enemy.step_autonomy(
            true,
            false,
            None,
            &mut || 1,
            &mut |point| {
                probes.push(point);
                false
            },
            1.,
        );
        assert_eq!(probes.len(), 2);
        assert_eq!(probes[0], [0., -FLOOR_LOOKAHEAD, 0.]);
        assert!(probes[1][0] > 100.);
        assert_eq!(enemy.target_heading, -AVOIDANCE_ANGLE);
        assert!(enemy.autonomy.unwrap().remaining >= 60);
    }

    #[test]
    fn patrol_uses_normal_speed_and_stops_to_watch_when_configured() {
        let mut enemy = actor(3);
        enemy.path.count = 2;
        enemy.path.points[0] = [100., 0., 0.];
        enemy.path.points[1] = [200., 0., 0.];
        enemy.enemy.as_mut().unwrap().alerted = true;
        update(&mut enemy, [0., -100., 0.]);
        assert_eq!(enemy.position[0], 1.5);
        enemy.enemy.as_mut().unwrap().alert_response = 1;
        let before = enemy.position;
        let intent = update(&mut enemy, [before[0], -100., 0.]);
        assert!(intent.walking && intent.paused);
        assert_eq!(enemy.position, before);
        assert_eq!(enemy.target_heading, 0.);
    }
}
