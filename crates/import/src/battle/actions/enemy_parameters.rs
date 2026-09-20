//! Native enemy policy constants; actor settings and voice lengths bind separately.
use super::*;

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some(("US_r_Top2Btl.rel", _)) = crate::battle::embedded::Layout::identify(file) else {
        return Ok(None);
    };
    crate::battle::embedded::write(
        file,
        output,
        "battle-enemy-parameters",
        &enemy::Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0x5508},"parameters_section":4}),
    )
    .map(Some)
}
