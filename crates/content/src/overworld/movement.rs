use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Native world movement/camera constants, extracted once with the terrain tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovementParameters {
    pub stick_maximum: f32,
    pub foot_stick_divisor: f32,
    pub mounted_stick_divisor: f32,
    pub flight_speed_multiplier: f32,
    pub ship_speed_multiplier: f32,
    pub direct_vehicle_stick_divisor: f32,
    pub flight_bank_divisor: f32,
    pub flight_acceleration_ticks: f32,
    pub ship_acceleration_ticks: f32,
    pub pitch_limit: f32,
    pub pitch_step: f32,
    pub altitude_per_pitch: f32,
    pub maximum_altitude: f32,
    pub flight_clearance: f32,
    pub turn_limit: f32,
    pub flight_turn_step: f32,
    pub ship_turn_step: f32,
    /// Zoom distances in the two perspective camera modes: [foot, mounted].
    pub camera_distances: [[f32; 2]; 2],
    pub zoom_transition_ticks: f32,
    pub lift_step: f32,
    pub minimum_lift_step: f32,
    pub descent_easing_height: f32,
    /// Degrees per unit of camera rotation input.
    pub camera_turn_degrees: f32,
    pub collision_radius: f32,
    /// Native scene-transfer precision, in steps per radian.
    pub heading_quantization: f32,
    pub camera_yaw_quantization: f32,
}
impl MovementParameters {
    pub fn validate(&self) -> Result<()> {
        let values = [
            self.stick_maximum,
            self.foot_stick_divisor,
            self.mounted_stick_divisor,
            self.flight_speed_multiplier,
            self.ship_speed_multiplier,
            self.direct_vehicle_stick_divisor,
            self.flight_bank_divisor,
            self.flight_acceleration_ticks,
            self.ship_acceleration_ticks,
            self.pitch_limit,
            self.pitch_step,
            self.altitude_per_pitch,
            self.maximum_altitude,
            self.flight_clearance,
            self.turn_limit,
            self.flight_turn_step,
            self.ship_turn_step,
            self.zoom_transition_ticks,
            self.lift_step,
            self.minimum_lift_step,
            self.descent_easing_height,
            self.camera_turn_degrees,
            self.collision_radius,
            self.heading_quantization,
            self.camera_yaw_quantization,
        ];
        ensure!(
            values
                .into_iter()
                .chain(self.camera_distances.into_iter().flatten())
                .all(|v| v.is_finite() && v > 0. && v <= 10000.),
            "invalid world movement parameter"
        );
        ensure!(
            self.mounted_stick_divisor <= self.foot_stick_divisor
                && self.flight_acceleration_ticks >= 1.
                && self.ship_acceleration_ticks >= 1.
                && self.zoom_transition_ticks >= 1.
                && self.pitch_step <= self.pitch_limit
                && self.flight_turn_step <= self.turn_limit
                && self.ship_turn_step <= self.turn_limit
                && self.flight_clearance < self.maximum_altitude
                && self.minimum_lift_step <= self.lift_step
                && self.descent_easing_height < self.flight_clearance
                && self.collision_radius < super::TILE_SIZE / 2.
                && self.camera_distances.iter().all(|pair| pair[0] < pair[1]),
            "inconsistent world movement limits"
        );
        // Turning subtracts from speed. All supported turn values must leave a
        // nonnegative maximum, including the slower foot divisor during takeoff.
        let speed = self.stick_maximum / self.foot_stick_divisor;
        ensure!(
            self.flight_speed_multiplier * speed >= self.turn_limit / self.flight_bank_divisor
                && self.ship_speed_multiplier * speed >= self.turn_limit * self.altitude_per_pitch,
            "world turning can produce a negative speed"
        );
        Ok(())
    }
}
