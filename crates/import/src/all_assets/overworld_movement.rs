//! World controller constants from the supported English field modules.
use crate::{embedded, read::f32 as float, rel::Rel};
use anyhow::{Result, ensure};
use resonance_content::overworld::{FlightZone, MovementParameters, Position};
use std::path::Path;

const FAMILY: &str = "overworld-movement";

pub(super) fn read(rel: &Rel) -> Result<MovementParameters> {
    let value = |offset| float(rel.at((5, offset))?, 0);
    let map_position = |x, z| -> Result<[f32; 2]> {
        let [x, y, _] = Position::from_plane([value(x)?, value(z)?, 0.])?.map();
        Ok([x, y])
    };
    ensure!(
        value(0x9f0)? == -value(0x9e8)? && value(0xa00)? == -value(0x9f8)?,
        "asymmetric world controller limits"
    );
    let parameters = MovementParameters {
        stick_maximum: value(0x900)?,
        foot_stick_divisor: value(0x924)?,
        mounted_stick_divisor: value(0x920)?,
        flight_speed_multiplier: value(0x8fc)?,
        ship_speed_multiplier: value(0x904)?,
        direct_vehicle_stick_divisor: value(0xaa8)?,
        flight_bank_divisor: value(0x9e0)?,
        flight_acceleration_ticks: value(0x9e4)?,
        ship_acceleration_ticks: value(0xa04)?,
        pitch_limit: value(0x9e8)?,
        pitch_step: value(0x9ec)?,
        altitude_per_pitch: value(0x95c)?,
        maximum_altitude: value(0x9f4)?,
        flight_clearance: value(0x954)?,
        flight_zones: [
            // Tower of Mana and the Mana Cannon.
            [23, 25]
                .map(|location| {
                    Ok(FlightZone::Landmark {
                        location,
                        margin: value(0x960)?,
                        height: value(0xa94)?,
                        active_only: true,
                    })
                })
                .into_iter()
                .collect::<Result<_>>()?,
            vec![
                FlightZone::Circle {
                    position: map_position(0xc2c, 0xc30)?,
                    radius: value(0xb50)?,
                    height: value(0x8e8)?,
                },
                // Exire and the Temple of Lightning.
                FlightZone::Landmark {
                    location: 266,
                    margin: value(0x954)?,
                    height: value(0xa8c)?,
                    active_only: true,
                },
                FlightZone::Landmark {
                    location: 274,
                    margin: value(0x960)?,
                    height: value(0xa8c)?,
                    active_only: false,
                },
                FlightZone::Circle {
                    position: map_position(0xc34, 0xc38)?,
                    radius: value(0x954)?,
                    height: value(0xb50)?,
                },
            ],
        ],
        turn_limit: value(0x9f8)?,
        flight_turn_step: value(0x9fc)?,
        ship_turn_step: value(0xa08)?,
        camera_distances: [
            [value(0x8e8)?, value(0x944)?],
            [value(0x948)?, value(0x940)?],
        ],
        zoom_transition_ticks: value(0x9e4)?,
        lift_step: value(0xb70)?,
        minimum_lift_step: value(0x904)?,
        descent_easing_height: value(0xb74)?,
        camera_turn_degrees: value(0xb78)?,
        collision_radius: value(0x8e8)?,
        heading_quantization: value(0x958)?,
        camera_yaw_quantization: value(0xa0c)?,
    };
    parameters.validate()?;
    Ok(parameters)
}

pub(super) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    if !matches!(
        file.file_name().and_then(|v| v.to_str()),
        Some("US_r_Top2field.rel" | "US_m_Top2field.rel" | "US_Top2field.rel")
    ) {
        return Ok(None);
    }
    embedded::write(file, output, FAMILY, &read(&Rel::read(file)?)?).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both extracted original discs; reads controller constants only"]
    fn original_overworld_movement_parameters_match_the_runtime_fixture() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        let expected: MovementParameters = serde_json::from_str(include_str!(
            "../../../content/tests/data/overworld-movement.json"
        ))?;
        for disc in [1, 2] {
            for module in [
                "US_r_Top2field.rel",
                "US_m_Top2field.rel",
                "US_Top2field.rel",
            ] {
                let file = root.join(format!("disc{disc}/files/{module}"));
                let parameters = read(&Rel::read(&file)?)?;
                assert_eq!(parameters, expected, "{disc}/{module}");
                let output = tempfile::tempdir()?;
                cook(&file, output.path())?.expect("supported module");
                assert_eq!(
                    embedded::read::<MovementParameters>(output.path(), FAMILY, module)?,
                    expected
                );
            }
        }
        Ok(())
    }
}
