//! Named martial callback data; action tracks and hit rules stay in their bundles.
use super::*;

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some(("US_r_Top2Btl.rel", _)) = crate::battle::embedded::Layout::identify(file) else {
        return Ok(None);
    };
    crate::battle::embedded::write(
        file,
        output,
        "battle-martial-parameters",
        &martial::Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0xd60},"parameters_section":4}),
    )
    .map(Some)
}
