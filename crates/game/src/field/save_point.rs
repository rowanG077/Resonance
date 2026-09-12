//! Field-owned memory-circle interaction, independent of the scenario VM.
use anyhow::{Result, ensure};
use resonance_content::font::TextSpan;
use resonance_events::{
    GameWorld, Operation, Outcome,
    dialogue::{ResolvedMessage, flags},
    effect::{BillboardEffect, RefractionPulse},
};

const TUTORIAL_SEEN: u16 = 0x208;
const ACTIVATION_RADIUS: f32 = 80.;
const IDLE_RATE: f32 = 0.1;
const ACTIVE_RATE: f32 = 0.2;
const RATE_STEP: f32 = 0.04;
const IDLE_GLOW: f32 = 0.08;
const SPARK_PERIOD: u32 = 4;

#[derive(Default)]
pub(super) struct SavePoints {
    tutorial: Vec<TextSpan>,
    notice: Option<Operation>,
}
fn within_reach(player: [f32; 3], point: [f32; 3]) -> bool {
    player
        .iter()
        .zip(point)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        < ACTIVATION_RADIUS.powi(2)
}

impl SavePoints {
    fn suspended(world: &GameWorld) -> bool {
        world.field_transition.is_some()
            || world.blocked_by_movie()
            || world
                .fade
                .as_ref()
                .is_some_and(|f| world.tick < f.start_tick.saturating_add(f.duration))
    }

    pub fn new(tutorial: Vec<TextSpan>) -> Self {
        Self {
            tutorial,
            ..Default::default()
        }
    }

    pub fn finish_notice(&mut self, world: &mut GameWorld) -> Result<()> {
        let Some(outcome) = self.notice.as_ref().and_then(|op| op.progress().outcome) else {
            return Ok(());
        };
        ensure!(
            matches!(outcome, Outcome::Completed(_)),
            "memory-circle tutorial was cancelled"
        );
        world.event_flags.insert(TUTORIAL_SEEN);
        world.input_enabled = true;
        self.notice = None;
        Ok(())
    }

    pub fn step(&mut self, world: &mut GameWorld, player_has_control: bool) -> Result<()> {
        if self.notice.is_some() {
            world.input_enabled = false;
            return Ok(());
        }
        if Self::suspended(world) {
            return Ok(());
        }
        let Some(player) = world.actors.get(&world.controlled_actor) else {
            return Ok(());
        };
        let player = player.position;
        let seen = world.event_flags.contains(&TUTORIAL_SEEN);
        if !seen
            && player_has_control
            && world
                .save_points
                .iter()
                .any(|p| within_reach(player, p.position))
        {
            ensure!(
                !self.tutorial.is_empty(),
                "memory-circle tutorial is not cooked"
            );
            self.notice = Some(
                world
                    .show_notice(ResolvedMessage::from_spans(&self.tutorial), flags::GREEN)
                    .map_err(anyhow::Error::msg)?,
            );
            world.input_enabled = false;
            world
                .actors
                .get_mut(&world.controlled_actor)
                .unwrap()
                .motion = None;
            return Ok(());
        }
        Ok(())
    }

