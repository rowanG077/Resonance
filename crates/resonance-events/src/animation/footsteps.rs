//! Native field foot contacts (fn_80019BDC): two windows per gait cycle.
use super::{AnimationSource, slot};
use crate::Actor;

impl Actor {
    /// Consume a native foot contact after the field owner resolves the floor.
    pub fn step_footstep(&mut self, tick: u32, on_floor: bool) -> Option<i16> {
        let animation = self.animation.as_ref()?;
        if animation.rate == 0. || animation.paused_at.is_some() {
            return None;
        }
        if !on_floor
            || !self.interaction_disabled
            || !self.visible
            || self.appearance.model_hidden
            || !self.grounded
            || self.attachment.is_some()
            || self.animation_culled
            || animation.source != AnimationSource::Model
            || !matches!(
                animation.slot,
                slot::WALK | slot::RUN | slot::EVENT_WALK | slot::EVENT_RUN
            )
        {
            self.footstep_contact = None;
            return None;
        }
        let native_frames =
            resonance_content::animation::FRAME_HZ / resonance_content::ANIMATION_HZ;
        let frame =
            (animation.sample(tick, 0, animation.duration_ticks as f32) * native_frames) as u32;
        let half = (animation.duration_ticks as f32 * native_frames) as u32 / 2;
        let contact = if (1..=3).contains(&frame) {
            1
        } else if half > 3 && (half..=half + 3).contains(&frame) {
            half
        } else {
            return None;
        };
        let contact = (
            animation.resource,
            animation.slot,
            animation.start_tick,
            contact,
        );
        if self.footstep_contact == Some(contact) {
            return None;
        }
        self.footstep_contact = Some(contact);
        let material = (self.ground_attributes >> 5) & 15;
        (material <= 12).then_some(11 + material as i16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Animation;

    fn walker() -> Actor {
        let mut actor = Actor::new(1, [0.; 3]);
        actor.interaction_disabled = true;
        actor.animation = Some(Animation::new(1, slot::WALK, 60, 0));
        actor
    }

    #[test]
    fn footsteps_follow_gait_contacts_and_material_changes() {
        let mut actor = walker();
        let mut heard = Vec::new();
        for tick in 0..=122 {
            if tick == 10 {
                actor.ground_attributes = 6 << 5;
            }
            if let Some(id) = actor.step_footstep(tick, true) {
                heard.push((tick, id));
            }
        }
        assert_eq!(heard, [(2, 11), (30, 17), (62, 17), (90, 17), (122, 17)]);
        for material in 0..16 {
            let mut actor = walker();
            actor.ground_attributes = material << 5 | 0x8000_001f;
            assert_eq!(
                actor.step_footstep(2, true),
                (material <= 12).then_some(11 + material as i16)
            );
        }
    }

    #[test]
    fn running_rate_and_blending_control_contact_times() {
        let mut actor = walker();
        let animation = actor.animation.as_mut().unwrap();
        animation.slot = slot::RUN;
        animation.rate = 2.;
        animation.blend_ticks = 2;
        let heard: Vec<_> = (0..=63)
            .filter(|&tick| actor.step_footstep(tick, true).is_some())
            .collect();
        assert_eq!(heard, [3, 17, 33, 47, 63]);

        let mut actor = walker();
        assert_eq!(actor.step_footstep(2, true), Some(11));
        for (slot, start) in [(slot::RUN, 4), (slot::RUN, 8), (slot::WALK, 12)] {
            actor.animation = Some(Animation::new(1, slot, 40, start));
            assert_eq!(actor.step_footstep(start + 2, true), Some(11));
            assert_eq!(actor.step_footstep(start + 3, true), None);
        }
    }

    #[test]
    fn idle_airborne_hidden_and_paused_actors_are_quiet() {
        for mode in 0..6 {
            let mut actor = walker();
            match mode {
                0 => actor.interaction_disabled = false,
                1 => actor.grounded = false,
                2 => actor.visible = false,
                3 => actor.animation_culled = true,
                4 => actor.animation.as_mut().unwrap().slot = slot::IDLE,
                _ => actor.appearance.model_hidden = true,
            }
            assert!((0..60).all(|tick| actor.step_footstep(tick, true).is_none()));
        }
        let mut actor = walker();
        assert_eq!(actor.step_footstep(2, false), None);
        actor.attachment = Some(crate::Attachment {
            actor: 2,
            bone: "root".into(),
        });
        assert_eq!(actor.step_footstep(2, true), None);
        actor.attachment = None;
        assert_eq!(actor.step_footstep(2, true), Some(11));
        actor.animation.as_mut().unwrap().script_pause(true, 2);
        assert!((3..60).all(|tick| actor.step_footstep(tick, true).is_none()));
        actor.animation.as_mut().unwrap().script_pause(false, 60);
        assert_eq!(actor.step_footstep(61, true), None);
        assert_eq!(actor.step_footstep(88, true), Some(11));
    }
}
