use super::*;
use crate::figurines::tests::compare_scene;
use serde_json::Value;

#[test]
#[ignore = "requires locally cooked monster assets"]
fn catalogue_matches_oracle_statistics_and_uses_converted_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked");
    for id in 0..MONSTER_COUNT as u8 {
        let monster: Monster =
            serde_json::from_slice(&fs::read(root.join(format!("monsters/{id:03}.json"))).unwrap())
                .unwrap();
        monster.validate(528).unwrap();
        assert_eq!(monster.id, id);
        for part in &monster.preview.parts {
            assert!(part.scene.mesh.ends_with(".glb"));
            let model = fs::read(root.join(&part.scene.mesh)).unwrap();
            assert_eq!(&model[..4], b"glTF");
            for texture in &part.scene.textures {
                assert!(texture.ends_with(".ktx2"));
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
}

#[test]
#[ignore = "requires extracted discs and prepared assets; RESONANCE_COOKED selects the baseline"]
fn shared_monster_preparation_preserves_every_catalogue_record_and_preview() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let baseline = std::env::var_os("RESONANCE_COOKED")
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
        let mut expected = expected["monsters"].clone();
        let mut behavior_cache = symphonia_script_tools::PreparationCache::default();
        let records = expected["records"]
            .as_array_mut()
            .context("frozen monster records")?;
        ensure!(records.len() == actual.records.len(), "monster count");
        for (monster, expected) in actual.records.iter().zip(records) {
            crate::model_behavior::tests::compare_baseline(
                &monster.preview,
                &mut expected["preview"],
                &mut behavior_cache,
            )?;
            expected["version"] = monster.version.into();
            let parts = expected["preview"]["parts"]
                .as_array_mut()
                .context("frozen preview parts")?;
            ensure!(
                parts.len() == monster.preview.parts.len(),
                "monster {} layer count",
                monster.id
            );
            for (index, (part, expected)) in monster.preview.parts.iter().zip(parts).enumerate() {
                compare_scene(&part.scene, &expected["scene"], output.path(), &baseline)
                    .with_context(|| format!("disc {disc} monster {} layer {index}", monster.id))?;
            }
        }
        let expected: MonsterBook = serde_json::from_value(expected)?;
        ensure!(
            serde_json::to_value(&actual)? == serde_json::to_value(expected)?,
            "disc {disc} monsters metadata differs from the prepared baseline"
        );
        for legacy in ["assets", "data", "sources.json"] {
            ensure!(
                !output.path().join(legacy).exists(),
                "source preparation recreated {legacy}"
            );
        }
    }
    Ok(())
}
