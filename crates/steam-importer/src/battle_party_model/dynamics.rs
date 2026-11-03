//! Party chain metadata and pose ownership use the maintained GC renderer rules.
use crate::{animation::Animation, model::Rig, pe};
use anyhow::{Context, Result, ensure};
use resonance_content::secondary_motion::Definition;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

fn source_name(name: &str) -> String {
    static GC: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
        let names: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../../../importer/src/model-joints.json")).unwrap();
        names
            .into_keys()
            .map(|name| (name.to_ascii_uppercase(), name))
            .collect()
    });
    if !name.starts_with("AB_") {
        return GC
            .get(&name.to_ascii_uppercase())
            .cloned()
            .unwrap_or_else(|| name.into());
    }
    // The port uppercases labels and escapes minus/plus in embedded decimal
    // parameters. This projection is only for the GC source-metadata reader;
    // cooked joints retain their existing canonical labels and indices.
    let mut name = name
        .to_ascii_lowercase()
        .replace("x2d_", "-")
        .replace("x2b_", "+")
        .replace("ab_root", "AB_ROOT")
        .replace("ab_", "AB_")
        .replace("_fp_", "_FP_")
        .replace("_nr_", "_NR_")
        .replace("_dt", "_Dt")
        .replace("_bb", "_Bb")
        .replace("_pow", "_Pow");
    for side in ['L', 'R'] {
        name = name.replace(
            &format!("_{}", side.to_ascii_lowercase()),
            &format!("_{side}"),
        );
    }
    // Stripped final parameters leave a side at the end of a root label. Keep
    // that token boundary visible to GC's body-plane side selection.
    if name.ends_with("_L") || name.ends_with("_R") {
        name.push('_');
    }
    name
}

pub(super) fn bind(
    character: u8,
    rig: &Rig,
    friendly: &[String],
    glb: &[u8],
) -> Result<Definition> {
    let tags = [
        "llo00", "col00", "gen00", "ref00", "shi00", "zel00", "pre00", "reg00", "kra00",
    ];
    let tag = tags
        .get(usize::from(character - 1))
        .context("invalid party dynamics owner")?;
    let names: Vec<_> = rig
        .nodes
        .iter()
        .map(|node| source_name(&node.name))
        .collect();
    let length = pe::word(glb, 12)? as usize;
    let json = serde_json::from_slice(glb.get(20..20 + length).context("short party GLB")?)?;
    let definition = resonance_importer::bind_character_dynamics(tag, &json, &names, friendly)?;
    ensure!(
        definition.chains.len() == resonance_importer::character_chain_roots(&names).len(),
        "party chain root count differs"
    );
    Ok(definition)
}

pub(super) fn pose_chains(animation: &Animation, rig: &Rig, dynamics: &Definition) -> Vec<String> {
    let declared: BTreeSet<_> = rig.animation_bindings.iter().copied().collect();
    let roots = rig
        .nodes
        .iter()
        .filter(|node| node.name.starts_with("AB_ROOT"))
        .zip(&dynamics.chains)
        .map(|(node, chain)| (node.binding, &chain.name))
        .collect::<BTreeMap<_, _>>();
    animation
        .targets
        .iter()
        .filter(|target| {
            matches!(target.kind, 3 | 0x10014 | 0x20003)
                && declared.contains(&(target.kind, target.binding))
                && animation.curves[target.curve].times.len() > 2
        })
        .filter_map(|target| roots.get(&target.binding).map(|name| (*name).clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_source_projection_preserves_embedded_numbers_and_gc_side_tokens() {
        assert_eq!(
            source_name("AB_ROOT_FP_01_KAMI03_R_DTX2D_0E3"),
            "AB_ROOT_FP_01_kami03_R_Dt-0e3"
        );
        assert_eq!(source_name("AB_ROOT_FP_01_KATA_L"), "AB_ROOT_FP_01_kata_L_");
        assert_eq!(source_name("BONE_SEBONE02"), "Bone_sebone02");
        assert_eq!(source_name("UnrelatedThing"), "UnrelatedThing");
    }

    #[test]
    #[ignore = "requires installed Steam and cooked party owners; checks native chains and audits remaining varying controllers"]
    fn installed_party_chains_and_pose_controls_use_shared_dynamics() -> Result<()> {
        use crate::{Language, battle_party_mappings, package::Package, source::Source};
        use resonance_content::{
            battle_model::document, diagnostics::Diagnostics, library::Library,
        };
        use std::path::PathBuf;
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let library = Library::open(&root, &[])?;
        let mut remaining = BTreeMap::<u32, usize>::new();
        let mut total = 0;
        for character in 1..=9 {
            let bytes =
                source.require(battle_party_mappings::source_name(&image, character, 0)?)?;
            let package = Package::parse(&bytes)?;
            let rig =
                Rig::read(&source.require(package.reference(0)?.context("missing party body")?)?)?;
            let (_, party) = document::capture_party(
                &mut library.snapshot(),
                character,
                0,
                &Diagnostics::new(true),
                &|| false,
            )?;
            let roots: Vec<_> = rig
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.name.starts_with("AB_ROOT"))
                .collect();
            let dynamics = &party.parts[0].secondary_motion;
            ensure!(
                !roots.is_empty() && dynamics.chains.len() == roots.len(),
                "missing party dynamics"
            );
            ensure!(
                *dynamics == party.parts[1].secondary_motion,
                "body and outline dynamics differ"
            );
            let names = rig.joint_names();
            let chains = dynamics.prepare(&names)?;
            for (chain, (index, _)) in chains.iter().zip(&roots) {
                ensure!(
                    usize::from(chain.joints[0].node) == *index,
                    "chain root changed source index"
                );
            }
            total += roots.len();
            if character == 3 {
                ensure!(
                    chains.iter().any(|chain| chain.joints[0].gravity == -0.3),
                    "lost Genis authored negative gravity"
                );
            }
            if character == 2 {
                ensure!(
                    party.parts[0]
                        .clips
                        .iter()
                        .any(|clip| !clip.pose_chains.is_empty()),
                    "lost authored Colette chain poses"
                );
            }
            // Only varying channels declared by the selected body are relevant;
            // shared clips may contain targets for other models and costumes.
            for member in 2..package.len() {
                let Some(name) = package.reference(member)? else {
                    continue;
                };
                let animation = Animation::read(&source.require(name)?)?;
                for target in &animation.targets {
                    if !matches!(target.kind, 3 | 0x10014 | 0x20003)
                        && rig
                            .animation_bindings
                            .contains(&(target.kind, target.binding))
                        && animation.curves[target.curve]
                            .values
                            .windows(2)
                            .any(|v| v[0] != v[1])
                    {
                        *remaining.entry(target.kind).or_default() += 1;
                    }
                }
            }
        }
        eprintln!(
            "Native party defaults retain {total} source chains; remaining varying non-transform controllers: {remaining:#x?}"
        );
        Ok(())
    }
}
