use super::*;

#[test]
fn enemy_names_keep_encoded_byte_lengths_and_reject_truncated_text() -> Result<()> {
    let package = |text: &[u8]| {
        let mut bytes = vec![0; 36];
        bytes[6..8].copy_from_slice(&8u16.to_be_bytes());
        bytes[12..12 + text.len()].copy_from_slice(text);
        bytes
    };
    assert_eq!(enemy_name(&package(b"enemy"))?, ("enemy".into(), 2));
    // Two source bytes per Japanese glyph, independent of UTF-8 length.
    assert_eq!(
        enemy_name(&package(&[0x82, 0xa0, 0x82, 0xa2]))?,
        ("あい".into(), 2)
    );
    assert!(enemy_name(&package(&[0x81])).is_err());
    assert!(enemy_name(&package(&[b'A'; 24])).is_err());
    assert!(enemy_name(&package(b"enemy")[..35]).is_err());
    assert!(enemy_name(&package(b"")).is_err());
    Ok(())
}

#[test]
#[ignore = "requires both extracted original discs; metadata only, no model publication"]
fn original_enemy_names_and_strategies_round_trip_source_headers_on_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut previous = None;
    for disc in [1, 2] {
        let extracted = local.join(format!("disc{disc}"));
        let sources = crate::source_assets::Sources::read(&extracted)?;
        let usual = std::fs::read(extracted.join("files").join(sources.usual))?;
        let archive = extracted.join("files").join(sources.enemy);
        let mut names = Vec::new();
        for id in 0..resonance_content::monster::MONSTER_COUNT {
            let bytes = crate::source_assets::enemy_package(&archive, &usual, id as u16)?;
            let source = crate::read::c_string(&bytes, usize::from(half(&bytes, 6)?) + 4)?;
            let (name, units) = enemy_name(&bytes)?;
            let strategy = enemy_strategy(&bytes)?;
            let start = usize::from(half(&bytes, 6)?);
            assert_eq!(strategy, bytes[start..start + 3], "enemy {id}");
            match id {
                36 => assert_eq!(strategy, [1, 1, 1]),
                49 => assert_eq!(strategy, [4, 1, 0]),
                _ => {}
            }
            let (encoded, _, invalid) = encoding_rs::SHIFT_JIS.encode(&name);
            ensure!(!invalid, "unencodable original enemy {id}");
            assert_eq!(encoded.as_ref(), source, "enemy {id}");
            assert_eq!(usize::from(units), source.len() >> 1, "enemy {id}");
            names.push((name, units, strategy));
        }
        if let Some(previous) = &previous {
            assert_eq!(previous, &names);
        }
        previous = Some(names);
    }
    Ok(())
}

fn model(names: &[&str]) -> Vec<u8> {
    let mut bytes = vec![0; 64 + names.len() * 28];
    let put = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes())
    };
    put(&mut bytes, 4, 32);
    put(&mut bytes, 32, 0x007b7960);
    bytes[38..40].copy_from_slice(&(names.len() as u16).to_be_bytes());
    put(&mut bytes, 44, 32);
    put(&mut bytes, 56, 1);
    put(&mut bytes, 60, (32 + names.len() * 28) as u32);
    for (index, name) in names.iter().enumerate() {
        if index + 1 < names.len() {
            put(
                &mut bytes,
                64 + index * 28 + 8,
                (32 + (index + 1) * 28) as u32,
            );
        }
        bytes[64 + index * 28 + 24] = 1;
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    let size = bytes.len() - 32;
    put(&mut bytes, 8, size as u32);
    bytes
}

