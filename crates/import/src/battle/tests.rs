use super::*;

#[test]
fn selected_encounters_reuse_a_formation_in_distinct_arenas() {
    let mut selection = CookSelection {
        encounters: [13, 14]
            .map(|arena| Encounter {
                formation: 0,
                arena,
            })
            .into(),
        ..CookSelection::default()
    };
    selection.validate().unwrap();
    let mut table = formations::FormationTable {
        formations: vec![formations::Record {
            actor_count: 1,
            resource_count: 1,
            flags: 5,
            hidden_names: 1,
            resources: [36, 0, 0, -1],
            ..Default::default()
        }],
    };
    let output = crate::temporary_path(&std::env::temp_dir().join("formation-binding"));
    let path = output.join("assets/shared/battle/all/usual/1.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&table).unwrap()).unwrap();
    fs::write(
        output.join("sources.json"),
        serde_json::to_vec(&serde_json::json!({
            "disc1/BTL/Renamed.dat": ["assets/shared"]
        }))
        .unwrap(),
    )
    .unwrap();
    let sources: all::Sources = serde_json::from_value(serde_json::json!({
        "usual": "BTL/Renamed.dat", "enemy": "unused", "archives": ["unused", "unused", "unused", "unused"]
    })).unwrap();
    let bound = formations::FormationTable::bind(&output, 1, &sources).unwrap();
    assert_eq!(bound, table);
    assert!(
        selected_formations(
            &bound,
            &[Encounter {
                formation: 1,
                arena: 13
            }],
            None
        )
        .is_err()
    );
    let formations = selected_formations(&bound, &selection.encounters, None).unwrap();
    assert_eq!(
        formations
            .iter()
            .map(|f| (f.id, f.arena))
            .collect::<Vec<_>>(),
        [(0, 13), (0, 14)]
    );
    assert_eq!(formations[0].enemies, formations[1].enemies);
    table.formations[0].actor_count = 9;
    fs::write(&path, serde_json::to_vec(&table).unwrap()).unwrap();
    assert!(formations::FormationTable::bind(&output, 1, &sources).is_err());
    fs::remove_file(path).unwrap();
    assert!(formations::FormationTable::bind(&output, 1, &sources).is_err());
    fs::remove_dir_all(output).unwrap();
    selection.encounters.push(selection.encounters[0]);
    assert_eq!(
        selection.validate().unwrap_err().to_string(),
        "duplicate selected encounter"
    );
}

#[test]
fn formation_resolves_slots_variants_and_signed_xz_pairs() {
    let mut row = formations::Record {
        actor_count: 2,
        resource_count: 2,
        flags: 5,
        hidden_names: 3,
        resources: [49, 36, 0, -1],
        ..Default::default()
    };
    row.actors[0] = formations::Actor {
        resource: 1,
        appearance: 3,
        position: [-300, 20],
        ..Default::default()
    };
    row.actors[1] = formations::Actor {
        variant: 1,
        appearance: 1,
        position: [500, -100],
        ..Default::default()
    };
    row.actors[7].resource = 255;
    row.actors[7].position = [i16::MIN, i16::MAX];
    let result = formation(&row, 2, 13, None).unwrap();
    assert!(!result.escape_allowed);
    assert!(result.opening_voice);
    assert_eq!(result.placement, FormationPlacement::Explicit);
    assert!(result.victory_music && result.victory_camera && result.victory_celebration);
    assert_eq!(
        result.enemies,
        [
            EnemySpawn {
                monster: 36,
                variant: 0,
                texture_variant: 3,
                name_visible: false,
                position: [-300., 20.]
            },
            EnemySpawn {
                monster: 49,
                variant: 1,
                texture_variant: 1,
                name_visible: false,
                position: [500., -100.]
            },
        ]
    );
    row.flags = 4;
    row.hidden_names = 1;
    let result = formation(&row, 2, 13, None).unwrap();
    assert!(result.escape_allowed);
    assert!(result.enemies[0].name_visible);
    assert!(!result.enemies[1].name_visible);
    row.actors[0].attachments[0] = 1;
    assert!(
        formation(&row, 2, 13, None)
            .unwrap_err()
            .to_string()
            .contains("attachment overrides")
    );
    row.actors[0].attachments[0] = 0;
    row.actors[0].resource = 2;
    assert!(formation(&row, 2, 13, None).is_err());
    row.actors[0].resource = 0;
    row.resources[0] = -1;
    assert!(formation(&row, 2, 13, None).is_err());
    row.resources[0] = 49;
    row.flags |= 2;
    assert!(formation(&row, 2, 13, None).is_err());
}

#[test]
fn undine_formation_preserves_generated_placement_and_opening_voice_suppression() {
    let layout = GeneratedEnemyLayout {
        row_x: [100., 350., 600.],
        member_offset: [50., -250.],
        back_row_x_correction: 100.,
        center_z: 200.,
        single_z: [150., -150., 150.],
        main_z: [0., -250., 250., -450., -650., 450., 650., -800.],
    };
    let mut row = formations::Record {
        actor_count: 1,
        resource_count: 1,
        flags: 0x43,
        resources: [195, 0, 0, 0],
        ..Default::default()
    };
    let result = formation(&row, 24, 13, Some(&layout)).unwrap();
    assert_eq!(
        result.placement,
        FormationPlacement::Generated {
            layout: layout.clone()
        }
    );
    assert!(!result.escape_allowed);
    assert!(!result.opening_voice);
    assert!(result.victory_music && result.victory_camera && result.victory_celebration);
    assert_eq!(result.enemies[0].monster, 195);
    assert_eq!(result.enemies[0].position, [0.; 2]);
    assert!(formation(&row, 24, 13, None).is_err());
    row.flags = 0x45;
    let explicit = formation(&row, 24, 13, None).unwrap();
    assert_eq!(explicit.placement, FormationPlacement::Explicit);
    assert!(!explicit.opening_voice);
    row.flags = 0x4b;
    assert_eq!(
        serde_json::to_value(formation(&row, 24, 13, Some(&layout)).unwrap()).unwrap(),
        serde_json::to_value(&result).unwrap(),
        "unused header bit must not alter runtime formation fields"
    );
    row.flags = 0x53;
    let continued_music = formation(&row, 24, 13, Some(&layout)).unwrap();
    assert!(!continued_music.victory_music);
    assert!(continued_music.victory_camera && continued_music.victory_celebration);
    assert_eq!(continued_music.placement, result.placement);
    assert_eq!(continued_music.enemies, result.enemies);
    for flags in [0xc3, 0xd3] {
        row.flags = flags;
        let no_celebration = formation(&row, 24, 13, Some(&layout)).unwrap();
        assert!(!no_celebration.victory_celebration);
        assert!(no_celebration.victory_camera);
        assert_eq!(no_celebration.victory_music, flags == 0xc3);
        assert_eq!(no_celebration.placement, result.placement);
        assert_eq!(no_celebration.enemies, result.enemies);
    }
    for flags in [0x63, 0xf3] {
        row.flags = flags;
        let error = formation(&row, 24, 13, Some(&layout))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("formation 24:") && error.contains(&format!("flags {:#04x}", row.flags)),
            "{error}"
        );
    }
}

