use super::*;

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
