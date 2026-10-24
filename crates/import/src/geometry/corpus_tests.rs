use super::*;
use crate::all_assets::physical_scene;
use anyhow::{Context, Result, ensure};

#[test]
#[ignore = "requires original Mana scenery; no devices"]
fn mana_flame_nodes_retain_native_camera_facing_flags() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    for disc in [1, 2] {
        let archive = crate::field::MapArchive::open(
            &local.join(format!("disc{disc}/files/MAP/lig_d01.bin")),
        )?;
        let sections = crate::field::sections(&archive.bytes)?;
        let section = &archive.bytes[sections[2].clone().context("flame layer")?];
        let source = section_source(section)?;
        let model = source.model.context("flame skeleton")?;
        assert_eq!(model.nodes.len(), 20);
        assert!(model.nodes.iter().all(|node| node.flags == 0x100));
        let geometry = decode_section(
            section,
            TextureSource::Local {
                catalogue: "lamps/palettes/textures.json".into(),
            },
        )?;
        for node in &geometry.gltf["nodes"].as_array().unwrap()[..20] {
            assert_eq!(node["extras"]["resonance_camera_facing"], true);
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires both original discs and current RESONANCE_TEST_ASSETS; no texture or audio conversion"]
fn original_physical_scenes_match_current_meshes_and_materials() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let library = resonance_content::library::Library::open(&baseline, &[])?;
    // Diagnostic provenance identifies the physical owner when equal native files
    // on two discs share a conversion. All payloads use the current schema.
    let reports = std::env::var_os("RESONANCE_TEST_IMPORT_REPORTS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_IMPORT_REPORTS to the external reports directory")?;
    let sources: std::collections::BTreeMap<String, Vec<String>> =
        serde_json::from_slice(&fs::read(reports.join("import-sources.json"))?)?;
    let output = tempfile::tempdir()?;
    // Repeated instances, a two-UV skin, vertex alpha, two textures, caller
    // textures, field instances, and authored meshes without runtime instances.
    let cases: &[(&str, &str, &[usize])] = &[
        ("lloyd000.bin", "member-00000", &[0]),
        ("shihna003.bin", "member-00000", &[0]),
        ("MAP/tri_i01.bin", "TRI_I01.BIN/member-00002", &[2]),
        ("MAP/dar_d00.bin", "DAR_D00.BIN/member-00002", &[2]),
        (
            "MAP/dar_d00.bin",
            "DAR_D00.BIN/member-00016/member-00000",
            &[16, 0],
        ),
        ("MAP/yum_d00.bin", "YUM_D00.BIN/member-00000", &[0]),
        (
            "MAP/yum_d00.bin",
            "YUM_D00.BIN/member-00028/member-00000",
            &[28, 0],
        ),
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
                let outputs = sources
                    .get(&format!("disc{disc}/{file}"))
                    .context("missing current source provenance")?;
                let suffix = format!("/{leaf}/scene.json");
                let paths = outputs
                    .iter()
                    .filter(|path| {
                        path.starts_with("resources/physical/") && path.ends_with(&suffix)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    paths.len() == 1,
                    "expected one physical scene for {file}/{leaf}"
                );
                let mut snapshot = library.snapshot();
                let expected: resonance_content::physical_scene::Scene =
                    snapshot.json(paths[0], 64 * 1024 * 1024)?;
                expected.validate()?;
                let palette = crate::read::u32(&normalized, 0)? as usize;
                let textures = if crate::read::u32(model, 0)? == 0
                    && crate::read::u32(&normalized, palette + 4)? == 0
                {
                    physical_scene::TextureSource::Caller
                } else {
                    physical_scene::TextureSource::Local {
                        catalogue: match &expected.textures {
                            physical_scene::TextureSource::Local { catalogue } => catalogue.clone(),
                            _ => anyhow::bail!("original local palette became a caller binding"),
                        },
                    }
                };
                let actual = physical_scene::cook(&normalized, output.path(), textures)?;
                let expected_mesh = &expected.mesh;
                let expected = serde_json::to_value(&expected)?;
                let mut metadata = serde_json::to_value(&actual)?;
                metadata["mesh"] = expected["mesh"].clone();
                ensure!(metadata == expected, "physical scene metadata differs");
                let mesh = crate::scene::glb::Glb::read(&output.path().join(&actual.mesh))?;
                let published = crate::scene::glb::Glb::read(&baseline.join(expected_mesh))?;
                for (bone, node) in mesh.json["nodes"]
                    .as_array()
                    .context("model nodes")?
                    .iter()
                    .take(actual.bone_names.len())
                    .enumerate()
                {
                    ensure!(node["extras"]["resonance_bone"] == bone, "bone binding");
                }
                ensure!(
                    mesh.json == published.json && mesh.binary == published.binary,
                    "geometry differs from current publication"
                );
                Ok(())
            };
            check().with_context(|| format!("disc {disc}: {file}/{leaf}"))?;
        }
    }
    Ok(())
}
