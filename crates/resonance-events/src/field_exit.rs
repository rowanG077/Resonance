//! A field handoff can open a nearby scenery door before activating its destination.
use crate::{
    ActorMotion, Animation, AudioCommand, BoneAdjustment, Fade, FieldTransition, GameWorld,
    ResourceLibrary, animation::slot,
};
use resonance_content::field::{DOOR_MOTION_RESOURCE_BASE, Door};

const APPROACH_SPEED: f32 = 6.;
const APPROACH_LIMIT: u32 = 90;
const OPENING_CONTACT_TICK: f32 = 24.;
const FADE_TICKS: u32 = 33;
const SCENERY_ACTOR: i32 = 999_996;
const HINGE_ADJUSTMENT: u8 = 255;

#[derive(Debug, Clone)]
pub(crate) struct DoorExit {
    request: FieldTransition,
    door: Door,
    phase: Phase,
}
#[derive(Debug, Clone)]
enum Phase {
    Prepare,
    Approach,
    Walking(u32),
    Facing,
    StartAnimation,
    Animation,
    Opening(f32),
}

impl GameWorld {
    pub(crate) fn begin_field_exit(
        &mut self,
        request: FieldTransition,
        resources: &ResourceLibrary,
    ) -> Result<(), String> {
        let nearest = self.actors.get(&self.controlled_actor).and_then(|actor| {
            resources
                .doors
                .iter()
                .filter_map(|door| {
                    let distance = (0..3)
                        .map(|i| (door.position[i] - actor.position[i]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    (distance <= self.door_interaction_radius.unwrap_or(250.))
                        .then_some((door, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(door, _)| door.clone())
        });
        if let Some(door) = nearest {
            let actor = &self.actors[&self.controlled_actor];
            let resource = DOOR_MOTION_RESOURCE_BASE + actor.resource;
            let slot = if door.pull { 24 } else { 20 };
            if resources
                .animations
                .get(&resource)
                .and_then(|clips| clips.get(&slot))
                .is_none()
            {
                return Err(format!(
                    "door-opening animation {resource:#x}/{slot} is not cooked"
                ));
            }
            if !self.actors.contains_key(&SCENERY_ACTOR) {
                return Err("door scenery actor is missing".into());
            }
            let actor = self.actors.get_mut(&self.controlled_actor).unwrap();
            actor.scripted_animation = true;
            self.preload_field = Some(request.map);
            self.field_exit = Some(DoorExit {
                request,
                door,
                phase: Phase::Prepare,
            });
        } else {
            self.field_transition = Some(request);
        }
        Ok(())
    }

    pub(crate) fn step_field_exit(&mut self, resources: &ResourceLibrary) -> Result<(), String> {
        let Some(mut exit) = self.field_exit.take() else {
            return Ok(());
        };
        self.input_enabled = false;
        let actor = self
            .actors
            .get_mut(&self.controlled_actor)
            .ok_or("door approach actor is missing")?;
        match exit.phase {
            Phase::Prepare => {
                // A request preserves the current pose until the door service
                // runs. Preparation does not start the approach in the same poll.
                if let Some(clip) = resources
                    .model(actor.resource)
                    .and_then(|m| m.clips.get(&slot::EVENT_IDLE))
                {
                    let mut animation = Animation::new(
                        actor.resource,
                        slot::EVENT_IDLE,
                        clip.duration_ticks,
                        self.tick,
                    );
                    animation.blend_ticks = 8;
                    actor.animation = Some(animation);
                }
                exit.phase = Phase::Approach;
            }
            Phase::Approach => {
                actor.scripted_animation = false;
                actor.motion = Some(ActorMotion {
                    target: exit.door.approach,
                    speed: APPROACH_SPEED,
                });
                exit.phase = Phase::Walking(self.tick);
            }
            Phase::Walking(start) => {
                if actor.motion.is_none() || self.tick - start > APPROACH_LIMIT {
                    actor.motion = None;
                    actor.target_heading = exit.door.heading;
                    actor.scripted_animation = true;
                    exit.phase = Phase::Facing;
                }
            }
            Phase::Facing => {
                if actor.heading == actor.target_heading {
                    exit.phase = Phase::StartAnimation;
                }
            }
            Phase::StartAnimation => {
                let resource = DOOR_MOTION_RESOURCE_BASE + actor.resource;
                let slot = if exit.door.pull { 24 } else { 20 };
                let clip = &resources.animations[&resource][&slot];
                let mut animation = Animation::new(resource, slot, clip.duration_ticks, self.tick);
                animation.source = crate::animation::AnimationSource::Resource;
                animation.blend_ticks = 1;
                animation.repeat = false;
                actor.animation = Some(animation);
                actor.scripted_animation = true;
                exit.phase = Phase::Animation;
            }
            Phase::Animation => {
                let animation = actor
                    .animation
                    .as_ref()
                    .ok_or("door-opening animation disappeared")?;
                if animation.elapsed(self.tick, 0) >= OPENING_CONTACT_TICK {
                    let alpha = self
                        .fade
                        .as_ref()
                        .map_or(0., |f| f.before_update(self.tick));
                    self.fade = Some(Fade::new(self.tick, FADE_TICKS, alpha, 256., false));
                    self.audio_commands.push(AudioCommand::Sound {
                        id: resonance_content::field_audio::ServiceCue::Door as i16,
                        volume: 127,
                        pan: 64,
                        slot: None,
                    });
                    exit.phase = Phase::Opening(0.);
                }
            }
            Phase::Opening(angle) => {
                let scenery = self
                    .actors
                    .get_mut(&SCENERY_ACTOR)
                    .ok_or("door scenery actor disappeared")?;
                scenery.appearance.bone_adjustments.insert(
                    HINGE_ADJUSTMENT,
                    BoneAdjustment {
                        bone: crate::BoneTarget::Index(exit.door.bone),
                        angles: [0., 0., angle],
                        from: [0., 0., angle],
                        duration_ticks: 1,
                        start_tick: self.tick,
                    },
                );
                // Present the current hinge pose before advancing the next one.
                let angle =
                    angle + (if exit.door.pull { 0.5 } else { 0.9375 }) * exit.door.angle.signum();
                if angle.abs() > exit.door.angle.abs() {
                    self.field_transition = Some(exit.request);
                    return Ok(());
                }
                exit.phase = Phase::Opening(angle);
            }
        }
        self.field_exit = Some(exit);
        Ok(())
    }
}
