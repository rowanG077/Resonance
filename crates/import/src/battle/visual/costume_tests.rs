use super::*;
use resonance_content::battle::visual::{PartyModel, PartyVariant, PartyVisuals};
use std::collections::BTreeMap;

#[test]
fn required_costumes_follow_parsed_overrides_and_story_owners() {
    let mut title = Title {
        name: String::new(),
        description: String::new(),
        growth: [0; 7],
        costume: None,
    };
    assert_eq!(required(9, &[title.clone()]), [Costume::Standard].into());
    title.costume = Some(Costume::Variant2);
    assert_eq!(
        required(9, &[title.clone(), title.clone()]),
        [Costume::Standard, Costume::Variant2].into()
    );
    for character in 1..=9 {
        let actual = required(character, &[title.clone()]);
        assert_eq!(actual.contains(&Costume::Story), matches!(character, 2 | 7));
        assert!(!actual.contains(&Costume::Variant1));
        assert!(!actual.contains(&Costume::Variant4));
    }
    for (character, costume) in [(0, 0), (10, 0), (1, 5)] {
        assert!(party::names(&[], character, costume).is_err());
    }
    let equipment = vec![0x120; 528];
    assert_eq!(weapon_owners(&equipment, 527), 0x120);
    assert_eq!(weapon_owners(&equipment, 528), 0);
    assert_eq!(weapon_owners(&equipment, u16::MAX), 0);
}