#[test]
#[ignore = "requires locally extracted GameCube assets"]
fn original_wind_formations_preserve_all_resources_and_distinct_stat_variants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let directory = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
    let rel = actions::Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
    let (_, layout) = placement::read(&rel, &embedded::Layout::RETAIL)
        .unwrap()
        .bind()
        .unwrap();
    let table = formations::FormationTable::read(actions::member(&directory, 1).unwrap()).unwrap();
    assert_eq!(
        table
            .formations
            .iter()
            .enumerate()
            .filter_map(|(id, row)| (row.flags & 8 != 0).then_some(id))
            .collect::<Vec<_>>(),
        [25]
    );
    for (id, flags, variant) in [(25, 0x0b, 0), (36, 0x03, 1)] {
        let row = &table.formations[id];
        assert_eq!(
            (
                row.actor_count,
                row.resource_count,
                row.flags,
                row.hidden_names
            ),
            (3, 3, flags, 0)
        );
        assert_eq!(
            row.actors[..3]
                .iter()
                .map(|actor| actor.resource)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let cooked = formation(row, id as u16, 13, Some(&layout)).unwrap();
        assert_eq!(
            cooked.placement,
            FormationPlacement::Generated {
                layout: layout.clone()
            }
        );
        assert!(!cooked.escape_allowed);
        assert!(
            cooked.opening_voice
                && cooked.victory_music
                && cooked.victory_camera
                && cooked.victory_celebration
        );
        assert_eq!(
            cooked
                .enemies
                .iter()
                .map(|e| (e.monster, e.variant, e.name_visible, e.position))
                .collect::<Vec<_>>(),
            [205, 206, 207].map(|id| (id, variant, true, [0.; 2]))
        );
    }
    // Actual formation-header consumers: opening, escape, placement and victory.
    // These exact masks exclude0x08; a changed binary must not inherit this audit.
    for (offset, instruction) in [
        (0x5450, 0x54000673),
        (0x5954, 0x540007ff),
        (0x43b0c, 0x540007bd),
        (0x56524, 0x540006f7),
        (0x57320, 0x546006b5),
        (0x57328, 0x54600631),
        (0x591c8, 0x540006b5),
        (0x599b0, 0x540006b5),
        (0x59c04, 0x540006b5),
        (0x5a0f8, 0x540006f7),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
}

#[test]
#[ignore = "requires locally extracted GameCube assets"]
fn original_lightning_rosters_keep_body_rows_separate_from_stat_variants() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let directory = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
    let rel = actions::Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
    let (_, layout) = placement::read(&rel, &embedded::Layout::RETAIL)
        .unwrap()
        .bind()
        .unwrap();
    let table = formations::FormationTable::read(actions::member(&directory, 1).unwrap()).unwrap();
    type Roster = [(u8, u8, u8)];
    let cases: &[(u16, &Roster)] = &[
        (1, &[(36, 0, 0)]),
        (2, &[(49, 1, 0), (36, 0, 0)]),
        (23, &[(197, 0, 0)]),
        (24, &[(195, 0, 0)]),
        (25, &[(205, 0, 0), (206, 0, 0), (207, 0, 0)]),
        (32, &[(244, 0, 0)]),
        (36, &[(205, 1, 0), (206, 1, 0), (207, 1, 0)]),
        (39, &[(197, 1, 0)]),
        (53, &[(194, 0, 0)]),
        (61, &[(36, 0, 0), (101, 0, 1), (101, 0, 1)]),
        (72, &[(223, 0, 0), (193, 0, 0), (193, 0, 0)]),
        (126, &[(36, 0, 0)]),
        (271, &[(73, 0, 1)]),
        (272, &[(73, 1, 1)]),
        (274, &[(73, 2, 0)]),
        (295, &[(100, 4, 3), (104, 1, 3), (104, 1, 3), (101, 1, 3)]),
        (301, &[(72, 0, 0), (72, 0, 0), (51, 0, 0)]),
        (418, &[(107, 0, 0)]),
        (444, &[(247, 0, 0), (247, 0, 0), (250, 0, 0), (250, 0, 0)]),
        (460, &[(172, 1, 0)]),
        (489, &[(39, 0, 0), (52, 0, 0)]),
        (698, &[(172, 0, 0)]),
    ];
    for &(id, expected) in cases {
        let row = &table.formations[usize::from(id)];
        assert!(row.actors.iter().all(|actor| actor.attachments == [0; 2]));
        let actual = formation(row, id, 13, Some(&layout)).unwrap();
        if matches!(id, 23 | 39 | 53) {
            assert_eq!(row.flags, if id == 53 { 0x13 } else { 0x43 });
            assert!(!actual.escape_allowed);
            assert_eq!(actual.opening_voice, id == 53);
            assert_eq!(actual.victory_music, id != 53);
            assert!(actual.victory_camera && actual.victory_celebration);
        }
        if id == 32 {
            assert_eq!(row.flags, 0xd3);
            assert!(!actual.victory_music && !actual.opening_voice && !actual.escape_allowed);
            assert!(!actual.victory_celebration && actual.victory_camera);
        }
        if id == 72 {
            assert_eq!(row.flags, 0x53);
            assert!(!actual.victory_music && !actual.opening_voice && !actual.escape_allowed);
            assert!(actual.victory_camera && actual.victory_celebration);
        }
        assert_eq!(
            actual
                .enemies
                .iter()
                .map(|e| (e.monster, e.variant, e.texture_variant))
                .collect::<Vec<_>>(),
            expected,
            "formation{id}"
        );
    }
}

