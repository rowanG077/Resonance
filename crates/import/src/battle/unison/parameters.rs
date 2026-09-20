//! Native callback parameters are decoded during cooking, independently of action bundles.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub pow: BTreeMap<PowWeapon, pow_blade::Parameters>,
    pub thrusts: BTreeMap<Thrust, thrust::Parameters>,
    pub strikes: BTreeMap<Strike, strike::Parameters>,
    pub pairs: BTreeMap<CombinedPair, combined_pair::Parameters>,
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        Ok(Self {
            pow: PowWeapon::ALL
                .into_iter()
                .map(|kind| {
                    pow_blade::read_parameters(rel, kind).map(|parameters| (kind, parameters))
                })
                .collect::<Result<_>>()?,
            thrusts: Thrust::ALL
                .into_iter()
                .map(|kind| thrust::read_parameters(rel, kind).map(|parameters| (kind, parameters)))
                .collect::<Result<_>>()?,
            strikes: Strike::ALL
                .into_iter()
                .map(|kind| strike::read_parameters(rel, kind).map(|parameters| (kind, parameters)))
                .collect::<Result<_>>()?,
            pairs: CombinedPair::ALL
                .into_iter()
                .map(|kind| {
                    combined_pair::read_parameters(rel, kind).map(|parameters| (kind, parameters))
                })
                .collect::<Result<_>>()?,
        })
    }
}

pub(crate) fn cook(file: &Path, output: &Path) -> Result<Option<Vec<String>>> {
    let Some((module, _)) = crate::battle::embedded::Layout::identify(file) else {
        return Ok(None);
    };
    // The other module builds have distinct callback code and constant roots.
    // Their ordinary Unison tables are shared, but these roots are retail-only.
    if module != "US_r_Top2Btl.rel" {
        return Ok(None);
    }
    crate::battle::embedded::write(
        file,
        output,
        "battle-unison-parameters",
        &Parameters::read(&Rel::read(file)?)?,
        serde_json::json!({"dispatch":{"section":5,"offset":0xf94},"parameters_section":4}),
    )
    .map(Some)
}

#[test]
#[ignore = "requires original US retail modules; no media conversion"]
fn original_unison_parameters_publish_checked_callbacks() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let output = crate::temporary_path(&std::env::temp_dir().join("unison-parameters"));
    for disc in ["disc1", "disc2"] {
        let file = local.join(disc).join("files/US_r_Top2Btl.rel");
        let mut rel = Rel::read(&file)?;
        let expected = Parameters::read(&rel)?;
        assert_eq!(
            (
                expected.pow.len(),
                expected.thrusts.len(),
                expected.strikes.len(),
                expected.pairs.len()
            ),
            (3, 3, 3, 8)
        );
        cook(&file, &output)?.context("missing retail Unison parameter publication")?;
        let cooked: Parameters =
            crate::embedded::read(&output, "battle-unison-parameters", "US_r_Top2Btl.rel")?;
        assert_eq!(serde_json::to_vec(&cooked)?, serde_json::to_vec(&expected)?);
        let entry = rel.pointer(5, 0xf98)?;
        rel.pointers.insert(entry, (1, 0));
        assert!(
            Parameters::read(&rel).is_err(),
            "wrong initializer must fail during cooking"
        );
    }
    std::fs::remove_dir_all(output)?;
    Ok(())
}
