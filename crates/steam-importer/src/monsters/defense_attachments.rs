//! Native Defense System arm roles keep the original indexed rig and motion curves.
use super::*;
use resonance_content::animation::Skeleton;
use std::{collections::BTreeMap, fs, path::PathBuf};

fn binaries(root: &Path, path: &Path, values: &mut BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            binaries(root, &path, values)?;
        } else if path.extension().is_some_and(|extension| extension == "glb") {
            let bytes = fs::read(&path)?;
            ensure!(bytes.starts_with(b"glTF"), "invalid model fixture");
            let start = 20 + pe::word(&bytes, 12)? as usize;
            ensure!(
                &bytes[start + 4..start + 8] == b"BIN\0",
                "missing binary model chunk"
            );
            values.insert(
                path.strip_prefix(root)?.to_owned(),
                bytes[start + 8..].to_vec(),
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires RESONANCE_TEST_STEAM and current RESONANCE_TEST_LIBRARY; republishes only core211"]
fn installed_defense_system_arm_roles_preserve_indices_and_animation_channels() -> Result<()> {
    let source = Source::open(
        &PathBuf::from(
            std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
        ),
        crate::Language::English,
    )?;
    let library = PathBuf::from(
        std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
    );
    let image = pe::Image::parse(&source.executable)?;
    let package = source.require(package(&image, 211)?)?;
    let descriptor =
        preview_model::reference(&package, 0x18)?.context("missing Defense System body")?;
    let rig = crate::model::Rig::read(&source.require(&descriptor)?)?;
    let roles = [
        ("PA00_Arm_R", 6),
        ("PA01_Arm_R", 3),
        ("PA02_Arm_L", 11),
        ("PA03_Arm_L", 9),
    ];
    for (name, index) in roles {
        ensure!(
            rig.nodes[index].name == name.to_ascii_uppercase(),
            "native arm index changed"
        );
        ensure!(
            rig.nodes
                .iter()
                .filter(|node| node.name == name.to_ascii_uppercase())
                .count()
                == 1,
            "native arm cardinality changed"
        );
        ensure!(
            rig.joint_names()[index] == name,
            "canonical arm role missing"
        );
    }
    let owner = library.join(format!("meshes/monsters/{}", symbol(211)));
    let mut before = BTreeMap::new();
    binaries(&owner, &owner, &mut before)?;
    ensure!(
        !before.is_empty(),
        "requires current core211 model/motions for binary comparison"
    );
    models_selected(&source, &library, Some(&[211]))?;
    crate::battle_enemies::cook(&source, &library, Some(&[211]))?;
    let mut after = BTreeMap::new();
    binaries(&owner, &owner, &mut after)?;
    ensure!(
        before == after,
        "arm spelling normalization changed model/index/animation binary data"
    );
    let bytes = fs::read(owner.join("meshes/body.glb"))?;
    let skeleton = Skeleton::from_glb(&bytes)?;
    for (name, index) in roles {
        ensure!(
            skeleton.bones[index].name == name,
            "cooked arm index changed"
        );
        ensure!(
            skeleton
                .bones
                .iter()
                .filter(|bone| bone.name == name)
                .count()
                == 1,
            "cooked arm role cardinality is not one"
        );
    }
    eprintln!(
        "Core211 source {descriptor}: four existing arm roles at6/3/11/9; {} GLB binary chunks unchanged",
        after.len()
    );
    Ok(())
}