fn source_enemy_records(
    bytes: &[u8],
) -> Result<(
    all::ActorSettings,
    all::EnemyStatistics,
    all::EnemyResources,
    all::EnemyVariants,
)> {
    let settings = all::ActorSettings::read(&bytes[usize::from(half(bytes, 4)?)..])?;
    let stats = usize::from(half(bytes, 6)?);
    let stats = all::EnemyStatistics::read(
        bytes
            .get(stats..stats + 60)
            .context("truncated enemy statistics")?,
    )?;
    let resources = all::EnemyResources::read(bytes)?;
    let variants = all::EnemyVariants::read(
        if resources.variant_offset == 0 {
            &[]
        } else {
            bytes
                .get(resources.variant_offset as usize..)
                .context("missing enemy variant table")?
        },
        usize::from(settings.combat.variant_count),
    )?;
    Ok((settings, stats, resources, variants))
}

fn source_enemy_data(bytes: &[u8]) -> Result<EnemyData> {
    let (settings, stats, resources, variants) = source_enemy_records(bytes)?;
    enemy_data(&settings, &stats, &resources, &variants.variants)
}

#[test]
fn enemy_data_preserves_shared_roll_thresholds_and_variant_stats() {
    let mut bytes = vec![0; 0x300];
    bytes[..8].copy_from_slice(b"em8\0\x00\x20\x02\x10");
    bytes[0x20 + 0x4e..0x20 + 0x50].copy_from_slice(&[25, 75]);
    bytes[0x20 + 0x50] = 60;
    bytes[0x20 + 0x94..0x20 + 0x96].copy_from_slice(&[90, 30]);
    bytes[0x20 + 0x5c..0x20 + 0x60].copy_from_slice(&0x20_0001_u32.to_be_bytes());
    bytes[0x20 + 0x28] = 2;
    bytes[0x20 + 0x2d..0x20 + 0x2f].copy_from_slice(&[12, 34]);
    bytes[0x20 + 0x55] = 56;
    bytes[0x20 + 0x11b] = 3;
    bytes[0x20 + 0x1ea..0x20 + 0x1ec].copy_from_slice(&(-5i16).to_be_bytes());
    bytes[0x210 + 0x1d] = 3;
    bytes[0x214..0x21b].copy_from_slice(b"Zombie\0");
    bytes[0x210] = 1;
    bytes[0x212] = 2;
    bytes[0x20 + 0x1e7] = 1;
    bytes[0x1e0..0x1e4].copy_from_slice(&0x250u32.to_be_bytes());
    bytes[0x250 + 33] = 7;
    bytes[0x210 + 0x32..0x210 + 0x34].copy_from_slice(&31u16.to_be_bytes());
    bytes[0x250 + 26..0x250 + 28].copy_from_slice(&42u16.to_be_bytes());
    let result = source_enemy_data(&bytes).unwrap();
    assert_eq!(
        result.placement_row,
        Some(resonance_content::battle::EnemyPlacementRow::Middle)
    );
    assert_eq!(result.concealed_name, "？？？");
    assert_eq!(result.drop_chances, [25, 75]);
    assert_eq!(result.steal_chance, 60);
    assert_eq!(result.grade, -5);
    assert_eq!((result.idle_delay, result.idle_jitter), (90, 30));
    assert!(result.flying && result.finish_waits_for_animation);
    assert_eq!(result.movement.gravity, 0.);
    assert_eq!(
        result.reaction.weight,
        resonance_content::battle::ReactionWeight::Heavy
    );
    assert_eq!(
        (
            result.defense.poise,
            result.defense.stun_resistance,
            result.defense.stagger_threshold,
            result.defense.stagger_ticks
        ),
        (3, 12, 34, 56)
    );
    assert_eq!(
        result
            .variants
            .iter()
            .map(|v| (v.level, v.intelligence))
            .collect::<Vec<_>>(),
        [(3, 31), (7, 42)]
    );
    let output = crate::temporary_path(&std::env::temp_dir().join("enemy-statistics"));
    let directory = output.join("assets/enemy/battle/all/enemy-7");
    fs::create_dir_all(&directory).unwrap();
    let (settings, stats, resources, variants) = source_enemy_records(&bytes).unwrap();
    for (name, record) in [
        ("header-4", serde_json::to_value(settings).unwrap()),
        ("header-6", serde_json::to_value(stats).unwrap()),
        ("header-14", serde_json::to_value(resources).unwrap()),
        ("variants", serde_json::to_value(variants).unwrap()),
    ] {
        fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        output.join("sources.json"),
        br#"{"disc1/BTL/enemy.dat":["assets/enemy"]}"#,
    )
    .unwrap();
    let source = crate::cooked::Source::open(&output, 1, "BTL/enemy.dat").unwrap();
    assert_eq!(
        serde_json::to_value(published_enemy_data(&source, 7).unwrap()).unwrap(),
        serde_json::to_value(&result).unwrap()
    );
    fs::remove_file(directory.join("header-14.json")).unwrap();
    assert!(published_enemy_data(&source, 7).is_err());
    fs::remove_dir_all(output).unwrap();
    bytes[0x250 + 26..0x250 + 28].copy_from_slice(&(-1_i16).to_be_bytes());
    assert_eq!(
        source_enemy_records(&bytes).unwrap().3.variants[0]
            .combat
            .intelligence,
        -1
    );
    assert!(source_enemy_data(&bytes).is_err());
    bytes[0x250 + 26..0x250 + 28].copy_from_slice(&42_i16.to_be_bytes());
    for (status, expected) in [(0, false), (2, false), (3, true), (255, true)] {
        bytes[0x20 + 0xe7] = status;
        assert_eq!(source_enemy_data(&bytes).unwrap().opening_warning, expected);
    }
    // Damage protection reads the body halfword, not the following storage.
    for (flags, adjacent, expected) in [
        (0x1000_u16, 0_u16, true),
        (0x2000, 0, true),
        (0, 0x3000, false),
    ] {
        bytes[0x20 + 0xb4..0x20 + 0xb6].copy_from_slice(&flags.to_be_bytes());
        bytes[0x20 + 0xb6..0x20 + 0xb8].copy_from_slice(&adjacent.to_be_bytes());
        assert_eq!(
            source_enemy_data(&bytes).unwrap().defense.quarter_damage,
            expected
        );
    }
    bytes[0x1e0..0x1e4].copy_from_slice(&0x2ffu32.to_be_bytes());
    assert!(source_enemy_data(&bytes).is_err());
}

