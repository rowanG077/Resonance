use super::*;
use std::fs;

fn current_book() -> Result<(std::path::PathBuf, MonsterBook)> {
    use resonance_content::{diagnostics::Diagnostics, library::Library, preparation, prepared};
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS")?;
    let mut snapshot = Library::open(&root, &[])?.snapshot();
    let diagnostics = Diagnostics::new(true);
    preparation::menu_models(&mut snapshot, &diagnostics, &|| false)?;
    let files = prepared::Files::from_snapshot(
        snapshot,
        &mut prepared::Cache::default(),
        || false,
        diagnostics,
    )?;
    let mut data = resonance_content::menu_data::MenuData::load(&files)?;
    Ok((
        root,
        data.presentation
            .monsters
            .take()
            .context("missing monster catalogue")?,
    ))
}

#[test]
#[ignore = "requires RESONANCE_TEST_ASSETS with the current named layout"]
fn catalogue_matches_oracle_statistics_and_uses_converted_assets() -> Result<()> {
    let (root, book) = current_book()?;
    for monster in &book.records {
        let id = monster.id;
        monster.validate(528)?;
        for part in &monster.preview.parts {
            assert!(part.scene.mesh.ends_with(".glb"));
            let model = fs::read(root.join(&part.scene.mesh))?;
            assert_eq!(&model[..4], b"glTF");
            for texture in &part.scene.textures {
                assert!(texture.ends_with(".png") || texture.ends_with(".ktx2"));
                assert!(root.join(texture).is_file());
            }
        }
        // Dolphin's base and repeat-battle pages, including zero-TP/zero-reward records.
        let expected: &[[u32; 6]] = match id {
            0 => &[[7480, 0, 1030, 90, 228, 321]],
            1 => &[[6390, 0, 856, 79, 183, 382]],
            3 => &[[470, 0, 140, 8, 8, 13], [687, 38, 250, 10, 0, 0]],
            36 => &[
                [800, 0, 130, 0, 8, 12],
                [700, 0, 133, 10, 8, 10],
                [2480, 0, 432, 10, 78, 102],
            ],
            _ => &[],
        };
        if !expected.is_empty() {
            assert_eq!(monster.statistics.len(), expected.len());
        }
        for (s, stats) in monster.statistics.iter().zip(expected) {
            assert_eq!(
                [
                    s.hp,
                    s.tp.into(),
                    s.attack.into(),
                    s.defense.into(),
                    s.experience,
                    s.gald
                ],
                *stats
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires extracted discs and current RESONANCE_TEST_ASSETS"]
fn shared_monster_preparation_preserves_every_catalogue_record_and_preview() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let (baseline, expected) = current_book()?;
    for disc in [1, 2] {
        let output = tempfile::tempdir()?;
        let extracted = local.join(format!("extracted/disc{disc}"));
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let sources = crate::source_assets::Sources::read_with(&extracted, &executable)?;
        let usual = fs::read(extracted.join("files").join(&sources.usual))?;
        let shared = crate::battle_effect::SharedPalettes::prepare(&usual, output.path())?;
        let actual = prepare(
            &extracted,
            output.path(),
            &sources,
            &usual,
            &crate::all_assets::monster_catalogue::read(&executable)?,
            &inventory_ui::read(&executable)?,
            &shared,
        )?;
        crate::named::equivalent_assets(
            &actual.book,
            &expected,
            output.path(),
            &baseline,
            &mut BTreeMap::new(),
        )
        .with_context(|| format!("disc {disc} monster catalogue"))?;
        for legacy in ["assets", "data", "sources.json"] {
            ensure!(
                !output.path().join(legacy).exists(),
                "source preparation recreated {legacy}"
            );
        }
    }
    Ok(())
}
