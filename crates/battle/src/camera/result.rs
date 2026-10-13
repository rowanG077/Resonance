//! The result scene's held opening, zoom and slow orbit.
use super::Camera;
use crate::{ActorId, Battle, BattlePhase, Side};
use anyhow::{Result, ensure};

const RESULT_HEADING: f32 = 120.;
const CAMERA_HOLD_TICKS: u32 = 20;
const CAMERA_START_RADIUS: f32 = 2500.;
const CAMERA_END_RADIUS: f32 = 1400.;
const CAMERA_ZOOM_FACTOR: f32 = 0.98;
const CAMERA_TURN_PER_TICK: f32 = 0.025;
const CAMERA_FOCUS_HEIGHT: f32 = 112.5;
const CAMERA_PITCH: f32 = 4.5;
const PARTY_ARC_RADIUS: f32 = 200.;
const PARTY_ARC_EXTRA_DEGREES: f32 = 10.;

impl Battle {
    /// Center the celebrant and arrange the remaining roster around them.
    pub fn arrange_result_actors(&mut self, celebrant: ActorId) -> Result<()> {
        let count = self
            .actors
            .iter()
            .filter(|actor| actor.side == Side::Party)
            .count();
        ensure!(
            self.phase() == BattlePhase::Results
                && self
                    .actors
                    .get(celebrant.index())
                    .is_some_and(|actor| actor.side == Side::Party),
            "invalid result formation"
        );
        let angle_step = 360. / (count - 1).max(1) as f32 + PARTY_ARC_EXTRA_DEGREES;
        let mut outer_slot = 0;
        for (index, actor) in self
            .actors
            .iter_mut()
            .enumerate()
            .filter(|(_, actor)| actor.side == Side::Party)
        {
            actor.position = if index == celebrant.index() {
                [0.; 3]
            } else {
                let (sin, cos) = (angle_step * outer_slot as f32).to_radians().sin_cos();
                outer_slot += 1;
                [PARTY_ARC_RADIUS * cos, 0., PARTY_ARC_RADIUS * sin]
            };
            actor.heading = RESULT_HEADING;
        }
        if let Some(camera) = &mut self.camera {
            camera.result_tick = 0;
            camera.frame_results();
        }
        Ok(())
    }
}

impl Camera {
    pub(crate) fn frame_results(&mut self) {
        self.spell = None;
        self.scene = None;
        let moving_ticks = self.result_tick.saturating_sub(CAMERA_HOLD_TICKS) as f32;
        self.pose.focus = [0., CAMERA_FOCUS_HEIGHT, 0.];
        self.pose.pitch = CAMERA_PITCH;
        self.pose.yaw = 90. - RESULT_HEADING - CAMERA_TURN_PER_TICK * moving_ticks;
        self.pose.radius =
            (CAMERA_START_RADIUS * CAMERA_ZOOM_FACTOR.powf(moving_ticks)).max(CAMERA_END_RADIUS);
        let (sin, cos) = self.pose.yaw.to_radians().sin_cos();
        self.pose.eye = [
            self.pose.radius * cos,
            CAMERA_FOCUS_HEIGHT + self.pose.radius * CAMERA_PITCH.to_radians().sin(),
            self.pose.radius * sin,
        ];
        self.result_tick = self.result_tick.saturating_add(1);
    }
}
