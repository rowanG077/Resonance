use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    direction: [f32; 3],
    distance: f32,
    color: [u8; 4],
    release_tick: u16,
    projectile_rule: usize,
    projectile_heading: Option<f32>,
    projectile_height: Option<f32>,
    initial_effect_scale: Option<f32>,
}

pub(super) fn read_parameters(rel: &Rel, kind: Strike) -> Result<Parameters> {
    let (initializer, constants) = match kind {
        Strike::LightningTiger => (0x8b498, 0x8b50),
        Strike::FieryBeast => (0x9045c, 0x95f0),
        Strike::ThunderTiger => (0x926e0, 0x9a58),
    };
    let dispatch = rel.pointer(5, 0xf94 + usize::from(kind.native() - 300) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected combined strike dispatch"
    );
    decode_parameters(kind, rel.at((4, constants))?)
}

fn decode_parameters(kind: Strike, constants: &[u8]) -> Result<Parameters> {
    let (release_tick, projectile_rule) = match kind {
        Strike::LightningTiger => (35, 1),
        Strike::FieryBeast => (66, 2),
        Strike::ThunderTiger => (65, 2),
    };
    Ok(Parameters {
        direction: [
            float(constants, 0)?,
            float(constants, 4)?,
            float(constants, 8)?,
        ],
        distance: float(constants, 20)?,
        color: constants
            .get(12..16)
            .context("truncated strike color")?
            .try_into()?,
        release_tick,
        projectile_rule,
        projectile_heading: (kind != Strike::ThunderTiger)
            .then(|| float(constants, 16))
            .transpose()?,
        projectile_height: (kind == Strike::ThunderTiger)
            .then(|| float(constants, 16))
            .transpose()?,
        initial_effect_scale: (kind != Strike::LightningTiger)
            .then(|| float(constants, 24))
            .transpose()?,
    })
}

pub(super) fn cook(inputs: &Inputs) -> Result<BTreeMap<Strike, StrikeProgram>> {
    Strike::ALL
        .into_iter()
        .map(|kind| {
            let parameters = inputs
                .parameters
                .strikes
                .get(&kind)
                .context("missing strike parameters")?;
            let package = inputs.package(kind.native())?;
            // Only Fiery Beast has models; no initializer or completion installs temporary weapons.
            let models = if kind == Strike::FieryBeast { 2 } else { 0 };
            ensure!(
                package.resources.models[models..]
                    .iter()
                    .all(|&offset| offset == 0)
                    && package.resources.callback_resources == [0; 4],
                "unexpected combined strike native resource"
            );
            let data = program(kind, &package.actions, parameters)?;
            Ok((kind, data))
        })
        .collect()
}

fn program(kind: Strike, source: &Bundle, parameters: &Parameters) -> Result<StrikeProgram> {
    let phases = [phase(source, 0)?, phase(source, 1)?];
    ensure!(
        source.phases[2..].iter().all(|phase| phase.duration == 0),
        "unexpected combined strike role"
    );
    let data = StrikeProgram {
        phases,
        direction: parameters.direction,
        distance: parameters.distance,
        color: parameters.color,
        release_tick: parameters.release_tick,
        projectile_rule: source.rule(parameters.projectile_rule)?,
        projectile_heading: parameters.projectile_heading,
        projectile_height: parameters.projectile_height,
        initial_effect_scale: parameters.initial_effect_scale,
    };
    data.validate(kind)?;
    Ok(data)
}

#[cfg(test)]
mod tests;