#[test]
fn undine_defeat_retains_the_body_and_keeps_unimplemented_collapse_paths_strict() {
    let mut metadata = all::ActorSettings::read(&[0; 0x1f0]).unwrap();
    // Original monster195 has flags40000, no clip override, and alpha255.
    metadata.combat.flags = 0x40000;
    metadata.model.alpha = 255;
    assert_eq!(death(&metadata, true).unwrap(), DeathStyle::Corpse);
    assert_eq!(
        death(&metadata, false).unwrap(),
        DeathStyle::Collapse { clip: 7 }
    );
    metadata.combat.flags = 0;
    assert_eq!(death(&metadata, true).unwrap(), DeathStyle::Fade);
    assert!(death(&metadata, false).is_err());
    metadata.model.death_clip = 9;
    assert_eq!(
        death(&metadata, false).unwrap(),
        DeathStyle::Collapse { clip: 9 }
    );
    assert!(death(&metadata, true).is_err());
    metadata.model.death_clip = 0;
    metadata.combat.flags = 0x40800;
    assert!(death(&metadata, true).is_err());
    assert!(death(&metadata, false).is_err());
}

#[test]
#[ignore = "requires locally extracted GameCube assets"]
fn original_neutral_affinity_uses_the_shared_element_selector() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let rel = actions::Rel::read(&root.join("files/US_r_Top2Btl.rel")).unwrap();
    for (start, bytes, expected) in [
        (
            0x20704,
            0x94,
            "d81c856d945d5101212beb9d0e3babd439769803fcabb2a394656c6c6cf2d72f",
        ),
        (
            0x61578,
            0x1ef8,
            "51c9c35ab83848b9259277ea6d8072703e77f307301d308a8f5fe9c4911cf185",
        ),
    ] {
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&rel.at((1, start)).unwrap()[..bytes])
            ),
            expected
        );
    }
    let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(root.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    for (monster, expected) in [
        (36u8, [0, 0, 0, 1, 0, 0, 0, 1, 2]),
        (63u8, [2, 0, 0, 1, 0, 0, 0, 0, 0]),
        (73u8, [4, 2, 2, 2, 2, 2, 2, 2, 2]),
        (240u8, [2, 2, 2, 2, 2, 2, 2, 2, 2]),
    ] {
        let offset = table + usize::from(monster) * 4;
        let start = word(&usual, offset).unwrap() as usize;
        let end = word(&usual, offset + 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let data = source_enemy_data(&bytes).unwrap();
        let mut actual = [data.physical_affinity as u8; 9];
        for (to, from) in actual[1..].iter_mut().zip(data.affinities) {
            *to = from as u8;
        }
        assert_eq!(actual, expected, "monster{monster}");
    }
}

#[test]
#[ignore = "requires original enemy packages; validates authored slots without cooking"]
fn original_guardians_retain_null_stun_loops_without_changing_immunity_or_other_motions() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files");
    let usual = fs::read(root.join("BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(root.join("BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    for (id, digest) in [
        (
            208usize,
            "909c64237b34d09a64dacb9b483d31464b8c8b040d77b14891d6ddc70969faf7",
        ),
        (
            209,
            "80228ebf9fa8dfffa4d31e8043714073c91886643c9e7492d6cb9b6ceb05d0a2",
        ),
        (
            210,
            "bffcb1c41ba9533c727219e901b13eeb841ef836880dffda98c23f2fcf0f710d",
        ),
    ] {
        let start = word(&usual, table + id * 4).unwrap() as usize;
        let end = word(&usual, table + (id + 1) * 4).unwrap() as usize;
        let mut bytes = compression::decode(&archive[start..end]).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), digest);
        let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
        assert_eq!(
            word(metadata, 0x5c).unwrap(),
            0,
            "guardian {id} is not fixed-motion"
        );
        assert_eq!(metadata[0xce], 0);
        assert_eq!((metadata[0x97], metadata[0xc1]), (0, 0));
        let clips = (0..word(&bytes, 0x14).unwrap().max(31) as usize)
            .filter(|&clip| word(&bytes, 0x20 + clip * 4).unwrap() != 0)
            .collect::<Vec<_>>();
        assert_eq!(
            clips,
            [0, 1, 2, 3, 7, 9, 11, 12, 13, 30, 31, 32, 33, 34, 35, 36, 37]
        );
        let resource = &bytes[word(&bytes, 0x18).unwrap() as usize..];
        let skeleton = pose::rig_skeleton(resource).unwrap();
        assert_eq!(skeleton.bones.len(), 102);
        assert_eq!(skeleton.bones[95].name, "mo4_Bone09_Mitos");
        for &clip in &clips {
            let anm = &bytes[word(&bytes, 0x20 + clip * 4).unwrap() as usize..];
            let motion = pose::motion(anm, resource).unwrap();
            motion.validate(&skeleton).unwrap();
            assert!(motion.duration_frames >= 1. && motion.duration_frames <= f32::from(i16::MAX));
        }
        let data = source_enemy_data(&bytes).unwrap();
        assert_eq!(data.stun.motion, StunMotion::KeepCurrent);
        assert!(!data.stun.immune);
        assert_eq!(data.stun.head_bone, 95);
        assert_eq!(data.stun.offset, [0., 75., 0.]);
        assert_eq!((data.stun.face, data.stun.idle_face), ([0; 4], [0; 4]));
        // Classification follows the authored slot, never the monster ID or flags.
        let motion = word(&bytes, 0x20 + 9 * 4).unwrap();
        bytes[0x20 + 21 * 4..0x24 + 21 * 4].copy_from_slice(&motion.to_be_bytes());
        assert_eq!(
            source_enemy_data(&bytes).unwrap().stun.motion,
            StunMotion::Loop
        );
    }
}

