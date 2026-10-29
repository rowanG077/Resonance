use super::*;
use serde_json::Value;

#[test]
#[ignore = "requires installed F14 metadata and retained originals; no media conversion"]
fn closing_defaults_preserve_enemy_policies_and_monster_catalogue() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let library = std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?;
    let library = Path::new(&library);
    let evidence = root.join("local/structure-review-20261027/fixed-ledger");
    let read = |p: &Path| -> Result<Value> { Ok(serde_json::from_slice(&fs::read(p)?)?) };
    let migration = read(&evidence.join("closing-migration.json"))?;
    let mut policies = 0;
    for row in migration["files"].as_array().unwrap() {
        let path = row["path"].as_str().unwrap();
        if path.starts_with("battle/enemies/") && path.ends_with("/enemy.json") {
            let before: crate::battle_enemy::rules::Rules =
                serde_json::from_value(read(&evidence.join("before").join(path))?["rules"].take())?;
            let after: crate::battle_enemy::rules::Rules =
                serde_json::from_value(read(&library.join(path))?["rules"].take())?;
            assert!(
                serde_json::to_value(before)? == serde_json::to_value(after)?,
                "enemy rules changed: {path}"
            );
            policies += 1;
        }
    }
    assert_eq!(policies, 200);

    let mods = std::env::var_os("RESONANCE_TEST_MOD_ASSETS")
        .map(std::path::PathBuf::from)
        .into_iter()
        .collect::<Vec<_>>();
    let mut snapshot = Library::open_paths(library, &mods)?.snapshot();
    let diagnostics = crate::diagnostics::Diagnostics::new(true);
    crate::preparation::menu_resources(&mut snapshot, &diagnostics, &|| false)?;
    let files = crate::prepared::Files::from_snapshot(snapshot, || false, diagnostics)?;
    let after = crate::monster::MonsterBook::decode_named(
        files.json::<Value>(crate::menu_data::MONSTER_BOOK_PATH)?,
        &files,
    )?;

    // The model-authoring corpus independently compares all 251 preview owners,
    // including the 12 changed here, with their retained settings and GLB media.
    // This test keeps catalogue values/defaults and record-to-preview ownership;
    // it uses current models so later media sharing does not obscure those checks.
    let before =
        read(&evidence.join("before/game/menu-data.json"))?["presentation"]["monsters"].take();
    let (before, paths) = crate::model_preview::document::expand_catalogue(before, |path| {
        Ok(fs::read(library.join(path))?.into())
    })?;
    let before: crate::monster::MonsterBook = serde_json::from_value(before)?;
    assert_eq!(after.records.len(), 251);
    assert_eq!(
        paths,
        after
            .records
            .iter()
            .map(|r| r.preview.document.clone().unwrap())
            .collect::<Vec<_>>()
    );
    assert!(
        serde_json::to_value(before)? == serde_json::to_value(after)?,
        "monster catalogue values changed"
    );
    eprintln!(
        "{policies} enemy policies and all 251 Monster List entries preserve typed values and preview owners"
    );
    Ok(())
}
