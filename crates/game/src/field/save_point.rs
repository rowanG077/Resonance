//! Memory-circle targeting and continuous visuals; dialogue is an authored task.
use anyhow::{Context, Result};
use resonance_events::{
    EventRuntime, GameWorld,
    effect::{BillboardEffect, NEUTRAL_PALETTE, RefractionPulse},
};

const TUTORIAL_SEEN: u16 = 0x208;
const ACTIVATION_RADIUS: f32 = 80.;
const EXAMINE_RADIUS: f32 = 30.;
const IDLE_RATE: f32 = 0.1;
const ACTIVE_RATE: f32 = 0.2;
const RATE_STEP: f32 = 0.04;
const IDLE_GLOW: f32 = 0.08;
const SPARK_PERIOD: u32 = 4;

#[derive(Default)]
pub(super) struct SavePoints {
    services: Option<std::sync::Arc<crate::authored::FieldServices>>,
    active: Option<i32>,
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
            || world.world_transition.is_some()
            || world.blocked_by_movie()
            || world
                .fade
                .as_ref()
                .is_some_and(|f| world.tick < f.start_tick.saturating_add(f.duration))
    }

    pub fn new(services: Option<std::sync::Arc<crate::authored::FieldServices>>) -> Self {
        Self {
            services,
            active: None,
        }
    }

    fn busy(&self, events: &EventRuntime) -> bool {
        self.active.is_some_and(|handle| events.is_active(handle))
    }

    pub fn sealed_target(world: &GameWorld) -> Option<usize> {
        let player = world.actors.get(&world.controlled_actor)?.position;
        // fn_8000E618 uses a strict three-dimensional distance of 30.
        world.save_points.iter().position(|p| {
            !p.is_open(&world.event_flags)
                && player
                    .iter()
                    .zip(p.position)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    < EXAMINE_RADIUS.powi(2)
        })
    }

    pub fn interact(&mut self, events: &mut EventRuntime, accept: bool) -> Result<()> {
        if accept
            && events.player_has_control()
            && !Self::suspended(&events.world)
            && let Some(point) = Self::sealed_target(&events.world)
        {
            let service = &self
                .services
                .as_ref()
                .context("memory-circle scripts were not prepared")?
                .memory_unlock;
            self.active = Some(service.start_with_arguments(events, &[i32::try_from(point)?])?);
        }
        Ok(())
    }

    pub fn step(&mut self, events: &mut EventRuntime, control: bool) -> Result<()> {
        let world = &events.world;
        if !control
            || self.busy(events)
            || Self::suspended(world)
            || world.event_flags.contains(&TUTORIAL_SEEN)
        {
            return Ok(());
        }
        let Some(player) = world.actors.get(&world.controlled_actor) else {
            return Ok(());
        };
        if world
            .save_points
            .iter()
            .any(|p| p.is_open(&world.event_flags) && within_reach(player.position, p.position))
        {
            let service = &self
                .services
                .as_ref()
                .context("memory-circle scripts were not prepared")?
                .memory_tutorial;
            self.active = Some(service.start(events)?);
        }
        Ok(())
    }

    /// Emit scene effects after actor updates, before particles and scripts.
    pub fn step_effects(&self, events: &mut EventRuntime, effect_tick: u32) -> Result<()> {
        if self.busy(events) || Self::suspended(&events.world) {
            return Ok(());
        }
        let world = &events.world;
        let Some(player) = world
            .actors
            .get(&world.controlled_actor)
            .map(|a| a.position)
        else {
            return Ok(());
        };
        for index in 0..events.world.save_points.len() {
            let point = &events.world.save_points[index];
            if !point.is_open(&events.world.event_flags) {
                continue;
            }
            let hidden_nodes = point.unlock_flag.and_then(|_| {
                events.resources().model(point.resource).map(|model| {
                    model
                        .names
                        .iter()
                        .enumerate()
                        .filter(|(_, name)| name.starts_with("HID_"))
                        .map(|(i, _)| i as u16)
                        .collect()
                })
            });
            let world = &mut events.world;
            let point = &mut world.save_points[index];
            if point.unlock_flag.take().is_some()
                && let Some(actor) = world.actors.get_mut(&point.actor)
            {
                // fn_8000E39C replaces the sealed model before its live glow starts.
                if let Some(hidden_nodes) = hidden_nodes {
                    actor.appearance.hidden_nodes = hidden_nodes;
                }
                if let Some(animation) = actor.animation.as_mut()
                    && animation.rate == 0.
                {
                    animation.phase_tick = world.tick;
                    animation.rate = IDLE_RATE;
                }
            }
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
                        operation: None,
                        image: resonance_events::effect::RefractionImage::Ripple,
                        palette: NEUTRAL_PALETTE,
                        orientation: resonance_events::effect::SpriteOrientation::World,
                        rotation: [0.; 3],
                        position: [position[0], position[1], position[2] + 10.],
                        born: world.tick,
                        lifetime: 30,
                        size: 20.,
                        growth: 40.,
                        alpha: 224.,
                        fade: resonance_events::effect::Fade::Tail { after: 0 },
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

    use resonance_events::{MemoryCircleText, ResourceLibrary};
    use std::sync::Arc;

    fn fixture(world: GameWorld) -> (SavePoints, EventRuntime) {
        struct Resources;
        impl crate::authored::Resources for Resources {
            fn asset(&mut self, _: &crate::authored::AssetReference) -> Result<()> {
                anyhow::bail!("unexpected asset")
            }
            fn message(&mut self, _: &str) -> Result<()> {
                Ok(())
            }
            fn substitution(&mut self, _: symphonia_script::authored::Type) -> Result<()> {
                Ok(())
            }
        }
        let sources = symphonia_script_tools::SourceTree::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts"
        ))
        .unwrap();
        let services = crate::authored::FieldServices::prepare(
            &mut Default::default(),
            &sources,
            &mut Resources,
        )
        .unwrap();
        let program = Arc::new(
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap(),
        );
        let resources = ResourceLibrary {
            memory_circle_text: MemoryCircleText {
                tutorial: vec![resonance_content::font::TextSpan {
                    text: "A notice".into(),
                    color: 4,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let events =
            EventRuntime::with_state(program, Arc::new(resources), world, Default::default())
                .unwrap();
        (SavePoints::new(Some(Arc::new(services))), events)
    }

    fn step(points: &mut SavePoints, events: &mut EventRuntime, control: bool, tick: u32) {
        points.step(events, control).unwrap();
        // Run a queued dialogue task; visual-only checks retain their explicit simulation clock.
        if points.busy(events) {
            events.step().unwrap();
        }
        points.step_effects(events, tick).unwrap();
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
            unlock_flag: None,
            glow_scale: 1.,
        });
        let mut points = SavePoints::default();
        let program = Arc::new(
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap(),
        );
        let mut events =
            EventRuntime::with_state(program, Default::default(), world, Default::default())
                .unwrap();
        for expected in [0.08, 0.22, 0.32, 0.38] {
            events.world.tick += 1;
            let tick = events.world.tick;
            step(&mut points, &mut events, true, tick);
            let animation = events.world.actors[&0].animation.as_ref().unwrap();
            assert!((animation.sample(events.world.tick, 0, 120.) - expected).abs() < 0.00001);
        }
        assert!(!events.world.save_points[0].active);
    }

    #[test]
    fn sealed_circle_needs_its_unlock_flag_before_tutorial_and_recovery() {
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
            unlock_flag: Some(851),
            glow_scale: 0.08,
        });
        let (mut points, mut events) = fixture(world);
        step(&mut points, &mut events, true, 0);
        assert!(events.world.dialogue.is_empty());
        assert!(!events.world.save_points[0].active);
        assert!(events.world.refractions.is_empty());
        events.world.event_flags.insert(851);
        step(&mut points, &mut events, false, 0);
        assert!(events.world.dialogue.is_empty());
        step(&mut points, &mut events, true, 0);
        assert!(!events.world.input_enabled);
        assert!(!events.world.event_flags.contains(&TUTORIAL_SEEN));
        let operation = events.world.dialogue[&0].operation.clone();
        step(&mut points, &mut events, true, 0);
        assert_eq!(events.world.dialogue[&0].operation.id(), operation.id());
        operation.complete(None).unwrap();
        events.step().unwrap();
        assert!(events.world.input_enabled);
        assert!(events.world.event_flags.contains(&TUTORIAL_SEEN));
        events.world.tick = 1;
        step(&mut points, &mut events, true, 132);
        assert!(!points.busy(&events));
        assert_eq!(events.world.dialogue[&0].operation.id(), operation.id());
        assert!(events.world.save_points[0].active);
        assert_eq!(events.world.refractions.len(), 1);
        assert_eq!(events.world.billboards.len(), 1);
        assert_eq!(events.world.audio_commands.len(), 1);
        let spark = events.world.billboards.values().next().unwrap();
        assert_eq!(spark.position[2], 0.);
        assert_eq!(spark.rotation[2], 4.);
        assert_eq!(spark.born, 1);
        assert!(spark.alive(spark.born + 60));
        assert!(!spark.alive(spark.born + 61));
        events.world.tick = 4;
        step(&mut points, &mut events, true, 135);
        assert_eq!(events.world.billboards.len(), 1);
        events.world.tick = 5;
        step(&mut points, &mut events, true, 136);
        assert_eq!(events.world.billboards.len(), 2);
        assert_eq!(events.world.audio_commands.len(), 1);
        events.world.actors.get_mut(&1).unwrap().position = [81., 0., 0.];
        events.world.tick += 1;
        step(&mut points, &mut events, true, 137);
        assert!(!events.world.save_points[0].active);
        events.world.actors.get_mut(&1).unwrap().position = [79., 0., 0.];
        events.world.tick += 1;
        step(&mut points, &mut events, true, 138);
        assert_eq!(events.world.refractions.len(), 2);
        assert_eq!(events.world.audio_commands.len(), 2);
    }
}
