use super::*;
#[test]
#[ignore = "requires extracted discs and current RESONANCE_TEST_ASSETS"]
fn shared_previews_bind_every_figurine_and_original_idle() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    use resonance_content::{diagnostics::Diagnostics, library::Library, preparation, prepared};
    let baseline = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let mut snapshot = Library::open(&baseline, &[])?.snapshot();
    let value = snapshot.json(
        resonance_content::menu_data::FIGURINES_PATH,
        preparation::JSON_LIMIT,
    )?;
    resonance_content::model_preview::document::capture_catalogue(&value, &mut snapshot, &|| {
        false
    })?;
    let files = prepared::Files::from_snapshot(
        snapshot,
        &mut prepared::Cache::default(),
        || false,
        Diagnostics::new(true),
    )?;
    let expected = FigurineBook::load(&files)?;
    for disc in [1, 2] {
        let output = tempfile::tempdir()?;
        let extracted = local.join(format!("extracted/disc{disc}"));
        let actual = prepare(
            &extracted,
            output.path(),
            &crate::all_assets::figurine_catalogue::read(&fs::read(
                extracted.join("sys/main.dol"),
            )?)?,
        )?;
        for legacy in ["assets", "data", "sources.json"] {
            ensure!(
                !output.path().join(legacy).exists(),
                "source preparation recreated {legacy}"
            );
        }
        crate::named::equivalent_assets(
            &actual,
            &expected,
            output.path(),
            &baseline,
            &mut BTreeMap::new(),
        )
        .with_context(|| format!("disc {disc} figurine catalogue"))?;
    }
    Ok(())
}
