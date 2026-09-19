use super::*;
use resonance_content::ScenePart;
use serde_json::Value;

/// Compare current geometry, textures and sparse curves without schema normalization.
pub(crate) fn compare_scene(
    scene: &ScenePart,
    expected: &Value,
    output: &Path,
    baseline: &Path,
) -> Result<()> {
    let expected: ScenePart = serde_json::from_value(expected.clone())?;
    ensure!(
        serde_json::to_value(scene)? == serde_json::to_value(expected)?,
        "preview scene metadata changed"
    );
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
        .unwrap_or_else(|| local.join("worktrees/generic-cooking/local/all-assets"));
    let expected: Value = serde_json::from_slice(&fs::read(baseline.join("game/menu-data.json"))?)?;
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
        let mut expected = expected["figurines"].clone();
        let mut behavior_cache = symphonia_script_tools::PreparationCache::default();
        let records = expected["records"]
            .as_array_mut()
            .context("frozen figurines")?;
        ensure!(records.len() == actual.records.len(), "figurine count");
        for (record, expected) in actual.records.iter().zip(records) {
            crate::model_behavior::tests::compare_baseline(
                &record.preview,
                &mut expected["preview"],
                &mut behavior_cache,
            )?;
            expected["version"] = record.version.into();
            let parts = expected["preview"]["parts"]
                .as_array()
                .context("frozen parts")?;
            ensure!(
                parts.len() == record.preview.parts.len(),
                "figurine layer count"
            );
            for (index, (part, expected)) in record.preview.parts.iter().zip(parts).enumerate() {
                compare_scene(&part.scene, &expected["scene"], output.path(), &baseline)
                    .with_context(|| format!("disc {disc} figurine {} layer {index}", record.id))?;
            }
        }
        let expected: FigurineBook = serde_json::from_value(expected)?;
        ensure!(
            serde_json::to_value(actual)? == serde_json::to_value(expected)?,
            "disc {disc} figurine metadata differs from immutable baseline"
        );
    }
    Ok(())
}
