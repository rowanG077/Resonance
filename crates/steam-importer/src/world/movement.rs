//! Normalize native travel values into the shared controller's units.
use super::*;
use world::{
    CameraDistances, CollisionSettings, FlightZone, FlightZones, MountDistances,
    MovementParameters, Position,
};

pub(super) fn read(image: &pe::Image<'_>) -> Result<MovementParameters> {
    let f = |at| float(image, at);
    let d = |at| double(image, at);
    let circle = |x, z, radius, height| -> Result<FlightZone> {
        let [x, y, _] = Position::from_plane([f(x)?, f(z)?, 0.])?.map();
        Ok(FlightZone {
            position: [x, y],
            radius,
            height,
        })
    };
    let parameters = MovementParameters {
        // Travel integration at 0x4bb2b0; direct vehicle input at 0x4b9e00.
        stick_maximum: d(0x8833d8)?,
        foot_stick_divisor: f(0x882a44)?,
        mounted_stick_divisor: f(0x883a9c)?,
        flight_speed_multiplier: d(0x882e18)?,
        ship_speed_multiplier: d(0x882d20)?,
        direct_vehicle_stick_divisor: d(0x882a98)?,
        flight_bank_divisor: d(0x882940)?,
        flight_acceleration_ticks: d(0x882b30)?,
        ship_acceleration_ticks: d(0x8829d0)?,
        pitch_limit: f(0x883174)?,
        pitch_step: d(0x884048)?,
        altitude_per_pitch: d(0x882aa8)?,
        maximum_altitude: f(0x882f5c)?,
        flight_clearance: d(0x882a78)?,
        // Fixed-circle flight floors. Landmark-relative floors are written
        // separately to the shared landmark owner below.
        flight_zones: FlightZones {
            sylvarant: Vec::new(),
            tethe_alla: vec![
                circle(0x883334, 0x883330, d(0x882e08)?, f(0x882f74)?)?,
                circle(0x883328, 0x883324, f(0x7a36d4)?, f(0x882efc)?)?,
            ],
        },
        turn_limit: f(0x883714)?,
        flight_turn_step: d(0x8839e0)?,
        ship_turn_step: d(0x882b28)?,
        // Mount/perspective transitions at 0x4b50ee..0x4b54bb.
        camera_distances: CameraDistances {
            close: MountDistances {
                foot: f(0x882f74)?,
                mounted: f(0x882f78)?,
            },
            distant: MountDistances {
                foot: f(0x882a40)?,
                mounted: f(0x882f7c)?,
            },
        },
        zoom_transition_ticks: d(0x882b30)?,
        lift_step: d(0x8829a8)?,
        minimum_lift_step: f(0x883a98)?,
        descent_easing_height: f(0x72f400)?,
        // Resonance's GameCube camera input convention, independent of the
        // source edition's camera/actor banking presentation.
        camera_turn_degrees: 0.015,
        collision: CollisionSettings {
            radius: f(0x882f74)?,
            ship_probe_half_extent: f(0x7d9220)?,
            ground_probe_half_extent: f(0x882e10)?,
        },
        heading_quantization: d(0x882cf8)?,
        camera_yaw_quantization: d(0x882cf0)?,
    };
    parameters.validate()?;
    Ok(parameters)
}

pub(super) fn bind_landmarks(library: &Path, image: &pe::Image<'_>) -> Result<()> {
    use world::{FlightFloor, Landmarks};
    let path = library.join("world/landmarks.json");
    let mut landmarks: Landmarks = serde_json::from_slice(&fs::read(&path)?)?;
    for (id, margin, height, active_only) in [
        (23, double(image, 0x882b60)?, float(image, 0x882e64)?, true),
        (25, double(image, 0x882b60)?, float(image, 0x882e64)?, true),
        (266, double(image, 0x882a78)?, float(image, 0x88332c)?, true),
        (
            274,
            double(image, 0x882b60)?,
            float(image, 0x88332c)?,
            false,
        ),
    ] {
        let landmark = landmarks
            .worlds
            .iter_mut()
            .flatten()
            .find(|landmark| landmark.id == id)
            .with_context(|| format!("world flight floor has no landmark {id}"))?;
        landmark.flight_floor = Some(FlightFloor {
            margin,
            height,
            active_only,
        });
    }
    landmarks.validate()?;
    output::json(&path, &landmarks)
}
