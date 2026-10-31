//! World controller constants directly into editable movement settings.
use crate::dol;
use anyhow::{Context, Result, ensure};
use resonance_content::overworld::{
    CameraDistances, CollisionSettings, FlightZone, FlightZones, MountDistances,
    MovementParameters, Position,
};
use std::{fs, path::Path};

fn section5(bytes: &[u8]) -> Result<&[u8]> {
    let count = dol::u32(bytes, 12)? as usize;
    ensure!(count > 5 && count <= 32, "missing world controller section");
    let table = dol::u32(bytes, 16)? as usize;
    let entry = bytes
        .get(table + 5 * 8..table + 6 * 8)
        .context("truncated REL section table")?;
    let start = (dol::u32(entry, 0)? & !3) as usize;
    let size = dol::u32(entry, 4)? as usize;
    ensure!(start > 0, "world controller section is not stored");
    let end = start
        .checked_add(size)
        .context("world controller section overflow")?;
    bytes
        .get(start..end)
        .context("truncated world controller section")
}

fn convert(bytes: &[u8]) -> Result<MovementParameters> {
    let data = section5(bytes)?;
    let value = |offset| -> Result<f32> { Ok(f32::from_bits(dol::u32(data, offset)?)) };
    let map_position = |x, z| -> Result<[f32; 2]> {
        let [x, y, _] = Position::from_plane([value(x)?, value(z)?, 0.])?.map();
        Ok([x, y])
    };
    ensure!(
        value(0x9f0)? == -value(0x9e8)? && value(0xa00)? == -value(0x9f8)?,
        "asymmetric world controller limits"
    );
    let surface_table = data
        .get(0xcd8..0xcd8 + 16 * 4)
        .context("missing world surface responses")?;
    for id in 0..16 {
        let row = &surface_table[id * 4..id * 4 + 4];
        let ground = if matches!(id, 0 | 8 | 9 | 11 | 12 | 14) {
            -1
        } else {
            id as i8
        };
        let ship = if id == 11 { 11 } else { -1 };
        ensure!(
            row[0] as i8 == ground && row[2] as i8 == ship,
            "unsupported native world surface behavior {id}"
        );
    }
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
        flight_zones: FlightZones {
            sylvarant: Vec::new(),
            tethe_alla: vec![
                FlightZone {
                    position: map_position(0xc2c, 0xc30)?,
                    radius: value(0xb50)?,
                    height: value(0x8e8)?,
                },
                FlightZone {
                    position: map_position(0xc34, 0xc38)?,
                    radius: value(0x954)?,
                    height: value(0xb50)?,
                },
            ],
        },
        turn_limit: value(0x9f8)?,
        flight_turn_step: value(0x9fc)?,
        ship_turn_step: value(0xa08)?,
        camera_distances: CameraDistances {
            close: MountDistances {
                foot: value(0x8e8)?,
                mounted: value(0x944)?,
            },
            distant: MountDistances {
                foot: value(0x948)?,
                mounted: value(0x940)?,
            },
        },
        zoom_transition_ticks: value(0x9e4)?,
        lift_step: value(0xb70)?,
        minimum_lift_step: value(0x904)?,
        descent_easing_height: value(0xb74)?,
        camera_turn_degrees: value(0xb78)?,
        collision: CollisionSettings {
            radius: value(0x8e8)?,
            ship_probe_half_extent: value(0xd44)?,
            ground_probe_half_extent: value(0xd48)?,
        },
        heading_quantization: value(0x958)?,
        camera_yaw_quantization: value(0xa0c)?,
    };
    parameters.validate()?;
    Ok(parameters)
}

pub fn cook_world_movement(root: &Path, library: &Path) -> Result<()> {
    let output = library.join("world/movement.json");
    let settings = convert(&fs::read(root.join("disc1/files/US_r_Top2field.rel"))?)?;
    crate::output::json(&output, &settings)?;
    Ok(())
}
