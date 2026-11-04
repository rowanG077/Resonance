//! Proven Steam scenery spelling aliases in the existing script joint map.
use crate::{model, output, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::field::document::JOINT_MAPPINGS;
use std::{collections::BTreeMap, path::Path};

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let mut mappings: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../importer/src/model-joints.json"))?;
    for (descriptor, requested) in [
        ("ela_d03b_OBJ000.TOMDLB_D", &["step", "step_br"][..]),
        ("ela_d03b_OBJ001.TOMDLB_D", &["yuka", "yuka_br"][..]),
        ("hol_d03_OBJ000.TOMDLB_D", &["temae_kabe"][..]),
        ("elc_d16_OBJ000.TOMDLB_D", &["gareki"][..]),
    ] {
        let names = model::Rig::read(&source.require(descriptor)?)?.joint_names();
        for &requested in requested {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed Steam; writes only a temporary ritual scenery probe"]
    fn installed_ritual_debris_alias_controls_only_its_source_mesh() -> Result<()> {
        let installation =
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?;
        let source = Source::open(Path::new(&installation), crate::Language::English)?;
        let temporary = tempfile::tempdir()?;
        cook(&source, temporary.path())?;
        let mappings: BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(temporary.path().join(JOINT_MAPPINGS))?)?;
        let descriptor = "elc_d16_OBJ000.TOMDLB_D";
        let native = model::Rig::read(&source.require(descriptor)?)?;
        assert_eq!(native.nodes[28].name, "GAREKI");
        let (mut part, _) = model::cook(
            &source,
            temporary.path(),
            descriptor,
            "fields/tower-of-salvation-0184",
            "part-0",
        )?;
        part.prepare_bones(&std::fs::read(temporary.path().join(&part.mesh))?)?;
        let target = &mappings["gareki"];
        assert_eq!(target, "GAREKI");
        assert_eq!(
            part.bone_names.iter().position(|name| name == target),
            Some(28)
        );
        assert_eq!(
            part.bone_names
                .iter()
                .filter(|name| *name == target)
                .count(),
            1
        );
        assert_eq!(
            part.material_nodes
                .iter()
                .enumerate()
                .filter_map(|(mesh, nodes)| nodes.contains(&28).then_some(mesh))
                .collect::<Vec<_>>(),
            [26]
        );
        assert_eq!(part.materials.len(), 27);
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam; writes only a temporary source-model probe"]
    fn installed_verius_foreground_alias_keeps_both_mesh_bindings() -> Result<()> {
        let installation =
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?;
        let source = Source::open(Path::new(&installation), crate::Language::English)?;
        let temporary = tempfile::tempdir()?;
        cook(&source, temporary.path())?;
        let mappings: BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(temporary.path().join(JOINT_MAPPINGS))?)?;
        let descriptor = "hol_d03_OBJ000.TOMDLB_D";
        let native = model::Rig::read(&source.require(descriptor)?)?;
        assert_eq!(native.nodes[4].name, "TEMAE_KABE");
        let (mut part, _) = model::cook(
            &source,
            temporary.path(),
            descriptor,
            "fields/martel-temple-0309",
            "part-0",
        )?;
        part.prepare_bones(&std::fs::read(temporary.path().join(&part.mesh))?)?;
        let target = &mappings["temae_kabe"];
        assert_eq!(target, "TEMAE_KABE");
        assert_eq!(
            part.bone_names.iter().position(|name| name == target),
            Some(4)
        );
        assert_eq!(
            part.bone_names
                .iter()
                .filter(|name| *name == target)
                .count(),
            1
        );
        // The original script hides this node, including its second draw.
        for mesh in [4, 5] {
            assert!(part.material_nodes[mesh].contains(&4));
        }
        assert_eq!(part.materials.len(), 6);
        Ok(())
    }
}
