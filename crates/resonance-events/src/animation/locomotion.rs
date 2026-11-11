//! Shared automatic poses for ordinary updates and immediate script releases.
use super::{Animation, slot};
use crate::stride::Gait;
use crate::{Actor, ModelResource};

pub(crate) struct Locomotion {
    pub movement_speed: Option<f32>,
    pub walking: bool,
    pub turn: f32,
    pub dialogue: bool,
    pub event_controlled: bool,
    pub player_controlled: bool,
    pub stride_scale: f32,
    /// Releasing a scripted animation forces a rebind; positive values override blending.
    pub release: Option<i32>,
}

impl Actor {
    pub(crate) fn select_automatic_animation(
        &mut self,
        model: &ModelResource,
        tick: u32,
        state: Locomotion,
    ) {
        let Locomotion {
            movement_speed,
            walking,
            turn,
            dialogue,
            event_controlled,
            player_controlled,
            stride_scale,
            release,
        } = state;
        let requested = if let Some(speed) = movement_speed {
            let running = if player_controlled {
                Gait::at(speed, stride_scale) == Gait::Run
            } else {
                (!walking || self.enemy.is_some()) && speed > 7.
            };
            let event_gait = if running {
                slot::EVENT_RUN
            } else {
                slot::EVENT_WALK
            };
            if event_controlled && model.clips.contains_key(&event_gait) {
                event_gait
            } else if running && model.clips.contains_key(&slot::RUN) {
                slot::RUN
            } else {
                slot::WALK
            }
        } else if self.enemy.is_none() && turn != 0. && model.clips.contains_key(&slot::TURN_RIGHT)
        {
            if turn < 0. {
                slot::TURN_LEFT
            } else {
                slot::TURN_RIGHT
            }
        } else if dialogue {
            // Use the event conversation pose when the script owns the player.
            [
                if event_controlled {
                    slot::EVENT_TALK
                } else {
                    slot::TALK
                },
                slot::TALK_FALLBACK,
                slot::IDLE,
            ]
            .into_iter()
            .find(|slot| model.clips.contains_key(slot))
            .unwrap_or(self.idle_animation)
        } else if event_controlled && model.clips.contains_key(&slot::EVENT_IDLE) {
            slot::EVENT_IDLE
        } else {
            self.idle_animation
        };
        let slot = if model.clips.contains_key(&requested) {
            requested
        } else {
            slot::IDLE
        };
        if model.clips.contains_key(&slot)
            && (release.is_some()
                || self.animation.as_ref().is_none_or(|a| {
                    a.source != crate::animation::AnimationSource::Model
                        || a.resource != self.resource
                        || a.slot != slot
                }))
        {
            self.animation = Some(Animation {
                blend_ticks: if let Some(value) = release.filter(|v| *v > 0) {
                    value as u32
                } else if matches!(slot, slot::TURN_RIGHT | slot::TURN_LEFT)
                    || player_controlled && matches!(slot, slot::WALK | slot::RUN)
                {
                    2
                } else {
                    8
                },
                repeat: !matches!(slot, slot::TURN_RIGHT | slot::TURN_LEFT),
                ..Animation::new(self.resource, slot, model.clips[&slot].duration_ticks, tick)
            });
        }
        if player_controlled
            && let Some(speed) = movement_speed
            && let Some(animation) = &mut self.animation
            && matches!(slot, slot::WALK | slot::RUN)
        {
            // Scale player gait with movement speed. Script-directed movement
            // keeps its independent authored rate and blend duration.
            let gait = if slot == slot::WALK {
                Gait::Walk
            } else {
                Gait::Run
            };
            let rate = gait.rate(speed, stride_scale);
            if animation.rate != rate {
                animation.seek(
                    animation.sample(tick, 0, animation.duration_ticks as f32),
                    tick,
                );
                animation.rate = rate;
            }
        }
    }
}