#[test]
fn rig_classification_keeps_byte_arithmetic_order_and_body_only_volumes() -> Result<()> {
    let source = model(&[
        "root",
        "mo07_body",
        "mon_hair",
        "DM3",
        "dm32",
        "at00_a",
        "at00_b",
        "at02",
        "kk01",
    ]);
    let r = rig(&source, RigKind::Body)?;
    assert_eq!(
        r.volumes
            .iter()
            .map(|v| (v.bone, v.radius, v.hurt, v.body))
            .collect::<Vec<_>>(),
        [
            (1, 70., true, true),
            (3, 30., true, false),
            (4, 0., false, true)
        ]
    );
    assert_eq!(r.attack_groups[&0], [5, 6]);
    assert_eq!(r.attack_groups[&2], [7]);
    assert!(r.attack_groups[&1].is_empty());
    assert_eq!(r.attachments[&1], 8);
    assert_eq!(r.target_bones, [0, 1]);
    let weapon = rig(&source, RigKind::Weapon)?;
    assert_eq!(weapon.attack_groups[&0], [5, 6, 7]);
    assert_eq!(weapon.attack_groups.len(), 1);
    assert!(weapon.volumes.is_empty() && weapon.attachments.is_empty());
    assert!(rig(&model(&["mo"]), RigKind::Body).is_err());
    assert!(rig(&model(&[r"mo\xb5"]), RigKind::Body).is_err());
    assert!(rig(&model(&["at0/"]), RigKind::Body).is_err());
    assert!(rig(&source[..source.len() - 1], RigKind::Body).is_err());
    Ok(())
}

#[test]
fn target_bounds_use_source_classifier_and_case_sensitive_exclusions() -> Result<()> {
    let source = model(&[
        "root",
        "mo16",
        "mo15",
        "DM3",
        "pa00",
        "AB00",
        "ef00",
        "ki00",
        "ns00",
        "Bone_ude01",
        "Bone_te01",
        "Bone_yu01",
        "Bone_ring01",
        "obj_nuno01",
        "manto",
        "bone_ude01",
    ]);
    let r = rig(&source, RigKind::Body)?;
    assert_eq!(r.target_bones, [0, 2, 15]);
    Ok(())
}

#[test]
#[ignore = "requires both extracted original discs and the current monster publications"]
fn every_enemy_rig_matches_shared_model_and_sparse_clip_bindings() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let mut first = None;
    let mut kinds = std::collections::BTreeSet::new();
    for disc in [1, 2] {
        let output = tempfile::tempdir()?;
        let paths = publish_enemies(
            &local.join(format!("extracted/disc{disc}")),
            &local.join("all-assets"),
            output.path(),
        )?;
        assert_eq!(paths.len(), resonance_content::monster::MONSTER_COUNT);
        let mut records = Vec::new();
        for (id, path) in paths.iter().enumerate() {
            let bytes = std::fs::read(output.path().join(path))?;
            let enemy: Enemy = serde_json::from_slice(&bytes)?;
            let monster: resonance_content::monster::Monster = serde_json::from_slice(
                &std::fs::read(local.join(format!("all-assets/monsters/{id:03}.json")))?,
            )?;
            let part = &monster.preview.parts[0].scene;
            assert!(
                enemy
                    .body
                    .skeleton
                    .bones
                    .iter()
                    .map(|b| &b.name)
                    .eq(part.bone_names.iter()),
                "enemy {id}"
            );
            assert_eq!(enemy.body.transform_kinds.len(), part.bone_names.len());
            kinds.extend(enemy.body.transform_kinds.iter().copied());
            for clip in &part.clips {
                resonance_content::animation::Motion::decode(&std::fs::read(
                    local.join("all-assets").join(&clip.motion),
                )?)?
                .validate(&enemy.body.skeleton)?;
            }
            assert!(
                enemy
                    .body
                    .volumes
                    .iter()
                    .all(|v| usize::from(v.bone) < part.bone_names.len())
            );
            if id == 49 {
                assert_eq!(
                    enemy
                        .body
                        .volumes
                        .iter()
                        .map(|v| (v.bone, v.radius))
                        .collect::<Vec<_>>(),
                    [(3, 80.)]
                );
                assert_eq!(enemy.profile.guard_pressure_limit, 10);
                assert_eq!(enemy.profile.head_bone, 5);
            }
            // F00C/F0D4 load this halfword through signed-16 GQR5. The
            // partial C reconstruction incorrectly describes a float load.
            if let Some(radius) = match id {
                44 => Some(3000),
                172 | 182 => Some(2500),
                198 => Some(2200),
                208..=210 | 240 => Some(2900),
                246 => Some(2750),
                _ => None,
            } {
                assert_eq!(enemy.profile.camera_minimum_radius, radius, "enemy {id}");
            }
            records.push(bytes);
        }
        if let Some(first) = &first {
            assert_eq!(&records, first);
        }
        first = Some(records);
    }
    eprintln!("Original enemy transform kinds: {kinds:?}");
    Ok(())
}