    /// Emit scene effects after actor updates, before particles and scripts.
    pub fn step_effects(&self, world: &mut GameWorld, effect_tick: u32) -> Result<()> {
        if self.notice.is_some() || Self::suspended(world) {
            return Ok(());
        }
        let Some(player) = world
            .actors
            .get(&world.controlled_actor)
            .map(|a| a.position)
        else {
            return Ok(());
        };
        for index in 0..world.save_points.len() {
            let point = &mut world.save_points[index];
            let active =
                world.event_flags.contains(&TUTORIAL_SEEN) && within_reach(player, point.position);
            let entered = active && !point.active;
            point.active = active;
            point.glow_scale = if active {
                if point.glow_scale < 1. {
                    point.glow_scale + 0.02
                } else {
                    1.
                }
            } else {
                (point.glow_scale - 0.01).max(IDLE_GLOW)
            };
            if let Some(animation) = world
                .actors
                .get_mut(&point.actor)
                .and_then(|a| a.animation.as_mut())
            {
                let rate = if active && animation.rate < ACTIVE_RATE {
                    animation.rate + RATE_STEP
                } else if !active && animation.rate > IDLE_RATE {
                    animation.rate - RATE_STEP
                } else {
                    animation.rate
                };
                if rate != animation.rate {
                    // The new speed applies to this update, including a loop wrap.
                    animation.start_frame = animation.sample(
                        world.tick.saturating_sub(1),
                        0,
                        animation.duration_ticks as f32,
                    ) + rate;
                    animation.phase_tick = world.tick;
                    animation.rate = rate;
                }
            }
            let position = point.position;
            if entered {
                world
                    .emit_refraction(RefractionPulse {
                        position: [position[0], position[1], position[2] + 10.],
                        born: world.tick,
                        lifetime: 30,
                        size: 20.,
                        growth: 40.,
                        alpha: 224.,
                        fade: 8.,
                    })
                    .map_err(anyhow::Error::msg)?;
                world
                    .audio_commands
                    .push(resonance_events::AudioCommand::Sound {
                        id: resonance_content::field_audio::ServiceCue::Recovery as i16,
                        volume: 100,
                        pan: 64,
                        slot: None,
                    });
            }
            if active && effect_tick.is_multiple_of(SPARK_PERIOD) {
                let x = (world.random() & 63) as f32 - 31.;
                let y = (world.random() & 63) as f32 - 31.;
                let size = (world.random() & 15) as f32 + 32.;
                let speed = (world.random() & 31) as f32 * 0.125 + 2.;
                let spark = BillboardEffect::rising_spark(
                    [position[0] + x, position[1] + y, position[2]],
                    size,
                    speed,
                    world.tick,
                    effect_tick,
                );
                // Existing billboards have already advanced, preserving this birth pose.
                world.emit_billboard(spark).map_err(anyhow::Error::msg)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_events::{Actor, Animation, SavePoint, animation::slot};

    fn step(points: &mut SavePoints, world: &mut GameWorld, control: bool, tick: u32) {
        points.step(world, control).unwrap();
        points.step_effects(world, tick).unwrap();
    }

    #[test]
    fn circle_decelerates_on_departure_even_at_a_loop_boundary() {
        let mut world = GameWorld::default();
        world.tick = 10;
        world.controlled_actor = 1;
        world.input_enabled = true;
        world.event_flags.insert(TUTORIAL_SEEN);
        world.actors.insert(1, Actor::new(0, [81., 0., 0.]));
        let mut circle = Actor::new(0, [0.; 3]);
        let mut animation = Animation::new(0, slot::IDLE, 120, world.tick);
        animation.start_frame = 119.9;
        animation.rate = 0.22;
        circle.animation = Some(animation);
        world.actors.insert(0, circle);
        world.save_points.push(SavePoint {
            actor: 0,
            position: [0.; 3],
            resource: 0,
            born: 0,
            active: true,
            glow_scale: 1.,
        });
        let mut points = SavePoints::default();
        for expected in [0.08, 0.22, 0.32, 0.38] {
            world.tick += 1;
            let tick = world.tick;
            step(&mut points, &mut world, true, tick);
            let animation = world.actors[&0].animation.as_ref().unwrap();
            assert!((animation.sample(world.tick, 0, 120.) - expected).abs() < 0.00001);
        }
        assert!(!world.save_points[0].active);
    }

    #[test]
    fn tutorial_waits_for_control_and_completes_once() {
        let mut world = GameWorld::default();
        world.controlled_actor = 1;
        world.input_enabled = true;
        world.actors.insert(1, Actor::new(0, [79., 0., 0.]));
        world.save_points.push(SavePoint {
            actor: i32::MIN,
            position: [0.; 3],
            resource: 0,
            born: 0,
            active: false,
            glow_scale: 0.08,
        });
        let mut points = SavePoints::new(vec![TextSpan {
            text: "A notice".into(),
            color: 4,
        }]);
        step(&mut points, &mut world, false, 0);
        assert!(world.dialogue.is_empty());
        step(&mut points, &mut world, true, 0);
        assert!(!world.input_enabled);
        assert!(!world.event_flags.contains(&TUTORIAL_SEEN));
        let operation = world.dialogue[&0].operation.clone();
        step(&mut points, &mut world, true, 0);
        assert_eq!(world.dialogue[&0].operation.id(), operation.id());
        operation.complete(None).unwrap();
        points.finish_notice(&mut world).unwrap();
        assert!(world.input_enabled);
        assert!(world.event_flags.contains(&TUTORIAL_SEEN));
        world.tick = 1;
        step(&mut points, &mut world, true, 132);
        assert!(points.notice.is_none());
        assert_eq!(world.dialogue[&0].operation.id(), operation.id());
        assert!(world.save_points[0].active);
        assert_eq!(world.refractions.len(), 1);
        assert_eq!(world.billboards.len(), 1);
        assert_eq!(world.audio_commands.len(), 1);
        let spark = world.billboards.values().next().unwrap();
        assert_eq!(spark.position[2], 0.);
        assert_eq!(spark.rotation[2], 4.);
        assert_eq!(spark.born, 1);
        assert!(spark.alive(spark.born + 60));
        assert!(!spark.alive(spark.born + 61));
        world.tick = 4;
        step(&mut points, &mut world, true, 135);
        assert_eq!(world.billboards.len(), 1);
        world.tick = 5;
        step(&mut points, &mut world, true, 136);
        assert_eq!(world.billboards.len(), 2);
        assert_eq!(world.audio_commands.len(), 1);
        world.actors.get_mut(&1).unwrap().position = [81., 0., 0.];
        world.tick += 1;
        step(&mut points, &mut world, true, 137);
        assert!(!world.save_points[0].active);
        world.actors.get_mut(&1).unwrap().position = [79., 0., 0.];
        world.tick += 1;
        step(&mut points, &mut world, true, 138);
        assert_eq!(world.refractions.len(), 2);
        assert_eq!(world.audio_commands.len(), 2);
    }
}
