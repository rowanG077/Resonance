//! Proven Steam scenery spelling aliases in the existing script joint map.
use crate::{model, output, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::field::document::JOINT_MAPPINGS;
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let mut mappings: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../importer/src/model-joints.json"))?;
    for (descriptor, requested) in [
        ("ela_d03b_OBJ000.TOMDLB_D", ["step", "step_br"]),
        ("ela_d03b_OBJ001.TOMDLB_D", ["yuka", "yuka_br"]),
    ] {
        let names = model::Rig::read(&source.require(descriptor)?)?.joint_names();
        for requested in requested {
            let target = requested.to_ascii_uppercase();
            ensure!(
                names
                    .iter()
                    .filter(|name| name.eq_ignore_ascii_case(requested))
                    .count()
                    == 1
                    && names.contains(&target)
                    && !mappings.contains_key(requested),
                "ambiguous scenery joint alias {requested} in {descriptor}"
            );
            mappings.insert(requested.into(), target);
        }
    }
    output::json(&library.join(JOINT_MAPPINGS), &mappings)
        .context("publish Steam scenery joint aliases")
}
