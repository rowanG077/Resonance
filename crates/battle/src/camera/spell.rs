//! A stored spell temporarily frames its caster, then rejoins ordinary tracking.
use super::{BASE_PITCH, CameraPose, approach_position, sub};
use crate::distance;

const CAST_RADIUS: f32 = 1900.;
const FOCUS_HEIGHT_OFFSET: f32 = 4800.;
const FOCUS_HEIGHT_SCALE: f32 = 72.;
const FOCUS_SPEED: f32 = 6.;
const FOCUS_RESPONSE: f32 = 0.1;
const MAXIMUM_FOCUS_SPEED: f32 = 180.;
const EYE_HEIGHT: f32 = 120.;
const EYE_SPEED: f32 = 12.;
const EYE_RESPONSE: f32 = 0.2;
const ZOOM_SPEED: f32 = 15.;
const ZOOM_RESPONSE: f32 = 0.15;

#[derive(Debug, Clone)]
pub(super) struct Shot {
    eye: [f32; 3],
    focus: [f32; 3],
    zoom: f32,
    bearing: f32,
}

impl Shot {
    pub(super) fn new(view: CameraPose) -> Self {
        let direction = sub(view.eye, view.focus);
        Self {
            eye: view.eye,
            focus: view.focus,
            zoom: direction[0].hypot(direction[2]),
            bearing: direction[2].atan2(direction[0]),
        }
    }

    pub(super) fn frame(&self) -> CameraPose {
        let direction = sub(self.eye, self.focus);
        let radius = direction[0].hypot(direction[2]);
        CameraPose {
            eye: self.eye,
            focus: self.focus,
            pitch: direction[1].atan2(radius).to_degrees(),
            yaw: direction[2].atan2(direction[0]).to_degrees(),
            radius,
        }
    }

    pub(super) fn cast(&mut self, caster: [f32; 3], stage_pitch: f32) {
        let focus = [
            caster[0],
            (self.zoom + FOCUS_HEIGHT_OFFSET) / FOCUS_HEIGHT_SCALE,
            caster[2],
        ];
        self.step(
            focus,
            CAST_RADIUS,
            self.bearing,
            (BASE_PITCH + stage_pitch).to_radians().sin(),
        );
    }

    pub(super) fn return_to(&mut self, ordinary: CameraPose) -> bool {
        let direction = sub(ordinary.eye, ordinary.focus);
        let radius = direction[0].hypot(direction[2]);
        let slope = (direction[1] - EYE_HEIGHT) / radius;
        self.step(
            ordinary.focus,
            radius,
            direction[2].atan2(direction[0]),
            slope,
        )
    }

    fn step(&mut self, focus: [f32; 3], radius: f32, bearing: f32, slope: f32) -> bool {
        let delta = sub(focus, self.focus);
        let speed =
            (FOCUS_SPEED + FOCUS_RESPONSE * delta[0].hypot(delta[2])).min(MAXIMUM_FOCUS_SPEED);
        let focused = approach_position(&mut self.focus, focus, speed);
        let (sin, cos) = bearing.sin_cos();
        let eye = [
            self.focus[0] + self.zoom * cos,
            self.focus[1] + EYE_HEIGHT + self.zoom * slope,
            self.focus[2] + self.zoom * sin,
        ];
        let speed = EYE_SPEED + EYE_RESPONSE * distance::length(sub(eye, self.eye));
        let placed = approach_position(&mut self.eye, eye, speed);
        // Eye and focus use this update's zoom; the next update sees its new value.
        let delta = radius - self.zoom;
        let speed = ZOOM_SPEED + ZOOM_RESPONSE * delta.abs();
        self.zoom += delta.clamp(-speed, speed);
        focused && placed && delta == 0.
    }
}
