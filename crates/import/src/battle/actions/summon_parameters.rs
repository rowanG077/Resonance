//! Named summon callback data shared by physical cooking and selected preparation.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub resume_rate: f32,
    pub ground: [ground_summon::Parameters; 8],
    pub fixed: [summon::Parameters; 2],
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        Ok(Self {
            resume_rate: float(rel.at((4, 0x21c))?, 0)?,
            ground: [
                ground_summon::read_parameters(rel, 284)?,
                ground_summon::read_parameters(rel, 285)?,
                ground_summon::read_parameters(rel, 286)?,
                ground_summon::read_parameters(rel, 287)?,
                ground_summon::read_parameters(rel, 288)?,
                ground_summon::read_parameters(rel, 289)?,
                ground_summon::read_parameters(rel, 291)?,
                ground_summon::read_parameters(rel, 293)?,
            ],
            fixed: [
                summon::read_parameters(rel, 290)?,
                summon::read_parameters(rel, 292)?,
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
        "battle-summon-parameters",
        &Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0x1238},"parameters_section":4}),
    )
    .map(Some)
}
