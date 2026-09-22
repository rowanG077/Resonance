use super::*;
use resonance_content::ScenePart;

pub(crate) fn compare_scene_assets(
    scene: &ScenePart,
    output: &Path,
    baseline: &Path,
) -> Result<()> {
    for path in std::iter::once(&scene.mesh)
        .chain(&scene.textures)
        .chain(scene.clips.iter().map(|clip| &clip.motion))
    {
        ensure!(
            fs::read(output.join(path))? == fs::read(baseline.join(path))?,
            "preview asset changed: {path}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires extracted discs and immutable RESONANCE_FIELD_BASELINE figurine assets"]
fn shared_previews_bind_every_figurine_and_original_idle() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_FIELD_BASELINE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| local.join("all-assets"));
    let expected: resonance_content::menu_data::MenuData =
        serde_json::from_slice(&fs::read(baseline.join("game/menu-data.json"))?)?;
    for disc in [1, 2] {
        let output = tempfile::tempdir()?;
        let extracted = local.join(format!("extracted/disc{disc}"));
        let actual = prepare(
            &extracted,
            output.path(),
            &fs::read(extracted.join("sys/main.dol"))?,
        )?;
        for legacy in ["assets", "data", "sources.json"] {
            ensure!(
                !output.path().join(legacy).exists(),
                "source preparation recreated {legacy}"
            );
        }
        ensure!(
            serde_json::to_value(&actual)? == serde_json::to_value(&expected.figurines)?,
            "disc {disc} figurine metadata differs from immutable baseline"
        );
        for record in &actual.records {
            for (index, part) in record.preview.parts.iter().enumerate() {
                compare_scene_assets(&part.scene, output.path(), &baseline)
                    .with_context(|| format!("disc {disc} figurine {} layer {index}", record.id))?;
            }
        }
    }
    Ok(())
}
