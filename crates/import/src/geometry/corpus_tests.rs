use super::*;
use crate::{all_assets::physical_scene, cooked::Source};
use anyhow::{Context, Result, ensure};

#[test]
#[ignore = "requires both original discs and frozen cook-all scenes; no texture or audio conversion"]
fn original_physical_scenes_match_frozen_meshes_and_materials() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_COOKED")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| local.join("all-assets"));
    let output = tempfile::tempdir()?;
    // Repeated instances, a two-UV skin, vertex alpha, two textures, caller
    // textures, field instances, and authored meshes without runtime instances.
    let cases: &[(&str, &str, &[usize])] = &[
        ("lloyd000.bin", "0", &[0]),
        ("shihna003.bin", "0", &[0]),
        ("MAP/tri_i01.bin", "TRI_I01.BIN/2", &[2]),
        ("MAP/dar_d00.bin", "DAR_D00.BIN/2", &[2]),
        ("MAP/dar_d00.bin", "DAR_D00.BIN/16/0", &[16, 0]),
        ("MAP/yum_d00.bin", "YUM_D00.BIN/0", &[0]),
        ("MAP/yum_d00.bin", "YUM_D00.BIN/28/0", &[28, 0]),
    ];
    for disc in [1, 2] {
        for &(file, leaf, sections) in cases {
            let check = || -> Result<()> {
                let path = local.join(format!("extracted/disc{disc}/files/{file}"));
                let bytes = if file.starts_with("MAP/") {
                    crate::field::MapArchive::open(&path)?.bytes
                } else {
                    fs::read(path)?
                };
                let mut model = bytes.as_slice();
                for &section in sections {
                    let range = crate::field::sections(model)?
                        .get(section)
                        .and_then(Clone::clone)
                        .context("missing original model section")?;
                    model = &model[range];
                }
                let normalized = crate::character::texture_palette(model, model)?;
                let source = Source::open(&baseline, disc, file)?;
                let (directory, expected) = source.resolve(&format!("{leaf}/scene.json"))?;
                let expected: serde_json::Value = serde_json::from_slice(&expected)?;
                let name = format!("{directory}/{leaf}");
                let palette = crate::read::u32(&normalized, 0)? as usize;
                let textures = if crate::read::u32(model, 0)? == 0
                    && crate::read::u32(&normalized, palette + 4)? == 0
                {
                    physical_scene::TextureSource::Caller
                } else {
                    physical_scene::TextureSource::Local {
                        catalogue: format!("{name}/palettes/textures.json"),
                    }
                };
                let actual = physical_scene::cook(&normalized, output.path(), textures)?;
                let expected_mesh = expected["mesh"].as_str().context("missing frozen mesh")?;
                let mut metadata = serde_json::to_value(&actual)?;
                metadata["mesh"] = expected["mesh"].clone();
                ensure!(metadata == expected, "physical scene metadata differs");
                let mut mesh = crate::scene::glb::Glb::read(&output.path().join(&actual.mesh))?;
                let frozen = crate::scene::glb::Glb::read(&baseline.join(expected_mesh))?;
                for (bone, node) in mesh.json["nodes"]
                    .as_array_mut()
                    .context("model nodes")?
                    .iter_mut()
                    .take(actual.bone_names.len())
                    .enumerate()
                {
                    ensure!(node["extras"]["resonance_bone"] == bone, "bone binding");
                    node.as_object_mut().unwrap().remove("extras");
                }
                ensure!(
                    mesh.json == frozen.json && mesh.binary == frozen.binary,
                    "geometry differs from frozen publication"
                );
                Ok(())
            };
            check().with_context(|| format!("disc {disc}: {file}/{leaf}"))?;
        }
    }
    Ok(())
}