#[test]
#[ignore = "requires locally extracted GameCube assets"]
fn original_party_approach_keeps_dispatch_order_and_authored_steering() {
    use sha2::{Digest, Sha256};
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/extracted/disc1/files/US_r_Top2Btl.rel");
    let rel = actions::Rel::read(&path).unwrap();
    for (offset, target) in [
        (0xbe0, 0x31290),
        (0xbe4, 0x30c4c),
        (0xbe8, 0x30b4c),
        (0xbfc, 0x2f788),
    ] {
        assert_eq!(rel.pointer(5, offset).unwrap(), (1, target));
    }
    for (start, size, hash) in [
        (
            0x31290,
            0x8d8,
            "e61f0af92fb2dd23e45f9c66c387e32e72e7d125ac38b7308df1a118efa2c496",
        ),
        (
            0x30c4c,
            0x644,
            "cb45f971ec2630dc36847b70b8a82d41bfca75e500e83ecff1fe0f07fe3cd6b1",
        ),
        (
            0x2f788,
            0x39c,
            "f9bb0b35d2c9911baff414fb2530957587bd91f8affa57d098b44c4278dbd567",
        ),
        (
            0x24314,
            0x1bc,
            "987f4f2f25e5cb81ba9f37a26d10cf26f78f0ebfc4b63fc70540624b921bed37",
        ),
        (
            0x241f0,
            0x124,
            "531e5984253240e71cbc1ebb42e63a18da3578cca526d9917c076b3e744b5ac0",
        ),
        (
            0x24d24,
            0x2d0,
            "aa4f9282dc48983c76356dfcb54ef22c410d847d96e6c59e8c98a346de2523b2",
        ),
        (
            0x1f9b4,
            0x16c,
            "8abe840de06a1ef377c9e2bbcd02291040a62df2a6a5c11d3d9d50df6fcecdb2",
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(&rel.at((1, start)).unwrap()[..size])),
            hash
        );
    }
    for (offset, instruction) in [
        (0x31808, 0x4bff80b9), // Idle dispatch prepares the requested approach.
        (0x31ab8, 0x4bff326d), // Its tail turns, then integrates, without acceleration.
        (0x31ac4, 0x4bff2851),
        (0x30f40, 0x4bff33d5), // Running integrates old speed before 241F0 accelerates.
        (0x30f58, 0x4bff3299),
        (0x24490, 0xc03e1910), // Shared integration reads old speed, then acceleration.
        (0x24494, 0xc01e1918),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
    for (index, divisor) in [8, 10, 12, 12, 12, 12, 12, 12, 12].into_iter().enumerate() {
        let metadata = rel.at((5, 0x3d30 + index * 0x1f0)).unwrap();
        assert_eq!(metadata[0x2c], divisor);
        assert_eq!(word(metadata, 0x5c).unwrap(), 0x48000);
        assert_eq!(half(metadata, 0xb4).unwrap(), 0);
    }
}
