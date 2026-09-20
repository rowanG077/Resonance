use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    directions: [[f32; 3]; 2],
    distance: f32,
    color: [u8; 4],
    feedback_at: u16,
}

pub(super) fn read_parameters(rel: &Rel, kind: Thrust) -> Result<Parameters> {
    let (initializer, color, angles, distance, feedback_at) = match kind {
        Thrust::Cross => (0x82cbc, 0x7118, None, 0x711c, 40),
        Thrust::Mirage => (0x8345c, 0x73d8, Some([0x73e0, 0x73f0]), 0x73ec, 40),
        Thrust::Dark => (0x83720, 0x7448, Some([0x7450, 0x7460]), 0x745c, 60),
    };
    let dispatch = rel.pointer(5, 0xf94 + usize::from(kind.native() - 300) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected combined thrust dispatch"
    );
    let directions = if let Some(angles) = angles {
        let direction = |offset| -> Result<[f32; 3]> {
            let angle = f64::from_be_bytes(rel.at((4, offset))?[..8].try_into()?);
            Ok([angle.cos() as f32, 0., angle.sin() as f32])
        };
        [direction(angles[0])?, direction(angles[1])?]
    } else {
        let direction = |offset| -> Result<[f32; 3]> {
            let bytes = rel.at((4, offset))?;
            Ok([float(bytes, 0)?, float(bytes, 4)?, float(bytes, 8)?])
        };
        [direction(0x7100)?, direction(0x710c)?]
    };
    Ok(Parameters {
        directions,
        distance: float(rel.at((4, distance))?, 0)?,
        color: rel.at((4, color))?[..4].try_into()?,
        feedback_at,
    })
}

pub(super) fn cook(inputs: &Inputs) -> Result<BTreeMap<Thrust, ThrustProgram>> {
    Thrust::ALL
        .into_iter()
        .map(|kind| {
            let parameters = inputs
                .parameters
                .thrusts
                .get(&kind)
                .context("missing thrust parameters")?;
            let package = inputs.package(kind.native())?;
            ensure!(
                package.resources.models == [0; 10]
                    && package.resources.callback_resources == [0; 4],
                "unexpected combined thrust native resource"
            );
            let source = &package.actions;
            let count = if kind == Thrust::Cross { 2 } else { 4 };
            let phases = (0..count)
                .map(|index| phase(source, index))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                source.phases[count..]
                    .iter()
                    .all(|phase| phase.duration == 0),
                "unexpected combined thrust role"
            );
            Ok((
                kind,
                ThrustProgram {
                    phases,
                    directions: parameters.directions,
                    distance: parameters.distance,
                    color: parameters.color,
                    feedback_at: parameters.feedback_at,
                },
            ))
        })
        .collect()
}
