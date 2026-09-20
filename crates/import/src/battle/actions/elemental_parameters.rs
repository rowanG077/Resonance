//! Native elemental callback data, decoded once before selected action preparation.
use super::*;
use resonance_content::battle::actions::acid_rain::AcidRainRecipe;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub spread: stored_parameters::Ground,
    pub air_thrust: air_thrust::Parameters,
    pub lightning: [lightning::Parameters; 4],
    pub ice_tornado: stored_parameters::Ground,
    pub freeze_lancer: freeze_lancer::Parameters,
    pub spiral_flare: spiral_flare::Parameters,
    pub thunder_arrow: thunder_arrow::Parameters,
    pub acid_rain: AcidRainRecipe,
    pub ground_pulses: [ground_pulse::Parameters; 4],
    pub ray: ray::Parameters,
    pub genis_final: [genis_final::Parameters; 3],
    pub prism: prism::Parameters,
    pub orbs: [orb::Parameters; 2],
    pub lances: [lance::Parameters; 2],
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        Ok(Self {
            spread: spread::read_parameters(rel)?,
            air_thrust: air_thrust::read_parameters(rel)?,
            lightning: [
                lightning::read_parameters(rel, 216)?,
                lightning::read_parameters(rel, 217)?,
                lightning::read_parameters(rel, 218)?,
                lightning::read_parameters(rel, 219)?,
            ],
            ice_tornado: ice_tornado::read_parameters(rel)?,
            freeze_lancer: freeze_lancer::read_parameters(rel)?,
            spiral_flare: spiral_flare::read_parameters(rel)?,
            thunder_arrow: thunder_arrow::read_parameters(rel)?,
            acid_rain: acid_rain::read_parameters(rel)?,
            ground_pulses: [
                ground_pulse::read_parameters(rel, 223)?,
                ground_pulse::read_parameters(rel, 224)?,
                ground_pulse::read_parameters(rel, 228)?,
                ground_pulse::read_parameters(rel, 229)?,
            ],
            ray: ray::read_parameters(rel)?,
            genis_final: [
                genis_final::read_parameters(rel, 230)?,
                genis_final::read_parameters(rel, 231)?,
                genis_final::read_parameters(rel, 233)?,
            ],
            prism: prism::read_parameters(rel)?,
            orbs: [
                orb::read_parameters(rel, 251)?,
                orb::read_parameters(rel, 278)?,
            ],
            lances: [
                lance::read_parameters(rel, 253)?,
                lance::read_parameters(rel, 283)?,
            ],
        })
    }
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((module, _)) = crate::battle::embedded::Layout::identify(file) else {
        return Ok(None);
    };
    if module != "US_r_Top2Btl.rel" {
        return Ok(None);
    }
    crate::battle::embedded::write(
        file,
        output,
        "battle-elemental-spell-parameters",
        &Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0x1238},"parameters_section":4}),
    )
    .map(Some)
}