#[test]
#[ignore = "requires original US bodies, CABs, weapons and source; parses complete rigs without cooking"]
fn original_costumes_keep_all_declarations_full_tracks_and_linked_weapon_banks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let extracted = root.join("local/extracted/disc1");
    let files = extracted.join("files");
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    assert_eq!(
        digest(&executable),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    for (file, expected) in [
        (
            "auto_800EBA08_gap.c",
            "7be5f49abce7f3a591293ddf2bdeb79394458f6f8903a97fe5a9722461fc1918",
        ),
        (
            "fn_800A2778.c",
            "a01f768f1435274cd67dff3594826de90d8af9108968d00ffe67ef88227ad893",
        ),
        (
            "fn_80081618.c",
            "773a959579163d9045fc279428ec613cafc6684429088e0ded718e4290be844e",
        ),
        (
            "fn_800816A8.c",
            "ec56b1b984ff8bb8f3321c52e1195a38f92c621bf2ea9613cf8af91669486723",
        ),
        (
            "fn_8008172C.c",
            "010e504b3c612713ef46cc72ff940cdecf8f3ca7a8737cefafb44a910f23fed9",
        ),
        (
            "fn_8006D2E0.c",
            "7b4300a8308b917157113252c47728be21b559f1d311a6d75ea7ac4ae0816503",
        ),
        (
            "fn_8006EB68.c",
            "b9979d3fcea5d4d1536d5c93d48d2985554f07c686f0789b671acd723d6b5086",
        ),
        (
            "rel/US_r_Top2Btl/fn_1_1CCAC.c",
            "df29d1406fac1f76f670d5ab4c77d1ae32310b0b767263a53aa2b082a1e6b57b",
        ),
        (
            "rel/US_r_Top2Btl/fn_1_4B350.c",
            "f3cbf9239309a91d4730d9e2911df659f088d083bb5b639880dde2267f200dd3",
        ),
        (
            "rel/US_r_Top2Btl/fn_1_2C05C.c",
            "21ddd968ea4397606d3bfc58ae9ca829f6547093abf368f9b164786b40ad55a1",
        ),
        (
            "rel/US_r_Top2Btl/fn_1_40C8.c",
            "83470c493ad5557116976347fd3fe69d74aa596d8b0edfae238a29f9b28d7cfe",
        ),
        (
            "rel/US_r_Top2Btl/fn_1_5B738.c",
            "e8c9fb03df6524c95f6abf8b122bf2f8435b1a277d64f025d62fc40f2919ad5c",
        ),
    ] {
        assert_eq!(
            digest(
                &fs::read(
                    root.join("../Tales-of-Symphonia-decomp/src/game")
                        .join(file)
                )
                .unwrap()
            ),
            expected
        );
    }
    let titles = crate::menu::titles(&executable).unwrap();
    let equipment_owners =
        crate::session::equipment_owners(&executable, &crate::item::read(&executable).unwrap())
            .unwrap();
    assert_eq!(equipment_owners.len(), 528);
    let rows = crate::dol::slice(&executable, 0x801fad98, 528 * 60).unwrap();
    for (item, (row, mask)) in rows.chunks_exact(60).zip(&equipment_owners).enumerate() {
        for member in 0..9 {
            let expected = match member {
                8 => row[0x15] & 0x20 != 0,
                5 => row[0x15] & 0x20 != 0 && ![236, 284, 327, 363].contains(&item),
                member => row[0x15] & (1_u8 << member) != 0,
            };
            assert_eq!(
                mask & (1_u16 << member) != 0,
                expected,
                "item {item}, member {member}"
            );
        }
    }
    for item in [236, 284, 327, 363] {
        assert_eq!(equipment_owners[item] & 0x120, 0x100);
    }
    let rel = fs::read(files.join("US_r_Top2Btl.rel")).unwrap();
    let weapons = fs::read(files.join("BTL/BTLwepon.dat")).unwrap();
    assert_eq!(
        digest(&rel),
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    assert_eq!(
        digest(&weapons),
        "e59189587b49d1d0413d660ba55860921900a3aebb939a27dd6a7765ff67d0cd"
    );
    let fst_bytes = fs::read(extracted.join("sys/fst.bin")).unwrap();
    let fst = nod::disc::fst::Fst::new(&fst_bytes).unwrap();
    let entries: Vec<_> = fst
        .iter()
        .filter(|(_, entry, _)| entry.is_file())
        .map(|(_, _, name)| name.to_ascii_lowercase())
        .collect();
    let mut bodies = BTreeMap::new();
    let mut archives = BTreeMap::new();
    let mut missing = Vec::new();
    let mut required_count = 0;
    let actions = if let Some(path) = std::env::var_os("RESONANCE_COSTUME_PREFLIGHT_SELECTION") {
        let bytes = fs::read(root.join(path)).unwrap();
        let selection: crate::battle::CookSelection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(selection.party, (1..=9).collect::<Vec<_>>());
        eprintln!(
            "Costume motion preflight: all36 selections, {} selected artes, selection SHA256 {}",
            selection.artes.len(),
            digest(&bytes)
        );
        crate::battle::actions::cook_selected(
            &extracted,
            &root.join("local/all-assets/data"),
            &crate::arte::cooked(&root.join("local/all-assets/data")).unwrap(),
            &crate::embedded::read(
                &root.join("local/all-assets/data"),
                "battle-motion",
                "US_r_Top2Btl.rel",
            )
            .unwrap(),
            &selection,
            &[],
            &mut || {
                let rel = crate::battle::actions::Rel::read(&files.join("US_r_Top2Btl.rel"))?;
                let usual = fs::read(files.join("BTL/BTLusual.dat"))?;
                crate::battle::actions::source_contact(&rel, &usual)
            },
        )
        .unwrap()
    } else {
        eprintln!(
            "Costume motion preflight: all36 normal/result tables; selected-arte check NOT RUN (RESONANCE_COSTUME_PREFLIGHT_SELECTION unset)"
        );
        let source = crate::battle::actions::Rel::read(&files.join("US_r_Top2Btl.rel")).unwrap();
        resonance_content::battle::actions::BattleActions {
            party: (1..=9)
                .map(|character| crate::battle::actions::party_actions(&source, character).unwrap())
                .collect(),
            enemies: vec![],
            projectiles: vec![],
            techniques: vec![],
            chains: None,
        }
    };
    // These records validate CPU motion requests only; no renderer resources are cooked.
    let mut visuals = VisualAssets {
        arenas: BTreeMap::new(),
        party: BTreeMap::new(),
        enemies: BTreeMap::new(),
        weapons: BTreeMap::new(),
        pow_weapons: BTreeMap::new(),
        pow_devastation: BTreeMap::new(),
        toon_ramp: String::new(),
        shadow_texture: String::new(),
        effect_models: vec![],
    };
    for character in 1..=9 {
        let required = required(character, &titles[usize::from(character - 1)]);
        required_count += required.len();
        for costume in 0..5 {
            let (body_name, _) = party::names(&executable, character, costume).unwrap();
            let archive = party::archive(&executable, &files, character, costume).unwrap();
            archives.insert((character, costume), archive.source_sha256.clone());
            let matches = entries
                .iter()
                .filter(|name| name.eq_ignore_ascii_case(&body_name))
                .count();
            if matches == 0 {
                assert!(party::body(&executable, &files, character, costume).is_err());
                missing.push((character, costume, body_name));
                continue;
            }
            assert_eq!(matches, 1);
            let body = party::body(&executable, &files, character, costume).unwrap();
            bodies.insert((character, costume), digest(&body));
            if !required.iter().any(|&value| value as u8 == costume) {
                continue;
            }
            let victory = victory::Package::open(&files.join("BTL/BTLwin.bfp"), character).unwrap();
            let mut clips = archive
                .sections
                .iter()
                .enumerate()
                .skip(2)
                .filter_map(|(member, range)| {
                    range.as_ref().map(|range| SourceClip {
                        slot: (member - 2) as u16,
                        bytes: &archive.bytes[range.clone()],
                        resource: None,
                    })
                })
                .collect::<Vec<_>>();
            clips.extend(victory.clips());
            let ranges = sections(&body).unwrap();
            let primary = &body[ranges[0].clone().unwrap()];
            let rig = rig(primary, &clips, RigKind::Actor).unwrap_or_else(|error| {
                panic!("character {character} costume {costume}: {error:#}")
            });
            let table = party::motion_table(&archive).unwrap();
            table
                .validate(&rig, victory.bindings.values().map(|motion| motion.clip))
                .unwrap();
            for slot in 0..table.count {
                assert_eq!(
                    table.nulls.contains(&slot),
                    word(&archive.bytes, 4 + (usize::from(slot) + 2) * 4).unwrap() == 0
                );
            }
            if character == 2 {
                assert_eq!(table.count, 93);
                assert_eq!(table.nulls.contains(&25), costume == 0);
                assert_eq!(table.nulls.contains(&91), costume != 0);
            }
            if character == 4 {
                assert_eq!(table.count, 61);
                assert!(table.nulls.contains(&31));
            }
            let mesh = super::super::super::pose::skeleton(primary).unwrap();
            assert!(
                rig.skeleton
                    .bones
                    .iter()
                    .map(|bone| &bone.name)
                    .eq(mesh.bones.iter().map(|bone| &bone.name)),
                "character {character} costume {costume} rig/mesh bone indices"
            );
            assert_eq!(rig.motions.len(), clips.len());
            let metadata = &rel_data(&rel).unwrap()[0x3d30 + usize::from(character - 1) * 496..];
            initial_pose::metadata(metadata, &rig).unwrap();
            target_anchor(metadata, &rig).unwrap();
            texture_layers(metadata, Some(primary)).unwrap();
            for range in ranges.iter().skip(1).flatten() {
                let layer = &body[range.clone()];
                let skeleton = super::super::super::pose::skeleton(layer).unwrap();
                for clip in &clips {
                    super::super::super::pose::motion(clip.bytes, layer)
                        .unwrap()
                        .validate(&skeleton)
                        .unwrap();
                }
            }
            let mut weapon_motions = BTreeMap::new();
            if character == GENIS {
                let fallback = archive.sections[62].clone().unwrap();
                assert_eq!(
                    digest(&archive.bytes[fallback]),
                    "9ac4b4522636885dd54dc46ba2f80577265d14b9b4cf58757d95b716a715be1a",
                    "Genis costume {costume} linked weapon fallback"
                );
                assert_eq!(
                    head_bone(&rel, &rig).unwrap(),
                    if costume == 4 { 12 } else { 13 }
                );
                for item in 135..=366 {
                    let row = crate::dol::slice(&executable, 0x801fad98 + u32::from(item) * 60, 60)
                        .unwrap();
                    if !(13..=22).contains(&row[0x1a])
                        || weapon_owners(&equipment_owners, item) & 4 == 0
                    {
                        continue;
                    }
                    let linked = linked_weapon_motions(&rel, &weapons, item, &archive).unwrap();
                    assert_eq!(linked.link.source_sha256, archive.source_sha256);
                    assert_eq!((linked.link.offset, linked.link.fallback), (60, 60));
                    let slots: BTreeSet<_> = weapon_clips(&archive)
                        .iter()
                        .map(|clip| clip.slot)
                        .collect();
                    for rig in linked.rigs.values() {
                        assert_eq!(rig.motions.keys().copied().collect::<BTreeSet<_>>(), slots);
                        rig.validate().unwrap();
                    }
                    weapon_motions.insert(item, linked);
                }
            } else {
                head_bone(&rel, &rig).unwrap();
            }
            let selected = *required
                .iter()
                .find(|&&selected| selected as u8 == costume)
                .unwrap();
            let motion_model = PartyModel {
                head_bone: head_bone(&rel, &rig).unwrap(),
                weapon_motions,
                visual: ModelVisuals {
                    model_sha256: digest(&body),
                    animation_sha256: archive.source_sha256.clone(),
                    authored_motions: Some(table),
                    rig,
                    model: ModelPreview {
                        scale: 1.,
                        elevation: 0.,
                        parts: vec![],
                        hidden_geometry: vec![],
                        node_scales: vec![],
                    },
                    volumes: vec![],
                    alpha: 255,
                    shadow: None,
                    bounds_joints: vec![],
                    target_anchor: Default::default(),
                    initial_pose: Default::default(),
                    paired_body: None,
                    texture_layers: texture_layers(metadata, Some(primary)).unwrap(),
                    variant_texture: None,
                    victory: victory.bindings,
                    attachments: BTreeMap::new(),
                    trails: BTreeMap::new(),
                },
            };
            visuals
                .party
                .entry(character)
                .or_insert_with(|| PartyVisuals {
                    bindings: BTreeMap::new(),
                })
                .bindings
                .insert(selected, PartyVariant::Model(Box::new(motion_model)));
        }
    }
    assert_eq!(required_count, 36);
    actions.validate_party_models(&visuals.party).unwrap();
    validate_martial(&crate::arte::read(&executable).unwrap(), &actions, &visuals).unwrap();
    let rel = crate::rel::Rel::read(&files.join("US_r_Top2Btl.rel")).unwrap();
    let layout = &crate::battle::embedded::Layout::RETAIL;
    crate::battle::unison::cook(
        &crate::battle::unison::Inputs::read_source(&extracted).unwrap(),
        &crate::battle::unison_tables::read(&rel, layout).unwrap(),
        &crate::battle::unison_opener::read(&rel, layout).unwrap(),
        &crate::arte::read(&executable).unwrap(),
    )
    .unwrap()
    .validate_party_motions(&visuals.party)
    .unwrap();
    assert_eq!((bodies.len(), archives.len()), (44, 45));
    assert_eq!(missing, [(9, 4, "kratos003.bin".into())]);
    assert_eq!(bodies[&(2, 0)], bodies[&(2, 3)]);
    assert_ne!(archives[&(2, 0)], archives[&(2, 3)]);
    assert_ne!(bodies[&(7, 0)], bodies[&(7, 3)]);
    assert_ne!(archives[&(7, 0)], archives[&(7, 3)]);
    assert_eq!(archives[&(3, 0)], archives[&(3, 3)]);
    for costume in [1, 2, 4] {
        assert_ne!(archives[&(3, 0)], archives[&(3, costume)]);
    }
}
