//! Reviewed meanings point to original operands, not guessed numeric gaps.
#[path = "support/original_words.rs"]
mod original_words;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use symphonia_script::{Op, Program};
use symphonia_script_compiler::scenario::{self, Context, Definitions};

fn evidence() -> impl Iterator<Item = Vec<&'static str>> {
    include_str!("../data/scenario-identity-evidence.tsv")
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
}

fn reviewed_symbol(expression: &str) -> &str {
    if let Some(arguments) = expression.strip_prefix("voice_archive(") {
        return arguments
            .split_once(',')
            .expect("malformed voice evidence")
            .0
            .trim();
    }
    expression
        .strip_prefix("bound_before(")
        .map(|s| s.strip_suffix(')').expect("malformed bound evidence"))
        .unwrap_or(expression)
}

#[test]
#[ignore = "requires the original main.dol at local/extracted"]
fn original_builtin_sphere_resolves_before_field_banks_and_keeps_constructor_flags() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    for (address, length, hash) in [
        (
            0x8005d138,
            416,
            "af096ca16b4c58c24ebc34e3c0199a13506d0b7b32f691b679883a31319cd4e6",
        ),
        (
            0x801e73e0,
            544,
            "c3ecd0c995289d3a9b4738deba4c494a5515e0ddff2dadb3f152ea6d32bbe6fe",
        ),
        (
            0x80059cfc,
            1456,
            "12859106675bd257995a75fb21c7a8991988715f79c5244183a0cc88654c0241",
        ),
        (
            0x80059838,
            468,
            "6370abdc71949345369067ba3c0ffb603408d1a1022441f9ce274e14a336e924",
        ),
        (
            0x80059428,
            472,
            "f33af950c6ef274dc59efe6a8f0d77d00a070e70b61f0566c3fcccb491917e3e",
        ),
        (
            0x80054864,
            1012,
            "2006de001a875883db07e6a4865704c04f67ff3e17d10d0cc61f3586ada91558",
        ),
        (
            0x80010aa4,
            96,
            "cc9dde3cfbf9ccb24276fd7c5cb5f9c9ec57ba6f9a38c908e64641a7edf9ea31",
        ),
    ] {
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + length]), hash, "{address:#x}");
    }
    // Exact key24 returns a constant pointer before the field-bank load.
    for (address, expected) in original_words::words::<u32>(
        "original_builtin_sphere_resolves_before_field_banks_and_keeps_constructor_flags",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    let start = offset(0x801e73e0);
    assert_eq!(word(start), 31);
    assert_eq!(word(start + 4), 0x80);
    assert_eq!(word(start + 8), 0);
    for entry in (12..0x80).step_by(4) {
        assert_eq!(word(start + entry), 0x220);
    }
    assert_eq!(word(start + 0xa0), 0x00b749e0); // GPL model magic
    assert_eq!(word(start + 0x84), 0x100); // pose offset in GPL section
    assert_eq!(word(start + 0x88), 0xa0); // pose size
    assert_eq!(word(start + 0x180), 0x007b7960); // pose magic
    assert_eq!(&dol[start + 0x160..start + 0x169], b"Sphere01\0");
    assert_eq!(&dol[start + 0x1f0..start + 0x1f7], b"nc.gpl\0");
    assert_eq!(&dol[start + 0x200..start + 0x209], b"Sphere01\0");
    let sphere = symphonia_constants::BuiltinFieldModelId::Sphere;
    assert_eq!(sphere as i32, 24);
    assert_eq!(sphere.resource_name(), None);
    use symphonia_constants::ActorProperty;
    assert_eq!(ActorProperty::DisableCollision as i32, 9);
    assert_eq!(ActorProperty::DisableGrounding as i32, 10);
    assert_eq!(ActorProperty::DisableShadow as i32, 11);
    assert_eq!(ActorProperty::HideModel as i32, 12);
    // Constructor flags are initial state, not a claim that this real model
    // must remain invisible after subsequent property edits.
}

fn is_reviewed_memory_load(bytes: &[u8], pc: u32, address: i32, mnemonic: &str) -> bool {
    if !(0..=65535).contains(&address) || !matches!(mnemonic, "load.s8" | "load.s16" | "load.s32") {
        return false;
    }
    let Ok((instruction, diagnostics)) = symphonia_script::scenario::decode_at(bytes, pc) else {
        return false;
    };
    diagnostics.is_empty()
        && !instruction.raw
        && instruction.mnemonic == mnemonic
        && instruction.operands == [i64::from(address)]
}

fn is_reviewed_global_load(bytes: &[u8], pc: u32, global: i32) -> bool {
    (0..256).contains(&global) && is_reviewed_memory_load(bytes, pc, 0x40 + 4 * global, "load.s32")
}

#[test]
fn narrow_load_evidence_checks_address_width_and_retained_code_bounds() {
    let assemble = |auxiliary| {
        symphonia_script::scenario::assemble(&format!(
        ".code_base 4\n.word 4\n.word 0\n.word {auxiliary}\n.word 0\nend\nload.s8 336\nload.s16 308\nend\n"
    )).unwrap()
    };
    let bytes = assemble(0);
    assert!(Program::decode(&bytes).unwrap().instruction(1).is_none());
    assert!(is_reviewed_memory_load(&bytes, 1, 336, "load.s8"));
    assert!(is_reviewed_memory_load(&bytes, 3, 308, "load.s16"));
    for (pc, address, mnemonic) in [
        (1, 336, "load.s16"),
        (3, 308, "load.s8"),
        (1, 337, "load.s8"),
        (1, -1, "load.s8"),
        (1, 65536, "load.s8"),
        (1, 336, "push.s8"),
    ] {
        assert!(!is_reviewed_memory_load(&bytes, pc, address, mnemonic));
    }
    assert!(!is_reviewed_memory_load(&assemble(5), 1, 336, "load.s8"));
    assert!(!is_reviewed_memory_load(&bytes[..11], 1, 336, "load.s8"));
}

#[test]
fn global_load_evidence_checks_retained_code_without_accepting_auxiliary_or_wrong_widths() {
    let assemble = |auxiliary, load: &str| {
        symphonia_script::scenario::assemble(&format!(
            ".code_base 4\n.word 4\n.word 0\n.word {auxiliary}\n.word 0\nend\n{load}\nend\n"
        ))
        .unwrap()
    };
    let bytes = assemble(0, "load.s32 316");
    // PC1 lies after the entry's End and has no registry entry or incoming edge.
    assert!(Program::decode(&bytes).unwrap().instruction(1).is_none());
    assert!(is_reviewed_global_load(&bytes, 1, 63));
    assert!(!is_reviewed_global_load(&bytes, 1, 62));
    assert!(!is_reviewed_global_load(
        &assemble(0, "load.s16 316"),
        1,
        63
    ));
    // Auxiliary starts immediately after End; its load-looking data is not code.
    assert!(!is_reviewed_global_load(
        &assemble(5, "load.s32 316"),
        1,
        63
    ));
    assert!(!is_reviewed_global_load(&bytes[..11], 1, 63));
    assert!(!is_reviewed_global_load(&bytes, 1, -1));
    assert!(!is_reviewed_global_load(&bytes, 1, 256));
}

#[test]
fn reviewed_identity_symbols_keep_their_original_values_and_distinct_namespaces() {
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let notes: BTreeMap<_, _> = include_str!("../data/scenario-identity-notes.tsv")
        .lines()
        .skip(1)
        .map(|line| line.split_once('\t').expect("evidence note columns"))
        .collect();
    let mut checked = BTreeSet::new();
    for row in evidence() {
        assert_eq!(row.len(), 7);
        assert!(!notes.get(row[6]).expect("missing evidence note").is_empty());
        if !checked.insert(row[0]) {
            continue;
        }
        let module = reviewed_symbol(row[0]).split("::").next().unwrap();
        let text = format!(
            "script scenario; use std::{module}; code_base = 4;
            data {{ word(4); pc(entry); word(0); word(0); }}
            task entry() {{ eval {}; end; }}",
            row[0]
        );
        let bytes = scenario::compile("evidence", &text, &definitions).unwrap();
        let (Op::Push(value), _) = Program::decode(&bytes).unwrap().instruction(0).unwrap() else {
            panic!("missing literal");
        };
        assert_eq!(value, row[1].parse::<i32>().unwrap(), "{}", row[0]);
    }
    assert!(checked.len() >= 390);
}

#[test]
#[ignore = "requires the original GQSEAF executable and battle REL at local/extracted"]
fn original_save_points_and_enemy_action_gates_use_the_story_flag_bank() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let rel = fs::read(root.join("files/US_r_Top2Btl.rel")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        digest(&rel),
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    let word = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(&dol, 0x48 + section * 4);
                let size = word(&dol, 0x90 + section * 4);
                (address >= start && address - start < size).then(|| {
                    word(
                        &dol,
                        word(&dol, section * 4) as usize + (address - start) as usize,
                    )
                })
            })
            .unwrap()
    };
    // The constructor's fourth argument is queried and retained on the actor.
    // The opened/sealed callback selection does not change its flag namespace.
    for (address, expected) in original_words::words::<u32>(
        "original_save_points_and_enemy_action_gates_use_the_story_flag_bank",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    assert_eq!(0x1098 - 0xead, symphonia_constants::ItemId::MemoryGem.raw());

    // A zero u16 at action-row+3a bypasses the query. A nonzero flag must be
    // set, independently of the eligibility mask at row+8.
    assert_eq!(word(&rel, 0), 1);
    let text = (word(&rel, word(&rel, 0x10) as usize + 8) & !3) as usize;
    for (offset, expected) in original_words::words::<usize>(
        "original_save_points_and_enemy_action_gates_use_the_story_flag_bank-2",
    ) {
        assert_eq!(word(&rel, text + offset), expected, "REL text+{offset:#x}");
    }
    // Resolve the call through its import relocation, not the unrelocated BL.
    let imports = word(&rel, 0x28) as usize;
    let mut query = false;
    for import in (imports..imports + word(&rel, 0x2c) as usize).step_by(8) {
        let module = word(&rel, import);
        let mut cursor = word(&rel, import + 4) as usize;
        let (mut section, mut offset) = (0, 0usize);
        loop {
            let delta = u16::from_be_bytes(rel[cursor..cursor + 2].try_into().unwrap());
            let kind = rel[cursor + 2];
            let target = rel[cursor + 3];
            let addend = word(&rel, cursor + 4);
            cursor += 8;
            match kind {
                203 => break,
                202 => {
                    section = target;
                    offset = 0;
                }
                _ => {
                    offset += usize::from(delta);
                    if (module, section, offset, kind, target, addend)
                        == (0, 1, 0x34f20, 10, 2, 0x80080fd4)
                    {
                        query = true;
                    }
                }
            }
        }
    }
    assert!(query, "enemy action gate must call the story flag reader");
}

#[test]
#[ignore = "requires the original GQSEAF executable and field REL at local/extracted"]
fn original_travel_permission_maps_sylvarant_region10_to_event_flag910() {
    use sha2::{Digest, Sha256};
    fn word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let rel = fs::read(root.join("files/US_r_Top2field.rel")).unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        Sha256::digest(&rel)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "680027057d07cdab9de63e40cb7bacd945572b4f2df11b55ca67dc5e64ac28f8"
    );
    assert_eq!(word(&rel, 0), 2);
    let sections = word(&rel, 0x10) as usize;
    let text = (word(&rel, sections + 2 * 8) & !3) as usize;
    let rodata = (word(&rel, sections + 5 * 8) & !3) as usize;
    let data = (word(&rel, sections + 6 * 8) & !3) as usize;
    let grid = &rel[rodata + 0x3e8..rodata + 0x3e8 + 2 * 18 * 24];
    let sylvarant = &grid[..18 * 24];
    assert!(sylvarant.iter().all(|&region| region <= 10));
    assert_eq!(sylvarant.iter().filter(|&&region| region == 10).count(), 38);

    // Native lookup: row*24 + world*432 + column, followed by the region
    // byte plus world*11, passed in r3 to the persistent-flag query.
    for (offset, expected) in original_words::words::<usize>(
        "original_travel_permission_maps_sylvarant_region10_to_event_flag910",
    ) {
        assert_eq!(word(&rel, text + offset), expected, "REL text+{offset:#x}");
    }

    // Resolve both the original table reference and imported DOL query from
    // the relocation stream; equal-looking instruction immediates are not enough.
    let imports = word(&rel, 0x28) as usize;
    let imports_end = imports + word(&rel, 0x2c) as usize;
    let mut region_references = BTreeSet::new();
    let mut flag_query = false;
    let mut unlock_references = BTreeSet::new();
    let mut unlock_calls = BTreeSet::new();
    let mut unlock_labels = BTreeMap::new();
    for import in (imports..imports_end).step_by(8) {
        let module = word(&rel, import);
        let mut relocation = word(&rel, import + 4) as usize;
        let mut section = 0;
        let mut at = 0usize;
        loop {
            let delta = u16::from_be_bytes(rel[relocation..relocation + 2].try_into().unwrap());
            let kind = rel[relocation + 2];
            let target_section = rel[relocation + 3];
            let addend = word(&rel, relocation + 4);
            relocation += 8;
            if kind == 203 {
                break;
            }
            if kind == 202 {
                section = target_section;
                at = 0;
                continue;
            }
            at += usize::from(delta);
            if section == 2 {
                if module == 2 && target_section == 5 && addend == 0x3e8 {
                    region_references.insert((at, kind));
                }
                if module == 0 && kind == 10 && at == 0x1ad74 && addend == 0x800810d4 {
                    flag_query = true;
                }
                if module == 2 && target_section == 6 && addend == 0x8c {
                    unlock_references.insert((at, kind));
                }
                if module == 0 && kind == 10 && matches!(at, 0x1ae0 | 0x19030 | 0x19060 | 0x191a4) {
                    unlock_calls.insert((at, addend));
                }
            }
            if section == 6
                && module == 2
                && kind == 1
                && target_section == 5
                && (0x8c..0x11c).contains(&at)
            {
                assert!(unlock_labels.insert(at, addend).is_none());
            }
        }
    }
    assert!(region_references.contains(&(0x1abe2, 6)));
    assert!(region_references.contains(&(0x1abea, 4)));
    assert!(flag_query);

    // The field unlock producer is table driven. Its twelve keyed rows each
    // contain an original region caption and up to three flags. It queries
    // the first flag, skips an already-unlocked group, and sets each nonzero
    // halfword in that row. These are flags901..922, not scene scratch bits.
    assert!(unlock_references.is_superset(&BTreeSet::from([
        (0x1ac6, 6),
        (0x1ace, 4),
        (0x19016, 6),
        (0x1901e, 4),
        (0x19042, 6),
        (0x1904a, 4),
        (0x19176, 6),
        (0x19196, 4),
    ])));
    assert_eq!(
        unlock_calls,
        BTreeSet::from([
            (0x1ae0, 0x80080fd4),
            (0x19030, 0x80080fd4),
            (0x19060, 0x80080f98),
            (0x191a4, 0x80080fd4),
        ])
    );
    for (offset, expected) in original_words::words::<usize>(
        "original_travel_permission_maps_sylvarant_region10_to_event_flag910-2",
    ) {
        assert_eq!(word(&rel, text + offset), expected, "REL text+{offset:#x}");
    }
    let halfword = |offset: usize| u16::from_be_bytes(rel[offset..offset + 2].try_into().unwrap());
    let mut unlocked_flags = BTreeSet::new();
    for (index, (key, flags, caption, prefix)) in [
        (45, [901, 0, 0], "Triet Desert", "LongRangeTrietDesert"),
        (
            46,
            [902, 0, 0],
            "Ossa Mountains and Izoold",
            "LongRangeOssaMountainsAndIzoold",
        ),
        (
            47,
            [904, 905, 0],
            "E. Palmacosta and Thoda",
            "LongRangeEPalmacostaAndThoda",
        ),
        (
            48,
            [903, 0, 0],
            "N.E. Palmacosta and Hakonesia Peak",
            "LongRangeNEPalmacostaAndHakonesiaPeak",
        ),
        (
            49,
            [906, 907, 0],
            "E. Luin and Asgard",
            "LongRangeELuinAndAsgard",
        ),
        (50, [909, 0, 0], "Katz Island", "LongRangeKatzIsland"),
        (51, [908, 0, 0], "W. Luin and Hima", "LongRangeWLuinAndHima"),
        (
            294,
            [911, 912, 0],
            "Meltokio region",
            "LongRangeMeltokioRegion",
        ),
        (
            295,
            [913, 914, 922],
            "Sybak and N.E. archipelago",
            "LongRangeSybakAndNEArchipelago",
        ),
        (
            296,
            [915, 917, 0],
            "Gaoracchia and Flanoir",
            "LongRangeGaoracchiaAndFlanoir",
        ),
        (
            297,
            [920, 921, 0],
            "the mining mountain range",
            "LongRangeTheMiningMountainRange",
        ),
        (
            298,
            [916, 918, 919],
            "Altamira and Ymir",
            "LongRangeAltamiraAndYmir",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 0x8c + index * 12;
        assert_eq!(halfword(data + offset + 4), key);
        let label = rodata + unlock_labels.remove(&offset).unwrap() as usize;
        let end = rel[label..].iter().position(|byte| *byte == 0).unwrap() + label;
        assert_eq!(&rel[label..end], caption.as_bytes());
        for (slot, flag) in flags.into_iter().enumerate() {
            assert_eq!(halfword(data + offset + 6 + slot * 2), flag);
            if flag != 0 {
                unlocked_flags.insert(flag);
                assert_eq!(
                    symphonia_constants::StoryFlag::try_from(flag)
                        .unwrap()
                        .name(),
                    format!("{prefix}Flag{}", slot + 1)
                );
            }
        }
    }
    assert!(unlock_labels.is_empty());
    assert_eq!(halfword(data + 0x11c + 4), 0);
    assert_eq!(
        unlocked_flags,
        (901..=922).filter(|flag| *flag != 910).collect()
    );

    let query = (0..7)
        .find_map(|section| {
            let start = word(&dol, 0x48 + section * 4);
            let size = word(&dol, 0x90 + section * 4);
            (0x800810d4 >= start && 0x800810d4 - start < size)
                .then(|| word(&dol, section * 4) as usize + (0x800810d4 - start) as usize)
        })
        .unwrap();
    assert_eq!(word(&dol, query + 0x10), 0x38630384); // addi r3,r3,900
    assert_eq!(word(&dol, query + 0x14), 0x2c0303ac); // compare upper flag bound
    assert_eq!(word(&dol, query + 0x48), 0x88a50c9d); // load the persistent flag byte
    assert_eq!(word(&dol, query + 0x54), 0x7ca30038); // test the selected bit
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_emitter_properties_address_the_constructor_parameter_slots() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    // Native registry entries contain the opcode and its actual DOL handler.
    for (opcode, entry, handler) in [
        (0x1c, 0x801e77a4, 0x8005673c),
        (0x1d, 0x801e77ac, 0x80055140),
        (0xbf, 0x801e7cb4, 0x80058f4c),
    ] {
        assert_eq!(at(entry), opcode << 16);
        assert_eq!(at(entry + 4), handler);
    }
    // Both switch dispatches index the original tables with the property.
    for (address, expected) in original_words::words::<u32>(
        "original_emitter_properties_address_the_constructor_parameter_slots",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let recipe_table: Vec<_> = (0..77)
        .flat_map(|index| at(0x8020aadc + index * 4).to_be_bytes())
        .collect();
    assert_eq!(
        Sha256::digest(&recipe_table)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "6b7742eef9c7d85bc6bc009e31efdff10e025fa8436341e483150a2ab8ae2e19"
    );

    let constructors = [
        (0x80059088, 0x80059090, 0x80840000, 0xb09f0000),
        (0x800590a8, 0x800590b0, 0x80840000, 0xb09f0000),
        (0x800590c0, 0x800590c4, 0x80040000, 0xb01f0000),
        (0x800590d4, 0x800590d8, 0x80040000, 0xb01f0000),
        (0x800590e8, 0x800590fc, 0x80040000, 0xd01f0000),
        (0x8005910c, 0x80059120, 0x80040000, 0xd01f0000),
        (0x80059130, 0x80059144, 0x80040000, 0xd01f0000),
        (0x80059154, 0x80059168, 0x80040000, 0xd01f0000),
        (0x80059178, 0x8005918c, 0x80040000, 0xd01f0000),
        (0x8005919c, 0x800591b0, 0x80040000, 0xd01f0000),
    ];
    for (slot, (load, store, load_op, store_op)) in constructors.into_iter().enumerate() {
        let slot = slot as u32;
        let property = 113 + slot;
        let offset = if slot < 4 {
            0xc4 + 2 * slot
        } else {
            0xcc + 4 * (slot - 4)
        };
        assert_eq!(at(load), load_op | (0x21c + slot * 4));
        assert_eq!(at(store), store_op | offset);
        let setter = at(0x801e83a8 + property * 4);
        let getter = at(0x801e8594 + property * 4);
        for case in [setter, getter] {
            assert_eq!(at(case), 0x3c608008); // lis r3,0x8008
            assert_eq!(at(case + 4), 0x809f0000); // lwz r4,callback
            assert_eq!(at(case + 8), 0x380372a4); // addi r0,r3,0x72a4
            assert_eq!(at(case + 12), 0x7c040040); // compare callback
            assert_eq!(
                at(case + 0x14),
                (if slot < 4 { 0xa89f0000 } else { 0xc01f0000 }) | offset
            );
        }
        if slot < 4 {
            assert_eq!(at(setter + 0x30), 0xb01f0000 | offset); // store halfword
        } else {
            assert_eq!(at(setter + 0x48), 0x5400043e); // unsigned 16-bit scalar write
            assert_eq!(at(setter + 0x58), 0xd01f0000 | offset); // store float
            assert_eq!(at(getter + 0x1c), 0xfc00001e); // truncate float read to integer
        }
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_dialogue_flags_select_yellow_and_the_conditional_close_timer() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_dialogue_flags_select_yellow_and_the_conditional_close_timer",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_trigger_query_uses_record_types_written_by_trigger_constructors() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    for (entry, opcode, handler) in [
        (0x801e7e1c, 0xec, 0x800530cc),
        (0x801e7a54, 0x73, 0x80052c20),
        (0x801e7a5c, 0x74, 0x80053418),
        (0x801e7a64, 0x75, 0x80051fdc),
        (0x801e78a4, 0x3c, 0x80051d14),
    ] {
        assert_eq!(at(entry), opcode << 16);
        assert_eq!(at(entry + 4), handler);
    }
    for (address, expected) in original_words::words::<u32>(
        "original_trigger_query_uses_record_types_written_by_trigger_constructors",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable and BTLusual.dat at local/extracted"]
fn original_formation_catalogue_names_every_enemy_group_and_battle_request_operand() {
    use sha2::{Digest, Sha256};
    use symphonia_constants::{FormationId, MonsterId};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(root.join("files/BTL/BTLusual.dat")).unwrap();
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&usual),
        "daf67bba141841c6a317c4bad950dc7dcfc5d3a90cfc7659d76b37e277af5d50"
    );
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_formation_catalogue_names_every_enemy_group_and_battle_request_operand",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }

    let header_word =
        |offset| u32::from_be_bytes(usual[offset..offset + 4].try_into().unwrap()) as usize;
    let start = header_word(8);
    let end = header_word(12);
    assert_eq!((start, end), (0x5c40, 0x1d340));
    assert_eq!(end - start, 1000 * 96);
    let mut bases = Vec::new();
    for row in usual[start..end].as_chunks::<96>().0 {
        assert_eq!(&row[..4], b"gp3\0");
        assert!(row[4] <= 8 && row[5] <= 4);
        let mut groups: Vec<(String, usize)> = Vec::new();
        for &resource in &row[16..16 + usize::from(row[4])] {
            assert!(resource < row[5]);
            let offset = 8 + usize::from(resource) * 2;
            let enemy = u16::from_be_bytes(row[offset..offset + 2].try_into().unwrap());
            let monster = MonsterId::try_from(u8::try_from(enemy).unwrap()).unwrap();
            let resource_name = monster.resource_name().unwrap();
            let stem = resource_name.rsplit_once('-').unwrap().0;
            if let Some((_, count)) = groups.iter_mut().find(|(name, _)| name == stem) {
                *count += 1;
            } else {
                groups.push((stem.into(), 1));
            }
        }
        let mut base = String::new();
        for (index, (stem, count)) in groups.iter().enumerate() {
            if index != 0 {
                base.push_str("And");
            }
            for word in stem.split('-') {
                let mut letters = word.chars();
                base.extend(letters.next().unwrap().to_uppercase());
                base.extend(letters);
            }
            if *count > 1 {
                base.push_str(&format!("X{count}"));
            }
        }
        if base.is_empty() {
            base.push_str("Empty");
        }
        bases.push(base);
    }
    let mut counts = BTreeMap::new();
    for base in &bases {
        *counts.entry(base.as_str()).or_insert(0usize) += 1;
    }
    assert_eq!(FormationId::ALL.len(), 1000);
    for (id, base) in bases.iter().enumerate() {
        let expected = if counts[base.as_str()] > 1 {
            format!("{base}Variant{id:04}")
        } else {
            base.clone()
        };
        let formation = FormationId::try_from(id as u16).unwrap();
        assert_eq!(formation.name(), expected, "formation {id}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_battle_options_use_low_bit_defeat_and_signed_music_override() {
    use sha2::{Digest, Sha256};
    use symphonia_constants::{BattleDefeatPolicy, BattleMusicSelection};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol");
    let dol = fs::read(path).unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_battle_options_use_low_bit_defeat_and_signed_music_override",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    for (policy, resumes) in [
        (BattleDefeatPolicy::GameOver, false),
        (BattleDefeatPolicy::ResumeEvent, true),
        (BattleDefeatPolicy::ResumeEventEncoded123, true),
    ] {
        assert_eq!(policy.raw() & 1 != 0, resumes);
    }
    for selection in BattleMusicSelection::ALL {
        let value = selection.raw();
        assert_eq!(if value == 0 { -1 } else { value as i16 }, -1);
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_animation_handler_reads_slots_and_flags_only_in_their_consuming_branches() {
    use sha2::{Digest, Sha256};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol");
    let dol = fs::read(path).unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_animation_handler_reads_slots_and_flags_only_in_their_consuming_branches",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable and resident SND bank at local/extracted"]
fn original_named_field_sound_cues_bind_distinct_programs_and_samples() {
    use sha2::{Digest, Sha256};
    use symphonia_constants::EffectId;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let bank = fs::read(root.join("files/S/se.snd")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        digest(&bank),
        "ed9c9cb040b00898a976753bbf0487eec41631d53e4c969f197fc36c34a2f93a"
    );
    let word = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let half = |bytes: &[u8], offset: usize| {
        u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
    };
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(&dol, 0x48 + section * 4);
                let size = word(&dol, 0x90 + section * 4);
                (address >= start && address - start < size).then(|| {
                    word(
                        &dol,
                        word(&dol, section * 4) as usize + (address - start) as usize,
                    )
                })
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_named_field_sound_cues_bind_distinct_programs_and_samples",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }

    // SND section0 owns public cue records; section1 owns typed program pools.
    // The IDs in these tables are not interchangeable.
    assert_eq!(word(&bank, 0), 4);
    let descriptor = word(&bank, 4) as usize;
    assert_eq!(half(&bank, descriptor + 6), 1);
    let sounds = descriptor + word(&bank, descriptor + 28) as usize;
    let count = usize::from(half(&bank, sounds));
    assert_eq!(count, 423);
    let pool = word(&bank, 12) as usize;
    let object = |kind: usize, id: u16| {
        let mut cursor = pool + word(&bank, pool + kind * 4) as usize;
        loop {
            let stride = word(&bank, cursor);
            assert_ne!(stride, u32::MAX, "missing pool kind {kind} object {id}");
            assert!(stride >= 8);
            if half(&bank, cursor + 4) == id {
                break &bank[cursor + 8..cursor + stride as usize];
            }
            cursor += stride as usize;
        }
    };
    for (cue, program, sample, target) in [
        (EffectId::AcquisitionFanfare, 459u16, 196u16, "cue-0036"),
        (EffectId::LearningFanfare, 711u16, 328u16, "cue-0037"),
        (EffectId::BodyImpact, 144u16, 94u16, "cue-0071"),
        (EffectId::DoorKnock, 137u16, 89u16, "cue-0156"),
        (EffectId::YmirFishMovement, 825u16, 406u16, "cue-0175"),
        (EffectId::LowRumble, 639u16, 295u16, "cue-0181"),
        (EffectId::ElectricalDischarge, 772u16, 367u16, "cue-0187"),
        (EffectId::ElectronicBeep, 351u16, 130u16, "cue-0189"),
        (EffectId::WeaponReady, 759u16, 356u16, "cue-0195"),
        (EffectId::StoneFloorCollapse, 76u16, 45u16, "cue-0210"),
        (EffectId::FireAmbience, 62u16, 36u16, "cue-0384"),
        (EffectId::WaterfallAmbience, 768u16, 364u16, "cue-0394"),
        (EffectId::MachineryDrive, 538u16, 220u16, "cue-0405"),
        (EffectId::MechanicalSlide, 33u16, 16u16, "cue-0239"),
        (EffectId::MechanismSignal, 597u16, 263u16, "cue-0246"),
        (
            EffectId::MachineryMotionVariant0248,
            516u16,
            221u16,
            "cue-0248",
        ),
        (EffectId::ElevatorDepartureBeep, 105u16, 263u16, "cue-0306"),
        (EffectId::MachineryMotion, 518u16, 221u16, "cue-0251"),
        (
            EffectId::MachineryMotionVariant0252,
            518u16,
            221u16,
            "cue-0252",
        ),
        // The shared sample does not make these two public cues aliases.
        (EffectId::RingRadar, 574u16, 130u16, "ring-radar"),
    ] {
        let record = (0..count)
            .map(|i| sounds + 4 + i * 10)
            .find(|&offset| half(&bank, offset) == cue.raw())
            .unwrap();
        assert_eq!(half(&bank, record + 2), program);
        assert_eq!(&bank[record + 4..record + 10], &[255, 10, 127, 64, 60, 0]);
        let body = object(0, program);
        let starts = body
            .as_chunks::<8>()
            .0
            .iter()
            .map(|command| word(command, 0))
            .filter(|command| command & 0xff == 0x10)
            .collect::<Vec<_>>();
        assert_eq!(starts, [u32::from(sample) << 8 | 0x10]);
        let pitch_offsets = body
            .as_chunks::<8>()
            .0
            .iter()
            .map(|command| word(command, 0))
            .filter(|command| command & 0xff == 0x18)
            .collect::<Vec<_>>();
        match cue {
            EffectId::MachineryMotionVariant0248 => {
                assert_eq!(pitch_offsets, [0x01000618]);
                assert_eq!(body.len(), 120);
                assert_eq!(
                    digest(body),
                    "7ea53819c4b141a7efbe64b8125aef0cb002ba2e1d1fc156e2ad2154681ddf12"
                );
            }
            EffectId::MechanismSignal => {
                assert!(pitch_offsets.is_empty());
                assert_eq!(body.len(), 96);
                assert_eq!(
                    digest(body),
                    "7f29f35cc4d96df28dfc078287c512f3afb5e63ebeb987d8dcd53812e79d2871"
                );
            }
            EffectId::BodyImpact => {
                assert!(pitch_offsets.is_empty());
                assert_eq!(body.len(), 64);
                assert_eq!(
                    digest(body),
                    "d06d049ebfd6140ff9110a6b223367f1ee8c23e3a29ca64c8bff611ff64bb9db"
                );
            }
            EffectId::WeaponReady => {
                assert!(pitch_offsets.is_empty());
                assert_eq!(body.len(), 96);
                assert_eq!(
                    digest(body),
                    "c5fa73cbe7806b64ec5839e36ae536309e17d469b51b3e2e1d472815bac2440e"
                );
            }
            EffectId::MachineryDrive => {
                assert!(pitch_offsets.is_empty());
                assert_eq!(body.len(), 120);
                assert_eq!(
                    digest(body),
                    "88e2232ffa5436f7d06dfb40a968fccea1d6f2e1c32fa62260ec84d4bb3a77af"
                );
            }
            EffectId::LowRumble => {
                assert_eq!(pitch_offsets, [0x01000218]);
                assert_eq!(body.len(), 104);
                assert_eq!(
                    digest(body),
                    "4651a39a3f0146105c3abc8b6793b629562eff498d61d25120204035536c4aef"
                );
            }
            EffectId::WaterfallAmbience => {
                assert!(pitch_offsets.is_empty());
                assert_eq!(body.len(), 120);
                assert_eq!(
                    digest(body),
                    "24599844cb6d0c19d60c61f1e95a2b810729378d7f2b9ed75ed4cedaac843326"
                );
            }
            EffectId::FireAmbience => {
                assert!(pitch_offsets.is_empty());
                assert_eq!(body.len(), 176);
                assert_eq!(
                    digest(body),
                    "780004f3f655b685db398adaa47a04634a9bcd19f46f06a5507eb886bb4d4560"
                );
            }
            EffectId::DoorKnock | EffectId::ElectricalDischarge | EffectId::StoneFloorCollapse => {
                assert!(pitch_offsets.is_empty());
                let (length, hash) = match cue {
                    EffectId::DoorKnock => (
                        48,
                        "a220b94d5d21f57ec27e77efb8300c32c9ec84ad366a4ee429fd79305d97b962",
                    ),
                    EffectId::ElectricalDischarge => (
                        104,
                        "64e3dfca2f8f09898dd83ab47e6acdecdffa4ae223a03d7f06a9d107f8cf888d",
                    ),
                    EffectId::StoneFloorCollapse => (
                        96,
                        "72c6882f6a434c5cdd2f58406746d77f8e1ab232b4c916e6b630560ce15dd1d6",
                    ),
                    _ => unreachable!(),
                };
                assert_eq!(body.len(), length);
                assert_eq!(digest(body), hash);
            }
            EffectId::YmirFishMovement => {
                assert_eq!(pitch_offsets, [0x0100fe18]);
                assert_eq!(body.len(), 136);
                assert_eq!(
                    digest(body),
                    "6fb87f0899474e43c253d2aec81f1f1af7d2044065923e898c23a0c35ae5e462"
                );
            }
            EffectId::ElectronicBeep => assert!(pitch_offsets.is_empty()),
            EffectId::ElevatorDepartureBeep => assert_eq!(pitch_offsets, [0x0100fb18]),
            EffectId::RingRadar => assert_eq!(pitch_offsets, [0x01000418]),
            EffectId::MachineryMotion | EffectId::MachineryMotionVariant0252 => {
                assert_eq!(pitch_offsets, [0x0100fb18]);
                assert_eq!(body.len(), 104);
                assert_eq!(
                    digest(body),
                    "868125f4fcb26bf1fb840c224e17ef437ce15468da7909eb34e65e42793412fa"
                );
            }
            _ => {}
        }
        // These names do not rename the existing editable audio targets.
        assert_eq!(cue.resource_name().unwrap(), target);
    }

    // RingModeChange is a public cue selecting a layer, not macro320 or
    // sample320 (the chest-opening sample). Keep every layer voice and its
    // original velocity/pan, including the reused earthquake-part programs.
    let record = (0..count)
        .map(|i| sounds + 4 + i * 10)
        .find(|&offset| half(&bank, offset) == EffectId::RingModeChange.raw())
        .unwrap();
    assert_eq!(half(&bank, record + 2), 0x80a1);
    assert_eq!(&bank[record + 4..record + 10], &[255, 10, 127, 64, 60, 0]);
    let layer = object(3, 0x80a1);
    assert_eq!(&layer[..4], &[0, 0, 0, 5]);
    assert_eq!(layer.len(), 4 + 5 * 12);
    for (entry, (program, velocity, pan, sample)) in layer[4..].as_chunks::<12>().0.iter().zip([
        (740u16, 127u8, 64u8, 346u16),
        (360, 110, 64, 202),
        (357, 90, 0, 202),
        (358, 70, 127, 202),
        (359, 50, 20, 202),
    ]) {
        assert_eq!(half(entry, 0), program);
        assert_eq!(&entry[2..8], &[0, 127, 0, velocity, 0, 0]);
        assert_eq!(&entry[8..12], &[pan, 0, 0, 0]);
        let words = object(0, program)
            .as_chunks::<8>()
            .0
            .iter()
            .map(|command| word(command, 0))
            .collect::<Vec<_>>();
        let samples = words
            .iter()
            .copied()
            .filter(|a| a & 255 == 0x10)
            .collect::<Vec<_>>();
        assert_eq!(samples, [u32::from(sample) << 8 | 0x10]);
        let pitches = words
            .iter()
            .copied()
            .filter(|a| a & 255 == 0x18)
            .collect::<Vec<_>>();
        assert_eq!(
            pitches,
            if program == 740 {
                vec![]
            } else {
                vec![0x0100fa18]
            }
        );
    }
    assert_eq!(EffectId::RingModeChange.resource_name(), Some("cue-0320"));
    // Layered movement cues retain every voice and its distinct controls.
    // Cue283 repeats program462 and cue367 repeats program275. Neither those
    // program IDs nor the layer IDs are interchangeable with public cue IDs.
    for (cue, layer_id, layer_hash, voices, target) in [
        (
            EffectId::SuccessFanfare,
            0x803c,
            "ab3733501d69cc2205553004b05413106cd7868684f25bc27f72aaee179da9a7",
            &[
                (
                    507u16,
                    171u16,
                    152usize,
                    "877d967ce3939e06ca1a6fe990e523f8e253b4962bef294ae85b078195ad863b",
                ),
                (
                    511u16,
                    171u16,
                    160usize,
                    "7123c11981183892497abc3b8873f9a15893370be9bccead549a9fa043545f9a",
                ),
                (
                    513u16,
                    171u16,
                    160usize,
                    "3e781dfbccfe35f3a9ee4eaa772218d0995fa49a22c158f2e95fa4e1ed308681",
                ),
                (
                    509u16,
                    171u16,
                    160usize,
                    "abe22a6bd007d5156c4111794475316196a23f664dac963d6695e58d1fdaaeff",
                ),
                (
                    510u16,
                    171u16,
                    160usize,
                    "76137d2b8de849a82d9455b8bdc2f7757b6905f50e191e5ebfd43e8858a4822a",
                ),
                (
                    512u16,
                    171u16,
                    160usize,
                    "4f5b590a3cfca52606ecf27700f4f737beab5d383663a9972d0a886ef340b4c9",
                ),
                (
                    508u16,
                    171u16,
                    160usize,
                    "3844ed1ccdbce8356bb70c2a4dff12a67834138342a4d89c6a0fffddba715304",
                ),
                (
                    506u16,
                    53u16,
                    160usize,
                    "06ac0b743c3d482f29a1e7a5716f7a3999d7c15b6002dcd27af9e52a128ba69f",
                ),
                (
                    504u16,
                    53u16,
                    168usize,
                    "71fdaa01df49b97610bfd6475ed46affafcdd28a61d0c5f52dbfa790a0b25cac",
                ),
                (
                    505u16,
                    53u16,
                    168usize,
                    "85b70a51dae67b0785bd68365bf8a8903106e6a2d100faa5639fa0b5b18042bb",
                ),
                (
                    514u16,
                    156u16,
                    128usize,
                    "4180d09dac644b4183dfb648ed0bab7df5e80d6c0b42916280090fdf23ce6367",
                ),
            ][..],
            "cue-0080",
        ),
        (
            EffectId::AlarmSignal,
            0x8053,
            "5699fb32283954ed97fc7f4fed9a03adedb6783cb6dfe1c9c1e0fbad9cb5e141",
            &[
                (
                    6u16,
                    5u16,
                    112usize,
                    "fadf53a51439434398e1b698182ba6c2adfe4e9dc292e5a771220f5af13cf351",
                ),
                (
                    7u16,
                    5u16,
                    128usize,
                    "7021d86e2a40ee93d55a55508fd50f35d7bdc40ac573cf588d70c66a209eedff",
                ),
                (
                    8u16,
                    5u16,
                    128usize,
                    "374b447bc4f449eadb2eb94f127e193da8f88f29ef552c100eb2ce42bcd644b8",
                ),
                (
                    9u16,
                    5u16,
                    128usize,
                    "d945307a423139c877c55882ebe03e32b9120fa80f3afbde7e4bf5a059f1041a",
                ),
                (
                    10u16,
                    5u16,
                    128usize,
                    "3144cba8e959c7864a9f48ca1a222a82f964005075f67a569a05037577f69f8d",
                ),
                (
                    11u16,
                    5u16,
                    128usize,
                    "fb1078bc723fd4e3ffbd90aac1ebbb87b851d65ad2b29e9a612e9c82c2719d79",
                ),
            ][..],
            "cue-0299",
        ),
        (
            EffectId::WonderChefPoof,
            0x8025,
            "a38da4138fb67d10d37c8b57b6ebb49eb87c0dc255f60fe3e25c96f9130dfae7",
            &[
                (
                    289u16,
                    343u16,
                    128usize,
                    "1e777836ec105a1fb87b8da0b370522a6fd32413a75d096a72a7ebe392280468",
                ),
                (
                    291,
                    343,
                    128,
                    "88f1ebb6f4b1262f4a4705e900a6acf7b777233ab6b06ab4da19749b0c722f9f",
                ),
                (
                    290,
                    137,
                    128,
                    "1e2fe31204fd2fe736658f041afbc8ef4708eea81b05dafde02079b6ee07e2b8",
                ),
                (
                    292,
                    343,
                    136,
                    "70ad899d075a1f6c1f2edc6b980ba811243a8787cb46280e0550ec8bc12fb5d5",
                ),
                (
                    293,
                    343,
                    136,
                    "a294aeccecb7d74088891d967a089e2a60619c6511042a418788f4b5341fbafa",
                ),
            ][..],
            "cue-0373",
        ),
        (
            EffectId::SurfAmbience,
            0x80a7,
            "534636ddb806a186a0d8376d751474640b2c066dc0686235c510741e80792b43",
            &[
                (
                    834u16,
                    411u16,
                    144usize,
                    "2f29d0f0d718b44e66ef53cf6c7242ed1ba11484da8273539dfd0cbef3e97209",
                ),
                (
                    831,
                    411,
                    144,
                    "a37bc63fe367ea25379b9dabe3270770172ec692f9da84866a4c78ad22c132d5",
                ),
                (
                    833,
                    411,
                    144,
                    "e9a898029fb125187e5ae53eb6b4486786cadc1b505fc009ae51bbc3f4b68df7",
                ),
                (
                    832,
                    411,
                    152,
                    "51ed4e589f8d94e5d0f92509d1b4a42229686fb413c2677f295371ebc8f52a14",
                ),
            ][..],
            "cue-0385",
        ),
        (
            EffectId::FlowingWaterAmbience,
            0x8038,
            "f54bbb3ea57fde01ec83b82d048d6faf5c8f36d106f40aec40b3b485ff03776d",
            &[
                (
                    480,
                    201,
                    112,
                    "4f246ffa77462ae1700f79bac09ee7a26143cb0c6d0f441b147ce2cbbb6c7102",
                ),
                (
                    481,
                    201,
                    128,
                    "eb262fc50f4bcc3dea443ed89daf64524a94661d1400aa18eaa69052d3da3b6b",
                ),
            ][..],
            "cue-0386",
        ),
        (
            EffectId::MagicalFade,
            0x80a4,
            "8788c98b8a638e97fcb4a0d34617736240705b6757b608cb9154b8c3b617ace5",
            &[
                (
                    811u16,
                    236u16,
                    152usize,
                    "14cbe3566b397be20e81016421656cbfc0f86c6e94ceaa5cf7c815bb82e97879",
                ),
                (
                    812,
                    236,
                    152,
                    "4e1c5143ab2c195fa1ab43b1086dc33917ee8dfafc7bed5aa99e684aa4bd5772",
                ),
                (
                    813,
                    240,
                    120,
                    "b67490965accf0a4ac4e50f3d98e83a8b0690f1fe498bd819d10cbbbb7aa016e",
                ),
            ][..],
            "cue-0275",
        ),
        (
            EffectId::JumpTakeoff,
            0x8084,
            "6dc5a06fa06641844e4c31b8914ba7044196dae026267c63d44092f21a27d979",
            &[
                (
                    460u16,
                    349u16,
                    88usize,
                    "5132c466f657a4b187d51cfc272389ac81d0bec5d91c30718727e854f8acf7fc",
                ),
                (
                    461,
                    332,
                    112,
                    "07854943b9a16ef9e59e2640971d6cf1e0a1a0eb27173c578e3d8922f7229d57",
                ),
            ][..],
            "cue-0282",
        ),
        (
            EffectId::LandingImpact,
            0x8085,
            "861275a6e7061ae4897d473a5350cf334384a1033c0a7ca543549e0fb6998d71",
            &[
                (
                    462,
                    393,
                    72,
                    "e6a7d21ced3561ce0f0371aa21f50a3d1a3d0d7d8fd7d2dc28f81b8f471a9e25",
                ),
                (
                    463,
                    393,
                    80,
                    "e128842f482dec5e1c7c95f5dcf24beeee1abdd204b19ae18aa5c7dc8f2eb165",
                ),
                (
                    464,
                    398,
                    72,
                    "c7c91b7420627b71478821b5e7d6096d2fe7c0e0d5d375cf17200f317c5906e0",
                ),
                (
                    462,
                    393,
                    72,
                    "e6a7d21ced3561ce0f0371aa21f50a3d1a3d0d7d8fd7d2dc28f81b8f471a9e25",
                ),
            ][..],
            "cue-0283",
        ),
        (
            EffectId::TeleportPadActivation,
            0x8021,
            "320729e5ec461e2d1f160bd762082adc7eb61cd03f8dbdd6a593a4ec3df54863",
            &[
                (
                    273u16,
                    211u16,
                    104usize,
                    "54a034633b6d46c8ec93df08d6b08d2c7f08d6c3cb8de0b67d5383fd76b8f2a4",
                ),
                (
                    274,
                    212,
                    104,
                    "a4d1b12c8ae5a785bb5343f1bc079021efff5656f7aeb352a978e6f061e76e4c",
                ),
            ][..],
            "cue-0366",
        ),
        (
            EffectId::TeleportDissolve,
            0x8022,
            "82ecdb20c631265ae26a2e7151977ca2dc711e2f99e02b1f0543b313068e94e7",
            &[
                (
                    275,
                    109,
                    128,
                    "7a33bb6c2abb186b296fcc8258bfd2e7f7295971e03fc78a43b532862ab2f931",
                ),
                (
                    275,
                    109,
                    128,
                    "7a33bb6c2abb186b296fcc8258bfd2e7f7295971e03fc78a43b532862ab2f931",
                ),
                (
                    276,
                    25,
                    136,
                    "2d694d9ee11be12fc9d76e21b4125648ed442712443ee730b0bd13dde08455a5",
                ),
                (
                    277,
                    25,
                    136,
                    "38b54744475b2c25e3a5f98d2e2d8153a3f9291bbbe687d3f8bb104461421f08",
                ),
                (
                    278,
                    25,
                    136,
                    "714069f5d61dd8ce97fbc1b55aefcefafa314cf767f3ab3e46756d788bf225ca",
                ),
                (
                    279,
                    25,
                    136,
                    "a42698f304d8aaa1691c01122f2485e535368956450849efc8a75efe9dc985a9",
                ),
            ][..],
            "cue-0367",
        ),
    ] {
        let record = (0..count)
            .map(|i| sounds + 4 + i * 10)
            .find(|&offset| half(&bank, offset) == cue.raw())
            .unwrap();
        assert_eq!(half(&bank, record + 2), layer_id);
        assert_eq!(&bank[record + 4..record + 10], &[255, 10, 127, 64, 60, 0]);
        let layer = object(3, layer_id);
        assert_eq!(word(layer, 0) as usize, voices.len());
        assert_eq!(layer.len(), 4 + voices.len() * 12);
        assert_eq!(digest(layer), layer_hash); // includes every voice control
        for (entry, &(program, sample, length, hash)) in
            layer[4..].as_chunks::<12>().0.iter().zip(voices)
        {
            assert_eq!(half(entry, 0), program);
            let body = object(0, program);
            assert_eq!(body.len(), length);
            assert_eq!(digest(body), hash);
            let samples: Vec<_> = body
                .as_chunks::<8>()
                .0
                .iter()
                .map(|command| word(command, 0))
                .filter(|command| command & 255 == 0x10)
                .collect();
            assert_eq!(samples, [u32::from(sample) << 8 | 0x10]);
        }
        assert_eq!(cue.resource_name(), Some(target));
    }
    let chest = (0..count)
        .map(|i| sounds + 4 + i * 10)
        .find(|&offset| half(&bank, offset) == EffectId::OpenChest.raw())
        .unwrap();
    assert_eq!(half(&bank, chest + 2), 689);
    let chest_samples = object(0, 689)
        .as_chunks::<8>()
        .0
        .iter()
        .map(|command| word(command, 0))
        .filter(|a| a & 255 == 0x10)
        .map(|a| a >> 8)
        .collect::<Vec<_>>();
    assert_eq!(chest_samples, [320, 322]);
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_wonder_chef_cues_keep_all_thirty_eight_smoke_requests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let mut count = 0;
    for field in [
        "altamira-0022",
        "asgard-0047",
        "dirks-house-0375",
        "exire-0187",
        "flanoir-0321",
        "heimdall-0289",
        "hima-0305",
        "izoold-0358",
        "latheon-gorge-0499",
        "luin-0462",
        "meltokio-0377",
        "meltokio-0388",
        "meltokio-0399",
        "mizuho-0327",
        "ozette-0427",
        "ozette-0431",
        "palmacosta-0068",
        "sybak-0445",
        "triet-0487",
    ] {
        let path = root.join(format!("fields/{field}/scripts/scenario.ssb"));
        let bytes = fs::read(&path).unwrap();
        let calls = symphonia_script::scenario::calls::arguments(
            &bytes,
            symphonia_script::NativeCall::PlaySoundSimple,
            2,
        )
        .unwrap();
        assert_eq!(
            calls.values().filter(|a| a[0] == Some(373)).count(),
            2,
            "{field}"
        );
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        assert!(
            result
                .source
                .contains("resources::FieldModelId::WonderChef")
        );
        for (at, _) in result
            .source
            .match_indices("effects::EffectId::WonderChefPoof")
        {
            let tail = &result.source[at..];
            let end = tail.find(");").unwrap() + 2;
            let effect = tail[end..].trim_start();
            assert!(
                effect.starts_with("native::create_effect_emitter("),
                "{field}"
            );
            let end = effect.find(");").unwrap();
            assert!(effect[..end].contains("recipe: scenario::EmitterRecipe::RisingSmoke"));
            count += 1;
        }
        assert_eq!(
            scenario::compile(field, &result.source, &definitions).unwrap(),
            bytes
        );
    }
    assert_eq!(count, 38);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_sound_arguments_are_pan_volume_and_binding_index() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(section * 4) as usize + (address - base) as usize)
            })
            .unwrap()
    };
    for (address, length, hash) in [
        (
            0x8004ecb0,
            380,
            "91c99079bdcab36286c7e7abd545651384c727eb704bf8a75b1dfd5797e618ba",
        ),
        (
            0x8004ef28,
            128,
            "a8ef4b978b80cba8937de829b7d762a49fbc3fbe95967db814cba60a71fa2072",
        ),
        (
            0x80070790,
            124,
            "a9b3956f39f26207b8efbc139300c17ed24c2f9c8a00278918a9d37954a6e595",
        ),
        (
            0x8007080c,
            124,
            "4e1d0216a4a1f481a0273980dd55fdd07b0950e64e2017da87f4e21c7e9d817b",
        ),
    ] {
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + length]), hash, "{address:#x}");
    }
    for (address, expected) in
        original_words::words::<u32>("original_sound_arguments_are_pan_volume_and_binding_index")
    {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    assert_eq!(symphonia_constants::SoundVolume::FieldDefault.raw(), 255);
    assert_eq!(symphonia_constants::SoundBinding::Unbound.raw(), 255);
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0x52).unwrap().arguments,
        ["sound_id", "pan_offset"]
    );
    assert_eq!(
        registry.get(0xe0).unwrap().arguments,
        ["sound_id", "pan_offset", "volume", "binding_index"]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_sound_volume_uses_seven_bits_after_spatial_attenuation() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(section * 4) as usize + (address - base) as usize)
            })
            .unwrap()
    };
    for (address, length, hash) in [
        // Slot operations, one-shot attenuation, ambient attenuation.
        (
            0x8004eb54,
            348,
            "da96d69fa05ec624ec432cf232f3f399a5595c5da49deecf44ec4252cd952fcb",
        ),
        (
            0x80070590,
            340,
            "8eaea718e9ae682a95a89663853f7907536d1cd56630aca4430342bd4a62ff74",
        ),
        (
            0x800706e4,
            172,
            "de0356deca909683dbff922f944002a1b0c3af1a274e53cde4cf4f356eacaa43",
        ),
        // Common volume service, SDK wrapper, voice traversal, controller store.
        (
            0x800a206c,
            64,
            "90546e5e8428890efa08ed0a268296156e8d3c9f7523a25c4732aeabcf44559e",
        ),
        (
            0x80138da8,
            96,
            "25225b2d3b33910328cce87ab3b1f2a78460414259229cb6d64459054386271d",
        ),
        (
            0x80137cfc,
            228,
            "4e231e1f0fa9f94d98b5c1263965546b9cb16f8cf19bf65cc3907b6b9bb46869",
        ),
        (
            0x8014cbec,
            1308,
            "cb407649181166579d3128f9580babc208ec8009a4c4c5d92e4e86b0c0bdaeb6",
        ),
    ] {
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + length]), hash, "{address:#x}");
    }
    for (address, expected) in original_words::words::<u32>(
        "original_sound_volume_uses_seven_bits_after_spatial_attenuation",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_unusual_sound_volumes_preserve_their_script_operands() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, pc, cue, volume) in [
        ("toize-valley-mine-0414", 0x132d, 132, 200),
        ("field-0240", 0x3644, 390, -80),
        ("izoold-0354", 0x10fd, 389, -80),
        ("dirks-house-0373", 0x694, 390, -80),
    ] {
        let path = root.join(format!("fields/{field}/scripts/scenario.ssb"));
        let bytes = fs::read(&path).unwrap();
        let calls = symphonia_script::scenario::calls::arguments(
            &bytes,
            symphonia_script::NativeCall::PlaySound,
            4,
        )
        .unwrap();
        assert_eq!(calls[&pc], [Some(cue), Some(0), Some(volume), Some(255)]);
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        let compact = result
            .source
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(compact.contains(&format!(
            "volume: {volume}, binding_index: scenario::SoundBinding::Unbound"
        )));
        assert_eq!(
            scenario::compile(field, &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_actor_audio_uses_field_cues_and_separate_emitter_disable_semantics() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_actor_audio_uses_field_cues_and_separate_emitter_disable_semantics",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    // The entire ambient handler is inline lookup/stores: disabling an actor
    // does not invoke the slot-release service or stop an earlier one-shot cue.
    for address in (0x8004e95c..0x8004eb54).step_by(4) {
        assert_ne!(at(address) & 0xfc000001, 0x48000001);
    }
    assert_eq!(
        symphonia_constants::ActorSelector::Controlled.raw(),
        0xf0000 + 0x423f
    );
    assert_eq!(symphonia_constants::AmbientSoundControl::Disable.raw(), -1);
    assert_eq!(symphonia_constants::SpatialSoundVolume::Default.raw(), 255);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_particle_constructors_share_recipes_and_keep_scalar_motion_units() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_particle_constructors_share_recipes_and_keep_scalar_motion_units",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    use symphonia_constants::ParticleRecipe;
    // Dispatch targets and descriptor selections for every published recipe name.
    for (recipe, target, descriptor_pc, descriptor_instruction) in [
        (ParticleRecipe::Glow, 0x80083194, 0x80083194, 0x381f01fc),
        (ParticleRecipe::Smoke, 0x80083234, 0x80083234, 0x387f01d4),
        (ParticleRecipe::RedGlow, 0x8008329c, 0x8008329c, 0x381f01fc),
        (
            ParticleRecipe::StationGlow,
            0x8008338c,
            0x8008338c,
            0x381f0274,
        ),
        (
            ParticleRecipe::CameraDisc,
            0x800833a8,
            0x800833a8,
            0x381f028c,
        ),
        (
            ParticleRecipe::WorldGlow,
            0x800833c4,
            0x800833c4,
            0x387f028c,
        ),
        (ParticleRecipe::Star, 0x80083458, 0x80083458, 0x381f0298),
        (
            ParticleRecipe::SpinningStar,
            0x8008347c,
            0x8008347c,
            0x381f0298,
        ),
        (
            ParticleRecipe::AirRefraction,
            0x800834ec,
            0x800834ec,
            0x381f052c,
        ),
        (ParticleRecipe::Orb, 0x80083580, 0x80083580, 0x381f02a4),
        (ParticleRecipe::Flame, 0x8008359c, 0x8008359c, 0x381f02b0),
        (
            ParticleRecipe::TrailGlow,
            0x800835e0,
            0x800835e0,
            0x381f02bc,
        ),
        (ParticleRecipe::Burst, 0x80083624, 0x80083624, 0x381f02c8),
        (
            ParticleRecipe::ElectricArc,
            0x80083668,
            0x80083668,
            0x381f02d4,
        ),
        (ParticleRecipe::Swirl, 0x800837d8, 0x800837d8, 0x381f02ec),
        (
            ParticleRecipe::SparkleCluster,
            0x8008387c,
            0x8008387c,
            0x381f0320,
        ),
        (
            ParticleRecipe::StationHalo,
            0x800838c0,
            0x800838c0,
            0x381f032c,
        ),
        (ParticleRecipe::Streak, 0x80083904, 0x80083904, 0x381f0338),
        (
            ParticleRecipe::FallingLeaf,
            0x800839b8,
            0x800839b8,
            0x389f0344,
        ),
        (
            ParticleRecipe::RisingLeaf,
            0x800839f8,
            0x800839f8,
            0x389f0344,
        ),
        (
            ParticleRecipe::CameraRipple,
            0x8008353c,
            0x8008353c,
            0x381f0538,
        ),
        (
            ParticleRecipe::WorldRipple,
            0x80083530,
            0x8008353c,
            0x381f0538,
        ),
        (
            ParticleRecipe::TextureSlot0,
            0x80083f88,
            0x80083f88,
            0x381f0550,
        ),
        (
            ParticleRecipe::TextureSlot7,
            0x80083fdc,
            0x80083fdc,
            0x381f05a4,
        ),
        (
            ParticleRecipe::CameraRing,
            0x80083414,
            0x80083414,
            0x381f028c,
        ),
        (ParticleRecipe::Ring, 0x80083430, 0x80083430, 0x387f028c),
        (
            ParticleRecipe::ElectricSpark,
            0x80083a3c,
            0x80083a3c,
            0x381f03b0,
        ),
        (
            ParticleRecipe::FallingSpark,
            0x80083a58,
            0x80083a58,
            0x389f03e8,
        ),
        (
            ParticleRecipe::ImpactFlare,
            0x80083b34,
            0x80083b34,
            0x381f040c,
        ),
        (
            ParticleRecipe::SealSpark,
            0x80083ca4,
            0x80083ca4,
            0x381f0418,
        ),
        (
            ParticleRecipe::SquareGlow,
            0x80083958,
            0x80083958,
            0x381f0380,
        ),
        (
            ParticleRecipe::LightningBolt,
            0x80083d14,
            0x80083d14,
            0x381f0460,
        ),
        (
            ParticleRecipe::FlamePlume,
            0x80083d30,
            0x80083d30,
            0x381f046c,
        ),
    ] {
        assert_eq!(
            at(0x8020a86c + u32::from(recipe.raw()) * 4),
            target,
            "{recipe}"
        );
        assert_eq!(at(descriptor_pc), descriptor_instruction, "{recipe}");
    }
    assert_eq!(at(0x8000332c), 0x3c408036);
    assert_eq!(at(0x80003330), 0x60422e40); // r2 =80362E40
    assert_eq!(f32::from_bits(at(0x8035b548)), 0.0);
    assert_eq!(f32::from_bits(at(0x8035b54c)), 1.0);
    assert_eq!(f32::from_bits(at(0x8035b564)), -1.0); // Aspect-ratio size sentinel.
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0xd0).unwrap().arguments,
        [
            "sprite",
            "lifetime",
            "x",
            "y",
            "z",
            "velocity_x",
            "velocity_y",
            "velocity_z",
            "size",
            "alpha",
            "fade",
            "palette",
            "variant"
        ]
    );
    for opcode in [0xd0, 0xd3] {
        assert_eq!(
            registry.get(opcode).unwrap().argument_types[0],
            "scenario::ParticleRecipe"
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_node_queries_distinguish_local_names_fixed_parts_and_host_actor_keys() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_node_queries_distinguish_local_names_fixed_parts_and_host_actor_keys",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    use symphonia_constants::BodyPartId;
    let byte = |address: u32| (at(address & !3) >> (24 - (address & 3) * 8)) as u8;
    for (part, address, name) in [
        (BodyPartId::Neck, 0x8017c234, "Bone_kubi"),
        (BodyPartId::Head, 0x8017c240, "Bone_atama"),
        (BodyPartId::Spine1, 0x8017c24c, "Bone_sebone01"),
        (BodyPartId::Spine2, 0x8017c25c, "Bone_sebone02"),
        (BodyPartId::Spine3, 0x8017c26c, "Bone_sebone03"),
        (BodyPartId::LeftArm1, 0x8017c27c, "Bone_ude01_L"),
        (BodyPartId::LeftArm2, 0x8017c28c, "Bone_ude02_L"),
        (BodyPartId::RightArm1, 0x8017c29c, "Bone_ude01_R"),
        (BodyPartId::RightArm2, 0x8017c2ac, "Bone_ude02_R"),
        (BodyPartId::LeftLeg1, 0x8017c2bc, "Bone_ashi01_L"),
        (BodyPartId::LeftLeg2, 0x8017c2cc, "Bone_ashi02_L"),
        (BodyPartId::LeftLeg3, 0x8017c2dc, "Bone_ashi03_L"),
        (BodyPartId::LeftLeg4, 0x8017c2ec, "Bone_ashi04_L"),
        (BodyPartId::RightLeg1, 0x8017c2fc, "Bone_ashi01_R"),
        (BodyPartId::RightLeg2, 0x8017c30c, "Bone_ashi02_R"),
        (BodyPartId::RightLeg3, 0x8017c31c, "Bone_ashi03_R"),
        (BodyPartId::RightLeg4, 0x8017c32c, "Bone_ashi04_R"),
    ] {
        assert_eq!(
            at(0x801e76b0 + u32::from(part.raw()) * 4),
            address,
            "{part}"
        );
        let actual: Vec<_> = (0..=name.len()).map(|i| byte(address + i as u32)).collect();
        assert_eq!(
            actual,
            name.bytes().chain([0]).collect::<Vec<_>>(),
            "{part}"
        );
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0xbc).unwrap().arguments,
        ["actor_id", "name_index"]
    );
    assert_eq!(
        registry.get(0xb4).unwrap().arguments,
        ["actor_id", "body_part"]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_message_controls_evaluate_before_resolving_their_separate_id_tables() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // Execute at src+1, then consume the final result using the control-specific ABI.
    for (address, expected) in original_words::words::<u32>(
        "original_message_controls_evaluate_before_resolving_their_separate_id_tables",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    use symphonia_constants::MessageControl;
    for (control, target) in [
        (MessageControl::CharacterName, 0x8005b0a8),
        (MessageControl::Delay, 0x8005b11c),
        (MessageControl::Color, 0x8005b140),
        (MessageControl::ItemName, 0x8005b164),
        (MessageControl::Number, 0x8005b19c),
        (MessageControl::Evaluate, 0x8005b2e4),
        (MessageControl::Control7, 0x8005b1cc),
        (MessageControl::NumberInput, 0x8005b1f0),
        (MessageControl::Voice, 0x8005b248),
        (MessageControl::TitleName, 0x8005b26c),
        (MessageControl::TechniqueName, 0x8005b2ac),
    ] {
        assert_eq!(
            at(0x801e87c8 + u32::from(control.raw()) * 4),
            target,
            "{control}"
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_attachment_query_preserves_battle_key_origin_and_coordinate_writes() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // Bone -1 returns before lookup. Mode9 uses the bone as request key, too.
    // Field aliases select actors; both paths transform the origin and truncate XYZ.
    for (address, expected) in original_words::words::<u32>(
        "original_attachment_query_preserves_battle_key_origin_and_coordinate_writes",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    for address in [0x8017c33c, 0x8017c340, 0x8017c344] {
        assert_eq!(at(address), 0, "origin {address:#x}");
    }
    assert_eq!(
        symphonia_script::semantics::NativeRegistry::gqseaf()
            .get(0xcc)
            .unwrap()
            .arguments,
        ["actor_id", "bone_index"]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_rotation_and_scale_controllers_keep_axis_order_and_interpolation_inputs() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_rotation_and_scale_controllers_keep_axis_order_and_interpolation_inputs",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    assert_eq!(at(0x8000332c), 0x3c408036);
    assert_eq!(at(0x80003330), 0x60422e40); // r2 =80362E40
    assert_eq!(f32::from_bits(at(0x8035b548)), 0.0);
    assert_eq!(f32::from_bits(at(0x8035b54c)), 1.0);
    assert_eq!(f32::from_bits(at(0x8035b560)), 100.0);
    assert_eq!(f32::from_bits(at(0x8035b56c)), 30.0);
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0xb5).unwrap().arguments,
        [
            "actor_id",
            "controller_slot",
            "bone_index",
            "rotation_x",
            "rotation_z",
            "rotation_y"
        ]
    );
    assert_eq!(
        registry.get(0xc1).unwrap().arguments,
        [
            "actor_id",
            "controller_slot",
            "bone_index",
            "rotation_x",
            "rotation_z",
            "rotation_y",
            "duration"
        ]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_bone_translation_and_frame_waits_keep_slots_aliases_and_host_branches() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_bone_translation_and_frame_waits_keep_slots_aliases_and_host_branches",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    assert_eq!(at(0x8000332c), 0x3c408036); // r2 =80362E40
    assert_eq!(at(0x80003330), 0x60422e40);
    assert_eq!(f32::from_bits(at(0x8035b548)), 0.0);
    assert_eq!(f32::from_bits(at(0x8035b54c)), 1.0);
    assert_eq!(
        symphonia_constants::ActorSelector::MainScenery.raw(),
        999996
    );
    assert_eq!(
        symphonia_constants::ActorSelector::ThirdScenery.raw(),
        999998
    );
    assert_eq!(symphonia_constants::BoneControllerSlot::Slot7.raw(), 7);
}

#[test]
#[ignore = "requires the original GQSEAF executable and cooked settings messages"]
fn original_session_commands_keep_toggle_queries_and_repeatable_clear_recording() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_session_commands_keep_toggle_queries_and_repeatable_clear_recording",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    for (index, target) in [
        0x80043a8c, 0x80043aa4, 0x80043ac4, 0x80043ae4, 0x80043b04, 0x80043b44, 0x80043b94,
        0x80043bac, 0x80043bc4, 0x80043bdc, 0x80043bfc, 0x8004436c, 0x80044378, 0x80044398,
        0x800443c8, 0x800443f8, 0x80044448, 0x80043b24, 0x80044418,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(at(0x801e80e4 + index as u32 * 4), target);
    }
    let messages: serde_json::Value = serde_json::from_slice(
        &fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/all-assets/fields/new-game-settings/messages.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for (index, label) in [
        (5, "Skit Title Display"),
        (17, "Rumble Feature"),
        (23, "Sound Mode"),
        (27, "Mono\nStereo\n"),
    ] {
        assert!(
            messages[index]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part.as_str() == Some(label))
        );
    }
    assert_eq!(symphonia_constants::SessionCommand::LeaderLocked.raw(), 12);
    assert_eq!(symphonia_constants::SessionCommand::MenuDisabled.raw(), 15);
    assert_eq!(symphonia_constants::SessionToggle::Query.raw(), -1);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_party_recovery_dispatch_and_gald_arithmetic_preserve_native_operations() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_party_recovery_dispatch_and_gald_arithmetic_preserve_native_operations",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let table = [
        0x80051200, 0x800513ac, 0x8005121c, 0x80051228, 0x80051234, 0x80051240, 0x8005124c,
        0x80051258, 0x80051264, 0x80051270, 0x8005127c, 0x80051288, 0x80051294, 0x800512b8,
        0x80051300, 0x80051324, 0x80051348, 0x8005136c, 0x80051378, 0x80051384, 0x80051390,
        0x8005139c, 0x800512dc, 0x800513a8,
    ];
    for (index, target) in table.into_iter().enumerate() {
        assert_eq!(at(0x801e8320 + index as u32 * 4), target);
    }
    let branch_target = |pc| {
        let instruction = at(pc);
        assert_eq!(instruction & 0xfc000003, 0x48000001);
        pc.wrapping_add((((instruction & 0x03fffffc) as i32) << 6 >> 6) as u32)
    };
    for (first_mode, helper) in [(2, 0x80080554), (7, 0x8008044c), (17, 0x80080234)] {
        for (offset, percent) in [100, 50, 10, 5, 1].into_iter().enumerate() {
            let pc = table[first_mode + offset];
            assert_eq!(at(pc), 0x38600000 | percent);
            assert_eq!(branch_target(pc + 4), helper);
        }
    }
    for (mode, percent) in [(12, 100), (13, 50), (14, 10), (15, 5), (16, 1), (22, 20)] {
        let pc = table[mode];
        assert_eq!(at(pc + 4), 0x38800000 | percent);
        assert_eq!(branch_target(pc + 16), 0x80080850); // leader damage display
        assert_eq!(branch_target(pc + 20), 0x8000adec);
        assert_eq!(at(pc + 24), 0x38600000 | percent);
        assert_eq!(branch_target(pc + 28), 0x80080334); // all-party HP reduction
    }
    assert_eq!(branch_target(table[0]), 0x80080720);
    assert_eq!(branch_target(table[23]), 0x8008065c);
    assert_eq!(symphonia_constants::PartyRecoveryMode::Rest.raw(), 0);
    assert_eq!(
        symphonia_constants::PartyRecoveryMode::DamageHp20Percent.raw(),
        22
    );
    assert_eq!(
        symphonia_constants::PartyRecoveryMode::ReviveIncapacitated.raw(),
        23
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_trigger_geometry_stores_vertices_height_and_full_width_keys() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_trigger_geometry_stores_vertices_height_and_full_width_keys",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_scenery_animation_constructor_and_edits_route_layers_differently() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_scenery_animation_constructor_and_edits_route_layers_differently",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    assert_eq!(at(0x8000332c), 0x3c408036); // r2 =80362E40
    assert_eq!(at(0x80003330), 0x60422e40);
    assert_eq!(f32::from_bits(at(0x8035b548)), 0.0);
    assert_eq!(f32::from_bits(at(0x8035b588)), 200.0);
    assert_eq!(symphonia_constants::SceneryAnimationChannel::Base.raw(), -1);
    assert_eq!(symphonia_constants::SceneryLayer::FourthActor.raw(), 999980);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_actor_decision_selector_leaves_out_of_range_encodings_unassigned() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // Explicit state+98 is copied first. Otherwise the byte+9A is bounded
    // by12 and larger values branch directly to the common epilogue.
    for (address, expected) in original_words::words::<u32>(
        "original_actor_decision_selector_leaves_out_of_range_encodings_unassigned",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    // Native selectors6..9 are also unassigned; the named observed selectors
    // above12 take the same epilogue without indexing this table.
    for (index, target) in [
        0x800231b0, 0x800231bc, 0x800231c8, 0x800231e0, 0x800231ec, 0x80023238, 0x8002324c,
        0x8002324c, 0x8002324c, 0x8002324c, 0x80023244, 0x800231d4, 0x80023238,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(at(0x801e3f24 + index as u32 * 4), target);
    }
    assert_eq!(symphonia_constants::ActorBehavior::NoDecision13.raw(), 13);
    assert_eq!(symphonia_constants::ActorBehavior::NoDecision90.raw(), 90);
    // 300 is narrowed to44 by the constructor, but the source must retain300.
    assert_eq!(300_i32 as u8, 44);
    assert!(symphonia_constants::ActorBehavior::try_from(44).is_err());
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_scene_constructors_bind_literal_keys_models_and_npc_behavior() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_scene_constructors_bind_literal_keys_models_and_npc_behavior",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    for opcode in [0xb9, 0xd6] {
        assert_eq!(
            registry.get(opcode).unwrap().arguments,
            [
                "actor_id", "x", "y", "z", "heading", "resource", "behavior", "speed"
            ]
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_spawn_handlers_separate_battle_requests_emotes_and_scene_keys() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_spawn_handlers_separate_battle_requests_emotes_and_scene_keys",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_ring_station_constructor_skips_operand4_and_resolves_the_last_model() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = Sha256::digest(&dol)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_ring_station_constructor_skips_operand4_and_resolves_the_last_model",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    // Self-based argument loads cover actor/X/Y/Z/model. There is no load of
    // the intervening +23c slot, despite its ring-mode-shaped retail values.
    let loads = (0x80050af0..0x80050c20)
        .step_by(4)
        .filter_map(|pc| {
            let instruction = at(pc);
            let base = (instruction >> 16) & 31;
            let target = (instruction >> 21) & 31;
            let displacement = instruction & 0xffff;
            (instruction >> 26 == 32
                && base == target
                && matches!(base, 3 | 5)
                && (0x22c..=0x240).contains(&displacement))
            .then_some(displacement)
        })
        .collect::<Vec<_>>();
    assert_eq!(loads, [0x22c, 0x230, 0x234, 0x238, 0x240]);
    let model = symphonia_constants::FieldModelId::SorcerersRingStation;
    assert_eq!(model.raw(), 363);
    assert_eq!(model.resource_name(), Some("props/model-363"));
}

#[test]
#[ignore = "requires the original GQSEAF executable and battle REL at local/extracted"]
fn original_animation_flags_start_at_one_and_zero_updates_only_resume() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let rel = fs::read(root.join("files/US_r_Top2Btl.rel")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        digest(&rel),
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    let word = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(&dol, 0x48 + section * 4);
                let size = word(&dol, 0x90 + section * 4);
                (address >= start && address - start < size).then(|| {
                    word(
                        &dol,
                        word(&dol, section * 4) as usize + (address - start) as usize,
                    )
                })
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_animation_flags_start_at_one_and_zero_updates_only_resume",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }

    let sections = word(&rel, 0x10) as usize;
    let text = (word(&rel, sections + 8) & !3) as usize;
    assert_eq!(word(&rel, 0), 1);
    assert_eq!(text, 0xcc);
    for (offset, expected) in original_words::words::<usize>(
        "original_animation_flags_start_at_one_and_zero_updates_only_resume-2",
    ) {
        assert_eq!(word(&rel, text + offset), expected, "REL text+{offset:#x}");
    }

    // Bind the exported callback entries and shared engine imports by their
    // actual relocation targets, rather than reading unrelocated call words.
    let imports = word(&rel, 0x28) as usize;
    let mut references = BTreeSet::new();
    for import in (imports..imports + word(&rel, 0x2c) as usize).step_by(8) {
        let module = word(&rel, import);
        let mut relocation = word(&rel, import + 4) as usize;
        let mut section = 0;
        let mut offset = 0usize;
        loop {
            let delta = u16::from_be_bytes(rel[relocation..relocation + 2].try_into().unwrap());
            let kind = rel[relocation + 2];
            let target = rel[relocation + 3];
            let addend = word(&rel, relocation + 4);
            relocation += 8;
            if kind == 203 {
                break;
            }
            if kind == 202 {
                section = target;
                offset = 0;
                continue;
            }
            offset += usize::from(delta);
            references.insert((module, section, offset, kind, target, addend));
        }
    }
    for reference in [
        (1, 5, 0x53ac, 1, 1, 0x5da00),      // start-animation callback at +2c
        (1, 5, 0x53b0, 1, 1, 0x5d894),      // flags-update callback at +30
        (0, 1, 0x5dbd0, 10, 2, 0x8006eb68), // same track initializer
        (0, 1, 0x51bf8, 10, 2, 0x8006d2e0), // same playback engine
    ] {
        assert!(references.contains(&reference), "missing {reference:?}");
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable and battle REL at local/extracted"]
fn original_battle_completion_copies_the_outcome_to_shared_script_word24() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let rel = fs::read(root.join("files/US_r_Top2Btl.rel")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        digest(&rel),
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(word(section * 4) as usize + (address - start) as usize))
            })
            .unwrap()
    };
    for (address, expected) in original_words::words::<u32>(
        "original_battle_completion_copies_the_outcome_to_shared_script_word24",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let rel_word = |offset: usize| u32::from_be_bytes(rel[offset..offset + 4].try_into().unwrap());
    let sections = rel_word(0x10) as usize;
    let text = (rel_word(sections + 8) & !3) as usize;
    assert_eq!(rel_word(0), 1);
    for (offset, expected) in original_words::words::<usize>(
        "original_battle_completion_copies_the_outcome_to_shared_script_word24-2",
    ) {
        assert_eq!(rel_word(text + offset), expected, "REL text+{offset:#x}");
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_dialogue_length_edits_relocate_entries_and_preserve_other_messages() {
    use sha2::{Digest, Sha256};
    use symphonia_script::message::{self, Token};
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/all-assets/fields/dirks-house-0373/scripts/scenario.ssb");
    let original = fs::read(&path).unwrap();
    let digest = Sha256::digest(&original)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        digest,
        "6ee3f5bbb0b0ef1ebf4b5f03fda23894193b1b464e4dae1c6870f16468eb41fb"
    );
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let result =
        scenario::decompile_with_context(&original, &definitions, Context::FieldScene(373))
            .unwrap();
    let before = "Whose gravestone is this?";
    let after = "Whose gravestone is this? This line was lengthened by a mod.";
    assert_eq!(result.source.matches(before).count(), 1);
    let edited = scenario::compile(
        "modded-dirks-house",
        &result.source.replace(before, after),
        &definitions,
    )
    .unwrap();
    let auxiliary = symphonia_script::scenario::analyze(&original)
        .unwrap()
        .header
        .auxiliary_offset();
    assert_eq!(original[..auxiliary], edited[..auxiliary]);
    let original_region = &original[auxiliary..];
    let edited_region = &edited[auxiliary..];
    let original_table = message::offset_table(original_region).unwrap().unwrap();
    let edited_table = message::offset_table(edited_region).unwrap().unwrap();
    assert_eq!(original_table.entry_bytes, edited_table.entry_bytes);
    assert_eq!(original_table.offsets.len(), edited_table.offsets.len());
    let originals = message::parse(original_region).unwrap();
    let edits = message::parse(edited_region).unwrap();
    let mut changed = 0;
    for (index, (mut expected, actual)) in originals.into_iter().zip(edits).enumerate() {
        let mut touched = false;
        for token in &mut expected.tokens {
            if let Token::Text { text } = token
                && text.contains(before)
            {
                *text = text.replace(before, after);
                touched = true;
            }
        }
        assert_eq!(expected, actual, "message{index}");
        if touched {
            changed += 1;
            continue;
        }
        let body = |region: &[u8], offsets: &[usize]| {
            let start = offsets[index];
            let end = offsets[index + 1..]
                .iter()
                .copied()
                .find(|&offset| offset > start)
                .unwrap_or(region.len());
            region[start..end].to_vec()
        };
        assert_eq!(
            body(original_region, &original_table.offsets),
            body(edited_region, &edited_table.offsets),
            "untouched body{index}"
        );
    }
    assert!(changed > 0);
    assert_eq!(edited.len() - original.len(), after.len() - before.len());
}

#[test]
#[ignore = "requires original .ssb inputs and generated local/readable-scripts reports"]
fn reported_message_identities_match_original_control_results_and_native_arguments() {
    use symphonia_script::{NativeCall, message, scenario::calls, semantics::NativeRegistry};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let report = fs::read_to_string(
        root.parent()
            .unwrap()
            .join("readable-scripts/message-identity-uses.tsv"),
    )
    .unwrap();
    let registry = NativeRegistry::gqseaf();
    let mut images = BTreeMap::new();
    let mut count = 0;
    for line in report.lines().skip(1) {
        let row: Vec<_> = line.split('\t').collect();
        assert_eq!(row.len(), 8);
        let address = usize::from_str_radix(row[1].trim_start_matches("0x"), 16).unwrap();
        let control: u8 = row[2].parse().unwrap();
        let value: i32 = row[6].parse().unwrap();
        let (bytes, auxiliary, ranges) = images.entry(row[0].to_owned()).or_insert_with(|| {
            let bytes = fs::read(root.join(row[0])).unwrap();
            let auxiliary = symphonia_script::scenario::analyze(&bytes)
                .unwrap()
                .header
                .auxiliary_offset();
            let region = &bytes[auxiliary..];
            let table = message::offset_table(region).unwrap().unwrap();
            let ranges = message::source_ranges(region, &table).controls;
            (bytes, auxiliary, ranges)
        });
        let span = ranges
            .iter()
            .find(|r| {
                r.opcode == control
                    && *auxiliary + r.bytes.start < address
                    && address < *auxiliary + r.bytes.end
            })
            .unwrap();
        let start = *auxiliary + span.bytes.start + 1;
        let end = *auxiliary + span.bytes.end;
        assert!((address - start).is_multiple_of(2));
        let pc = ((address - start) / 2) as u32;
        let mut wrapper = vec![0, 4, 0, 0, 0, 0, 0, 0];
        wrapper.extend_from_slice(&bytes[start..end]);
        let program = Program::decode(&wrapper).unwrap();
        if row[4] == "message::value" {
            assert_eq!(
                program.instruction(pc).unwrap().0,
                Op::Calculate(0),
                "{row:?}"
            );
            assert_eq!(
                message::Expression::from_native(&bytes[start..end]).unwrap(),
                message::Expression::Constant(value),
                "{row:?}"
            );
            let expected = match control {
                1 if value == 999999 => "scenario::ActorSelector",
                1 => "characters::CharacterId",
                4 => "items::ItemId",
                9 if matches!(value, -1 | 0) => "scenario::VoiceControl",
                9 => "resources::VoiceArchiveReference",
                17 => "characters::TitleId",
                18 => "techniques::TechniqueId",
                _ => panic!("unexpected identity control{control}"),
            };
            assert_eq!(row[3], expected);
        } else {
            let native = registry.get_by_name(row[4]).unwrap();
            assert_eq!(
                program.instruction(pc).unwrap().0,
                Op::Native(native.opcode)
            );
            let index = native
                .arguments
                .iter()
                .position(|name| name == row[5])
                .unwrap();
            let values = calls::arguments(
                &wrapper,
                NativeCall::from_opcode(native.opcode).unwrap(),
                native.arguments.len(),
            )
            .unwrap();
            assert_eq!(values[&pc][index], Some(value), "{row:?}");
        }
        count += 1;
    }
    assert_eq!(count, 38373);
}

#[test]
#[ignore = "requires the complete original .ssb corpus at local/all-assets"]
fn original_item_and_story_identity_coverage_has_only_reviewed_gaps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let mut pending = vec![root.clone()];
    let (mut scripts, mut bytes, mut item_uses, mut message_item_uses) = (0, 0, 0, 0);
    let (mut debug_flag_uses, mut invalid_property_uses) = (0, 0);
    let mut items = BTreeSet::new();
    let mut debug_flags = BTreeSet::new();
    let mut unnamed_flags = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
                continue;
            }
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "ssb") {
                continue;
            }
            let relative = path.strip_prefix(&root).unwrap();
            let input = relative.to_str().unwrap();
            let original = fs::read(&path).unwrap();
            let result = scenario::decompile_with_context(
                &original,
                &definitions,
                Context::from_path(relative),
            )
            .unwrap_or_else(|error| panic!("{input}: {error}"));
            assert_eq!(
                scenario::compile(input, &result.source, &definitions).unwrap(),
                original,
                "{input}"
            );
            scripts += 1;
            bytes += original.len();
            for identity in &result.identities {
                if identity.domain == "items::ItemId" {
                    assert!(
                        identity.symbolic && !identity.partial_mask,
                        "{input}: {identity:?}"
                    );
                    items.insert(identity.value);
                    item_uses += 1;
                }
                if identity.domain.starts_with("story::")
                    && identity.domain.ends_with("Flag")
                    && !identity.symbolic
                {
                    assert_eq!(identity.domain, "story::StoryFlag", "{input}: {identity:?}");
                    unnamed_flags.insert((input.to_owned(), identity.pc, identity.value));
                }
            }
            for identity in &result.message_identities {
                if identity.domain == "items::ItemId" {
                    assert!(
                        identity.symbolic && !identity.partial_mask,
                        "{input}: {identity:?}"
                    );
                    items.insert(identity.value);
                    message_item_uses += 1;
                }
                if identity.symbolic {
                    continue;
                }
                match identity.domain.as_str() {
                    "story::StoryFlag" => {
                        assert_eq!(input, "fields/field-debug-menu/scripts/scenario.ssb");
                        assert_eq!(identity.native, "test_event_bit");
                        assert!((0..1000).contains(&identity.value));
                        debug_flags.insert(identity.value);
                        debug_flag_uses += 1;
                    }
                    "scenario::ActorProperty" => {
                        assert!([519, 520, 521].iter().any(|field| input
                            == format!("fields/ymir-forest-{field:04}/scripts/scenario.ssb")));
                        assert_eq!(identity.native, "get_actor_property");
                        assert_eq!(identity.value, 999999);
                        invalid_property_uses += 1;
                    }
                    _ => panic!("unreviewed unnamed message identity in {input}: {identity:?}"),
                }
            }
        }
    }
    // These original instructions retain numeric operands because their
    // scene-specific set-state meanings have not been established. Equal bits
    // in other fields do not justify transferring those fields' aliases.
    let expected = [
        ("altessas-house-0138", 0x5e93, 199),
        ("altessas-house-0139", 0x72cb, 198),
        ("flanoir-0317", 0x44b3, 171),
        ("heimdall-0482", 0x4ba, 191),
        ("heimdall-0482", 0x4c4, 191),
        ("heimdall-0482", 0x4cb, 192),
        ("heimdall-0482", 0x4d5, 192),
        ("heimdall-0482", 0x4dc, 193),
        ("heimdall-0482", 0x4e6, 193),
        ("tethealla-base-0254", 0x2a99, 152),
        ("tethealla-base-0254", 0x2a9e, 153),
        ("tethealla-base-0254", 0x2ac7, 152),
        ("tethealla-base-0254", 0x2acc, 153),
        ("tower-of-salvation-0502", 0xf47, 155),
        ("tower-of-salvation-0502", 0xf79, 155),
    ]
    .map(|(field, pc, value)| (format!("fields/{field}/scripts/scenario.ssb"), pc, value))
    .into_iter()
    .collect();
    assert_eq!(unnamed_flags, expected);
    assert_eq!((scripts, bytes), (953, 17_811_493));
    assert_eq!(
        (items.len(), item_uses, message_item_uses),
        (350, 7692, 4401)
    );
    assert_eq!((debug_flags.len(), debug_flag_uses), (341, 366));
    assert_eq!(invalid_property_uses, 12);
    println!(
        "{scripts} byte-matching scenarios; 350 named item IDs; 15 unnamed scene-flag uses; \
         366 generic debug flag reads; 12 reversed Ymir property operands"
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable and scenario corpus at local"]
fn original_portrait_slot_return_and_battle_camera_eye_words_keep_their_roles() {
    use sha2::{Digest, Sha256};
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(local.join("extracted/disc1/sys/main.dol")).unwrap();
    let word = |bytes: &[u8], at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|s| {
                let start = word(&dol, 0x48 + s * 4);
                let size = word(&dol, 0x90 + s * 4);
                (address >= start && address - start < size)
                    .then(|| word(&dol, s * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    for (address, size, hash) in [
        (
            0x8004cb34,
            0x174,
            "5b6eda665d8422d4cf6d3fffe104f3d6652c7e16f6b7eb742d3713fe23f9448b",
        ),
        (
            0x800430ac,
            0x2c,
            "861805807d66dac4207d8037e0382eb09b0dc02904eabb5937f7fc5018f54305",
        ),
    ] {
        let at = offset(address);
        assert_eq!(digest(&dol[at..at + size]), hash);
    }
    for (address, expected) in original_words::words::<u32>(
        "original_portrait_slot_return_and_battle_camera_eye_words_keep_their_roles",
    ) {
        assert_eq!(word(&dol, offset(address)), expected, "{address:#x}");
    }
    let rel = fs::read(local.join("extracted/disc1/files/US_r_Top2Btl.rel")).unwrap();
    let table = word(&rel, 0x10) as usize;
    let text = (word(&rel, table + 8) & !3) as usize;
    assert_eq!(
        digest(&rel[text + 0x5cea4..text + 0x5cf9c]),
        "83987ef33d54728fe7222da53948c9504779212b42921ad6690f879eb0adfda6"
    );
    for (at, expected) in original_words::words::<usize>(
        "original_portrait_slot_return_and_battle_camera_eye_words_keep_their_roles-2",
    ) {
        assert_eq!(word(&rel, text + at), expected, "REL{at:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(registry.get(0x58).unwrap().result_type, None);
    assert_eq!(
        registry.get(0x58).unwrap().argument_types,
        ["skits::PortraitId"]
    );
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (input, names) in [
        (
            "battle/scripts/common.ssb",
            vec!["BattleCameraEyeX", "BattleCameraEyeY", "BattleCameraEyeZ"],
        ),
        (
            "skits/test-skit/scenario.ssb",
            vec!["TestSkitLloydNeutralHandle"],
        ),
    ] {
        let path = local.join("all-assets").join(input);
        let bytes = fs::read(&path).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        for name in names {
            assert!(
                result
                    .source
                    .contains(&format!("globals[story::GlobalId::{name}]"))
            );
        }
        assert_eq!(
            scenario::compile(input, &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable and scenario corpus at local"]
fn original_narrow_aliases_keep_party_backups_companion_actor_and_debug_storage_reuse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let source = |field: &str| {
        let path = root.join(format!("fields/{field}/scripts/scenario.ssb"));
        let bytes = fs::read(&path).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        assert_eq!(
            scenario::compile(field, &result.source, &definitions).unwrap(),
            bytes
        );
        result.source.split_whitespace().collect::<String>()
    };
    let has = |text: &str, expected: &str| {
        text.contains(&expected.split_whitespace().collect::<String>())
    };
    let debug = source("field-debug-menu");
    assert!(has(
        &debug,
        "memory::i8[story::ByteOffset::SavedFormation4] = native::get_actor_property(actor_id: scenario::ActorSelector::Controlled, property: scenario::ActorProperty::TargetHeading)"
    ));
    assert!(has(
        &debug,
        "globals[story::GlobalId::TowerOfManaLightPuzzle0X] = native::get_current_field()"
    ));
    assert!(has(
        &debug,
        "yaw: globals[story::GlobalId::TowerOfManaLightPuzzle2X]"
    ));
    let beach = source("altamira-0016");
    for slot in 0..8 {
        assert!(has(
            &beach,
            &format!(
                "memory::i8[story::ByteOffset::SavedFormation{slot}] = native::find_party_member(character_or_slot: encoded(scenario::FormationSlotSelector::Slot{slot}, (-65536 | {slot})))"
            )
        ));
    }
    assert!(has(
        &beach,
        "memory::i8[story::ByteOffset::SavedFieldLeader] = native::field_system(command: fields::FieldSystemCommand::FieldLeader, argument: 0)"
    ));
    let hima = source("hima-0300");
    assert!(has(
        &hima,
        "(memory::i8[story::ByteOffset::SecondarySavedFormationBase])[memory::i32[0x05FC]] = native::find_party_member(character_or_slot: (-65536 | memory::i32[0x05FC]))"
    ));
    assert!(has(
        &hima,
        "native::add_party_member(input_value: (memory::i8[story::ByteOffset::SecondarySavedFormationBase])[memory::i32[0x05FC]])"
    ));
    assert!(has(
        &hima,
        "(memory::i8[story::ByteOffset::SecondarySavedFormationBase])[memory::i32[0x05FC]] = 0"
    ));
    let heimdall = source("heimdall-0284");
    for actor in 2..=8 {
        assert!(has(
            &heimdall,
            &format!("memory::i16[story::HalfwordOffset::HeimdallCompanionActor] = {actor}")
        ));
    }
    for axis in ["X", "Y", "Z"] {
        assert!(has(
            &heimdall,
            &format!(
                "native::get_actor_property(actor_id: memory::i16[story::HalfwordOffset::HeimdallCompanionActor], property: scenario::ActorProperty::Position{axis})"
            )
        ));
    }
    let katz = source("altamira-0017");
    assert!(has(
        &katz,
        "(memory::i8[story::ByteOffset::KatzExpeditionResultBase])[memory::i32[0x0828]] |= 128"
    ));
}

#[test]
#[ignore = "requires the original GQSEAF executable and scenario corpus at local"]
fn original_colette_wing_request_controls_automatic_accessory_and_retirement() {
    use sha2::{Digest, Sha256};
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(local.join("extracted/disc1/sys/main.dol")).unwrap();
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    for (address, size, expected) in [
        (
            0x8003f988,
            0xc4,
            "a711a85fd64db17094fe5255fbd8304f40f71026b3cea7e3ce4efb1a2f80f858",
        ),
        (
            0x8001cb0c,
            0x168,
            "10b24d95fd1c1693b2fbcfce65877b9e7ce1f7031197588e9dac892fc6c45fce",
        ),
        (
            0x800191f0,
            0x8cc,
            "16a93678ae75fecfc862fb98ee217c4e4baf73c99d360b85f47af93fda926034",
        ),
    ] {
        let start = offset(address);
        let digest = Sha256::digest(&dol[start..start + size])
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(digest, expected, "{address:#x}");
    }
    for (address, expected) in original_words::words::<u32>(
        "original_colette_wing_request_controls_automatic_accessory_and_retirement",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let mut references = 0;
    for field in [
        "altessas-house-0138",
        "flying-dragon-0137",
        "fooji-mountains-0416",
        "great-seed-awakening",
        "luin-0448",
        "meltokio-0376",
        "palmacosta-0065",
        "palmacosta-ranch-0202",
        "tethealla-bridge-0075",
    ] {
        let path = local.join(format!("all-assets/fields/{field}/scripts/scenario.ssb"));
        let bytes = fs::read(&path).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        references += result
            .source
            .matches("globals[story::GlobalId::ColetteWingsRequested]")
            .count();
        assert!(!result.source.contains("memory::i32[0x0044]"));
        assert_eq!(
            scenario::compile(field, &result.source, &definitions).unwrap(),
            bytes
        );
    }
    assert_eq!(references, 21);
}

#[test]
#[ignore = "requires the original GQSEAF executable and scenario corpus at local"]
fn original_costume_and_record_storage_preserves_native_queries_and_comparison_direction() {
    use sha2::{Digest, Sha256};

    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(local.join("extracted/disc1/sys/main.dol")).unwrap();
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    let start = offset(0x80054470);
    assert_eq!(
        Sha256::digest(&dol[start..start + 0x120])
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "cbdbe284916582247796992698ab0797a058cb9cd27bc2cdc3b6e41804b28ecb"
    );
    for (address, expected) in original_words::words::<u32>(
        "original_costume_and_record_storage_preserves_native_queries_and_comparison_direction",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let source = |input: &str| {
        let path = local.join("all-assets").join(input);
        let bytes = fs::read(&path).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        assert_eq!(
            scenario::compile(input, &result.source, &definitions).unwrap(),
            bytes
        );
        result.source
    };
    let beach_save = source("fields/altamira-0016/scripts/scenario.ssb");
    let beach_restore = source("fields/altamira-0014/scripts/scenario.ssb");
    let banquet = source("fields/meltokio-0399/scripts/scenario.ssb");
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    for slot in 0..8 {
        let save = format!(
            "SavedCostume{slot}] = native::set_character_costume(character: memory::i8[story::ByteOffset::SavedFormation{slot}], costume_or_minus_one: -1)"
        );
        let restore = format!(
            "native::set_character_costume(character: memory::i8[story::ByteOffset::SavedFormation{slot}], costume_or_minus_one: globals[story::GlobalId::SavedCostume{slot}])"
        );
        assert!(beach_save.contains(&save), "{save}");
        assert!(
            compact(&beach_restore).contains(&compact(&restore)),
            "{restore}"
        );
        assert!(banquet.contains(&save) && compact(&banquet).contains(&compact(&restore)));
    }
    assert!(beach_save.contains(
        "SavedLloydEquippedTitle] = native::get_title(character: characters::CharacterId::Lloyd, packed_title: scenario::TitleLookup::Equipped)"
    ));
    assert!(compact(&beach_restore).contains(&compact(
        "native::set_equipped_title(character_id: characters::CharacterId::Lloyd, title_id: globals[story::GlobalId::SavedLloydEquippedTitle])"
    )));
    let sea = source("fields/tethealla-bridge-0077/scripts/scenario.ssb");
    for expected in [
        "SeaKatzHighestClearedLevel] < memory::i32[0x0960]",
        "(globals[story::GlobalId::SeaKatzBestTimeBase])[memory::i32[0x0960]] > memory::i32[0x0964]",
        "(globals[story::GlobalId::SeaKatzBestTimeBase])[memory::i32[0x0960]] == 0",
        "(globals[story::GlobalId::SeaKatzBestTimeBase])[memory::i32[0x0960]] = memory::i32[0x0964]",
        "native::yield_command(command: scenario::WaitCondition::Ticks, value: 1)",
    ] {
        assert!(sea.contains(expected), "{expected}");
    }
    let uncle = source("fields/triet-0485/scripts/scenario.ssb");
    for expected in [
        "UncleGameBestClearedStage] >= 5",
        "UncleGameBestClearedStage] <= memory::i32[0x0AAC]",
        "UncleGameBestClearedStage] = memory::i32[0x0AAC]",
        "UncleGameCorrectSelections] = (globals[story::GlobalId::UncleGameCorrectSelections] + 1)",
        "UncleGameCorrectSelections] >= 100",
        "UncleGameBestClearedStage] >= 10",
        "memory::i32[0x0AAC] = (memory::i32[0x0AAC] + 5)",
    ] {
        assert!(uncle.contains(expected), "{expected}");
    }
    let balloon = source("fields/izoold-0354/scripts/scenario.ssb");
    assert!(balloon.contains(
        "(globals[story::GlobalId::EmotionalBalloonBestRoundsBase])[(memory::i32[0x081C] - 4)] < (memory::i32[0x09F8])[0]"
    ));
    assert!(balloon.contains(
        "(globals[story::GlobalId::EmotionalBalloonBestRoundsBase])[(memory::i32[0x081C] - 4)] = (memory::i32[0x09F8])[0]"
    ));
    let cafeteria = source("fields/palmacosta-0070/scripts/scenario.ssb");
    assert!(cafeteria.contains("PalmacostaCafeteriaBestGaldReward] < memory::i32[0x0C64]"));
    assert!(cafeteria.contains("native::add_gald(amount: memory::i32[0x0C64])"));
    let orienteering = source("fields/palmacosta-0056/scripts/scenario.ssb");
    assert!(orienteering.contains("PalmacostaOrienteeringBestTimeTicks] > memory::i32[0x08D4]"));
    assert!(orienteering.contains("memory::i32[0x08D4] = native::get_field_ticks()"));
    let test = source("skits/test-skit/scenario.ssb");
    assert!(test.contains(
        "UncleGameBestClearedStage] = native::load_portrait(resource_id: skits::PortraitId::LloydLaughing)"
    ));
    assert!(test.contains("EmotionalBalloonBestRoundsBase] = 1"));
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_named_storage_preserves_counts_coordinates_masks_and_indexed_status() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let source = |field: &str| {
        let path = root.join(format!("fields/{field}/scripts/scenario.ssb"));
        let bytes = fs::read(&path).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::from_path(&path))
                .unwrap();
        assert_eq!(
            scenario::compile(field, &result.source, &definitions).unwrap(),
            bytes
        );
        result.source
    };
    let volt = source("temple-of-lightning-0475");
    assert!(volt.contains("globals[story::GlobalId::SeveredManaLinkCount] = 1"));
    for field in [
        "balacruf-mausoleum-0510",
        "temple-of-earth-0237",
        "temple-of-ice-0325",
        "triet-ruins-0221",
    ] {
        let text = source(field);
        assert!(text.contains(
            "globals[story::GlobalId::SeveredManaLinkCount] = (globals[story::GlobalId::SeveredManaLinkCount] + 1)"
        ));
    }
    let linkite = source("linkite-tree-0523");
    assert!(linkite.contains(
        "(globals[story::GlobalId::SeveredManaLinkCount] == 3) && (native::test_event_bit(bit: story::StoryFlag::ShadowPactFormed) == 1)"
    ));
    let tower = source("tower-of-mana-0366");
    for index in 0..4 {
        for axis in ["X", "Y"] {
            assert!(tower.contains(&format!(
                "globals[story::GlobalId::TowerOfManaLightPuzzle{index}{axis}] = native::get_actor_property(actor_id: {}, property: scenario::ActorProperty::Position{axis})",
                480 + index
            )));
        }
    }
    let palmacosta = source("palmacosta-0054");
    for expected in [
        "4 <= memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCheckpointCount]",
        "memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCurrentCheckpoint] = 15",
        "5 == memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCurrentCheckpoint]",
        "memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCurrentCheckpoint] = 14",
        "(0 == (globals[story::GlobalId::PalmacostaOrienteeringCheckpointMask] & (1 << memory::i32[0x0828])))",
        "globals[story::GlobalId::PalmacostaOrienteeringCheckpointMask] |= (1 << memory::i32[0x0828])",
        "(memory::i16[story::HalfwordOffset::PalmacostaOrienteeringCheckpointCount])++",
    ] {
        assert!(palmacosta.contains(expected), "{expected}");
    }
    let restart = source("palmacosta-0056");
    assert!(restart.contains("globals[story::GlobalId::PalmacostaOrienteeringCheckpointMask] = 0"));
    let katz = source("altamira-0017");
    for expected in [
        "(globals[story::GlobalId::KatzExpeditionStatusBase])[memory::i32[0x0828]] = s32(36000)",
        "(globals[story::GlobalId::KatzExpeditionStatusBase])[memory::i32[0x0828]] = 0",
        "(globals[story::GlobalId::KatzExpeditionStatusBase])[memory::i32[0x0828]] > 1",
        "(memory::i8[story::ByteOffset::KatzExpeditionResultBase])[memory::i32[0x0828]] = memory::i32[0x086C]",
    ] {
        assert!(katz.contains(expected), "{expected}");
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_latheon_flag_requests_name_exact_assignments_without_typing_shared_cells() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (input, hash, expected, flower, introduction) in [
        (
            "fields/latheon-gorge-0492/scripts/scenario.ssb",
            "67d64dae4ff584b9d2097ce78ebb1adea42d7de242a5907b28eeed40e4c5ad75",
            &[
                (0x41C7, 0xA18, 430),
                (0x41DA, 0xA18, 431),
                (0x41EC, 0xB6C, 412),
                (0x4200, 0xA18, 432),
                (0x4213, 0xA18, 433),
                (0x4226, 0xA18, 434),
                (0x4239, 0xA18, 435),
                (0x424C, 0xA18, 436),
                (0x425F, 0xA18, 437),
                (0x4272, 0xA18, 438),
                (0x4285, 0xA18, 439),
                (0x4298, 0xA18, 440),
                (0x42AB, 0xA18, 441),
                (0x42B4, 0xB6C, 410),
                (0x42C4, 0xB6C, 411),
            ][..],
            0xA18,
            0xB6C,
        ),
        (
            "fields/latheon-gorge-0493/scripts/scenario.ssb",
            "3888639119c28db62595c741b96b653505954ca1effa17a7484618bea8a81ea7",
            &[
                (0xF07, 0x8C0, 445),
                (0xF1A, 0x8C0, 446),
                (0xF2D, 0x8C0, 447),
                (0xFB0, 0x924, 413),
            ][..],
            0x8C0,
            0x924,
        ),
        (
            "fields/latheon-gorge-0494/scripts/scenario.ssb",
            "62d8afe0636d1adbf96a880ad4de543190dc102cbbfd7f001d30bbbf7e841d9c",
            &[
                (0x12B9, 0x9A8, 449),
                (0x12CC, 0x9A8, 450),
                (0x12DF, 0x9A8, 451),
                (0x12F2, 0x9A8, 452),
                (0x1305, 0x9A8, 453),
                (0x1318, 0x9A8, 454),
                (0x132B, 0x9A8, 455),
                (0x133E, 0x9A8, 456),
            ][..],
            0x9A8,
            0xAAC,
        ),
        (
            "fields/latheon-gorge-0495/scripts/scenario.ssb",
            "804dc4040950c89d1f0925fa777b6cffea4df34b089e1eb82b5e6e2b1bcffecd",
            &[
                (0x1D97, 0xA50, 464),
                (0x1DAA, 0xA50, 465),
                (0x1DBD, 0xA50, 466),
                (0x1DD0, 0xA50, 467),
                (0x1DE3, 0xA50, 468),
                (0x1DF6, 0xA50, 469),
                (0x1E09, 0xA50, 470),
                (0x1E1C, 0xA50, 471),
                (0x1E2F, 0xA50, 472),
                (0x1E42, 0xA50, 473),
                (0x1E55, 0xA50, 474),
                (0x1E68, 0xA50, 475),
                (0x1E7B, 0xA50, 476),
                (0x1E8E, 0xA50, 477),
            ][..],
            0xA50,
            0xBCC,
        ),
        (
            "fields/latheon-gorge-0496/scripts/scenario.ssb",
            "cdd5d1c266bb2c4cce83e0f7808847f90179c8c1da4f050d326037239a66dcad",
            &[(0x425C, 0x898, 465)][..],
            0x898,
            0x8C0,
        ),
        (
            "fields/latheon-gorge-0497/scripts/scenario.ssb",
            "4b98dd0f4a8ae99847a2c9467a816f19b6fccd92a2297d97b17e37c206e08d49",
            &[
                (0x31A1, 0x970, 481),
                (0x31B4, 0x970, 482),
                (0x31C7, 0x970, 483),
                (0x31DA, 0x970, 484),
            ][..],
            0x970,
            0xA38,
        ),
        (
            "fields/latheon-gorge-0498/scripts/scenario.ssb",
            "23a300df8bf19ac5b23d93cd7c9573cdd060154d3391bf6d48f9c7dd684ef065",
            &[
                (0x827, 0xA18, 460),
                (0x83A, 0xA18, 461),
                (0x84D, 0xA18, 462),
                (0x860, 0xA18, 463),
            ][..],
            0xA18,
            0xA7C,
        ),
    ] {
        let bytes = fs::read(root.join(input)).unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(digest, hash);
        let result = scenario::decompile_with_context(
            &bytes,
            &definitions,
            Context::from_path(Path::new(input)),
        )
        .unwrap();
        let uses: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.native == "memory::flag_constant32")
            .collect();
        assert_eq!(uses.len(), expected.len(), "{input}");
        let compact = result.source.split_whitespace().collect::<String>();
        for &(pc, address, value) in expected {
            let (load, diag) = symphonia_script::scenario::decode_at(&bytes, pc).unwrap();
            assert!(diag.is_empty() && !load.raw);
            assert_eq!(load.mnemonic, "load.s32");
            assert_eq!(load.operands, [address]);
            let (literal, diag) =
                symphonia_script::scenario::decode_at(&bytes, load.end_pc()).unwrap();
            assert!(diag.is_empty() && !literal.raw && literal.mnemonic.starts_with("push."));
            assert_eq!(literal.operands, [i64::from(value)]);
            let (calc, diag) =
                symphonia_script::scenario::decode_at(&bytes, literal.end_pc()).unwrap();
            assert!(diag.is_empty() && !calc.raw);
            assert_eq!(calc.mnemonic, "calc");
            assert_eq!(calc.operands, [0x10]);
            assert!(uses.iter().any(|id| id.pc == pc
                && id.parameter == format!("{address:#x}/0x10")
                && id.domain == "story::StoryFlag"
                && id.value == value
                && id.symbolic));
            let flag = symphonia_constants::StoryFlag::try_from(value as u16).unwrap();
            assert!(
                compact.contains(&format!(
                    "memory::i32[0x{address:04X}]=story::StoryFlag::{}",
                    flag.name()
                )),
                "{input}:{}",
                flag.name()
            );
        }
        // The helper's parameter is still a shared-memory read at all native
        // uses; do not specialize a callee to just one caller's flower.
        for op in ["test_event_bit", "set_event_bit", "clear_event_bit"] {
            assert!(
                compact.contains(&format!("{op}(bit:memory::i32[0x{flower:04X}])")),
                "{input} {op}"
            );
        }
        assert!(compact.contains(&format!(
            "set_event_bit(bit:memory::i32[0x{introduction:04X}])"
        )));
        assert!(compact.contains(&format!("memory::i32[0x{introduction:04X}]=0")));
        assert!(compact.contains("items::ItemId::Kirima"));
        assert!(compact.contains("items::ItemId::Amango"));
        if input.contains("0496/") {
            // This room's flower reuses 0495 actor3001's state, not a made-up
            // room0496 save bit. Keep the documented original shared identity.
            assert_eq!(expected.len(), 1);
            assert_eq!(expected[0].2, 465);
            assert!(compact.contains("LatheonGorge0495Flower3001Blowing"));
        }
        let formatted =
            symphonia_script_compiler::format("latheon-requests", &result.source).unwrap();
        assert_eq!(
            scenario::compile("latheon-requests", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_palmacosta_global_aliases_keep_warp_state_counter_and_later_quest_uses_separate() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets/fields");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, hash, expected) in [
        (
            205,
            "76c7971a9a406f35cbcdda6fbb2df06564b3c9260300160b66de08679dc7f69c",
            &[(0xD36, 0x12C), (0xE84, 0x12C)][..],
        ),
        (
            206,
            "34e94549b6852e5f2816bc9a5256a88f75df4fce8f481917b83619b66037c573",
            &[
                (0xA25, 0x12C),
                (0xABC, 0x12C),
                (0xB21, 0x12C),
                (0xB27, 0x12C),
                (0xB2C, 0xE8),
                (0xC1A, 0x12C),
                (0xCE2, 0x12C),
                (0xD94, 0x12C),
                (0xD9A, 0xE8),
                (0x34AD, 0x12C),
                (0x5DCD, 0xE8),
                (0x5DD6, 0xE8),
            ][..],
        ),
    ] {
        let bytes =
            fs::read(root.join(format!("palmacosta-ranch-{field:04}/scripts/scenario.ssb")))
                .unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(digest, hash);
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        let aliases: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.native == "memory::global_alias")
            .collect();
        assert_eq!(aliases.len(), expected.len());
        for &(pc, address) in expected {
            assert!(is_reviewed_memory_load(&bytes, pc, address, "load.s32"));
            assert!(aliases.iter().any(|id| id.pc == pc
                && id.value == (address - 0x40) / 4
                && id.parameter == format!("{address:#x}")
                && id.domain == "story::PalmacostaGlobalId"
                && id.symbolic));
            assert!(
                !result
                    .identities
                    .iter()
                    .any(|id| id.pc == pc && id.native == "memory::constant32")
            );
        }
        let compact = result.source.split_whitespace().collect::<String>();
        if field == 205 {
            assert_eq!(
                compact
                    .matches("globals[story::PalmacostaGlobalId::WarpRoomState]=7")
                    .count(),
                2
            );
            assert!(compact.contains("story::StoryFlag::PalmacostaRanchPurpleCardObtained"));
            assert!(compact.contains("story::StoryFlag::PalmacostaRanchRedCardObtained"));
            assert!(compact.contains("story::StoryFlag::PalmacostaRanchBlueCardObtained"));
            assert!(compact.contains("field_id:fields::FieldId::PalmacostaRanch0206"));
        } else {
            assert_eq!(
                compact.matches("PalmacostaGlobalId::WarpRoomState").count(),
                8
            );
            assert_eq!(
                compact
                    .matches("PalmacostaGlobalId::WarpBattleCount")
                    .count(),
                4
            );
            let setup = compact
                .split("pubtasksub_34AB(){")
                .nth(1)
                .unwrap()
                .split("pubtask")
                .next()
                .unwrap();
            assert!(
                setup.contains(
                    "memory::i16[0x0818]=globals[story::PalmacostaGlobalId::WarpRoomState]"
                )
            );
            for room in 0..=8 {
                assert!(setup.contains(&format!("memory::i32[0x0000]=={room}")));
            }
            let battle = compact
                .split("pubtaskactor_8000(){")
                .nth(1)
                .unwrap()
                .split("pubtask")
                .next()
                .unwrap();
            assert!(battle.contains(
                "gotoL_5DCD;}L_5DCD:eval(globals[story::PalmacostaGlobalId::WarpBattleCount])++;"
            ));
            assert!(battle.contains("globals[story::PalmacostaGlobalId::WarpBattleCount]==20"));
            assert!(
                battle.contains("story::PalmacostaRanchAssaultStage::ChocolatRescuedAndGuiding")
            );
            assert!(battle.contains("story::PalmacostaRanchDorrStage::ChocolatRescuedAndGuiding"));
            assert!(!battle.contains("CelsiusPactStage"));
            assert!(compact.contains("globals[story::GlobalId::CelsiusPactRoute]==story::CelsiusPactStage::TempleEntryHandled"));
            assert!(compact.contains("globals[story::GlobalId::ManaLeafHerbRoute]==story::ManaLeafHerbStage::ManaLeafHerbObtained"));
        }
        let formatted =
            symphonia_script_compiler::format("palmacosta-roles", &result.source).unwrap();
        assert_eq!(
            scenario::compile("palmacosta-roles", &formatted, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_dog_and_map_flag_banks_preserve_counts_awards_and_partial_map_loops() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets/fields");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let dogs: Vec<_> = symphonia_constants::StoryFlag::ALL
        .into_iter()
        .filter(|flag| flag.name().starts_with("Dog") && flag.name().ends_with("Named"))
        .map(|flag| flag.raw())
        .collect();
    assert_eq!(dogs, (1214..1244).collect::<Vec<_>>());
    for (field, hash, dogs, maps) in [
        (
            "altamira-0011",
            "4c0516373c8fc1ac0aad9f83876d10377ef7e1dc750c7a8bff5cc5de118196d1",
            1,
            false,
        ),
        (
            "altamira-0013",
            "f383ee061c35443906f1b65b71a182953c4315ac5e7ba42e9716be8bf0b59e13",
            1,
            false,
        ),
        (
            "altamira-0017",
            "6192545b2d58e38d09ad5208e798d78ac626d516641154235e10e98d70cebe9c",
            0,
            true,
        ),
        (
            "asgard-0029",
            "f0c3afaf6ec80546d7316907bcc58c38e75e163cf5e6838a7fe25ac29c6b2cab",
            1,
            true,
        ),
        (
            "asgard-0030",
            "a9a03a247aa31744d73c352667f7882f6d7b15b84405b37590700ee2c1779fc2",
            1,
            false,
        ),
        (
            "exire-0187",
            "5ad055dd74b7badd062abe916f428b87f5525d1f75a0d9ed81c8b0594b9754f2",
            0,
            true,
        ),
        (
            "exire-0189",
            "9c982b6d6af28a71dd4426b8cf478f2f6be1edf808572a821e0a05afbfc9f6f7",
            1,
            false,
        ),
        (
            "exire-0190",
            "29314901065a6622953449d28446dd139ce89c61a33664ba05368f65b1e2ebc7",
            1,
            false,
        ),
        (
            "flanoir-0310",
            "fbb7754732de701c288943ddb01241960c4551f4b47be108d2c3ecd2ad305ced",
            1,
            false,
        ),
        (
            "flanoir-0312",
            "365b663ab12415a0e7b78dede2b8d11475354a7751d4b3a2fd8d9c4b823f5b87",
            1,
            false,
        ),
        (
            "flanoir-0319",
            "abd8a6d7f427c7ddf0e45e47891f5478cace0ac11a8dddbc8d55a7198809429d",
            0,
            true,
        ),
        (
            "heimdall-0282",
            "d71c5dfef1d332f60cfcebb014cee4a4605ae8d6ec0c49d19dfc8cb658706b3d",
            1,
            false,
        ),
        (
            "heimdall-0283",
            "525f1d78c86c7c14b0c51c07ca538bd268545915e89b6c1e53dff5a33c0e120d",
            1,
            false,
        ),
        (
            "heimdall-0287",
            "30d65d4e94300d45223e052a118a2969f3971eabe39ae5c868056cbd02e0db01",
            0,
            true,
        ),
        (
            "heimdall-0291",
            "98792e77d17cedaf5c7c422600806ef421179b44765bfe38b96f4bfa5f1f3f9c",
            0,
            true,
        ),
        (
            "hima-0300",
            "22f4a061a29ad74d9cc4bc60d1bdc027c3aee21d1e1b972c7f747f0ad094e47c",
            1,
            true,
        ),
        (
            "house-of-salvation-0081",
            "68e1728a0222d3f80f868b9312e3b7619a4d70a76743e0b0f37a55ee68012b98",
            1,
            false,
        ),
        (
            "house-of-salvation-0083",
            "a76540132acfb9fe2c38fb5c67c11c484cf1b53f7cbc373126b242b0c8023f13",
            1,
            false,
        ),
        (
            "house-of-salvation-0084",
            "de9993ef69d47eef5acf174187189fdb122cf334e8ef7f68d16d6760048e6449",
            1,
            false,
        ),
        (
            "iselia-0330",
            "d3b6410372fc438488ba870da248d88e80e1fb5983446b289618cc8fe69e9dd2",
            1,
            false,
        ),
        (
            "iselia-0344",
            "e5c4ec73c8650166980f4a748c175fdf301ba9873a1e9fb0fbb38da1d3303722",
            1,
            false,
        ),
        (
            "izoold-0353",
            "e41180b4d7d8ed32afe28162adf223e47c9650f701738754dc001cc390b39925",
            1,
            true,
        ),
        (
            "katz-village-0419",
            "29628c7eb38ce5095eff1a0f96bceb1eb7a99024584b57f7e363dba62f772ebf",
            0,
            true,
        ),
        (
            "luin-0447",
            "148092b9ee9059130d6af95a947ba85ee891a8e37977fd359f319b9bda8c3270",
            1,
            false,
        ),
        (
            "luin-0449",
            "adcc495e37f6c03c0818f55e608fbb8e1eecfc71c6a65630941cc45dd5026a2b",
            1,
            false,
        ),
        (
            "luin-0452",
            "e04599f2ac02b0d7ea361a516825d6c375f68b8a75f22411da2a878ec7c9bcd0",
            1,
            true,
        ),
        (
            "luin-0455",
            "490b59b4a3b0f1030e670fe27c3675dd78a19855d8c0d0ed954ca242c3bb48b7",
            1,
            true,
        ),
        (
            "meltokio-0378",
            "a41e59d8f941fab34f10e39cfbbb86c38282b7bded1cf7ca1eba2ed362d311a1",
            1,
            false,
        ),
        (
            "meltokio-0379",
            "0aba575c281772dff592f9997fcce43fbe66b4417b4b3c3b1bfbd2bed9289823",
            1,
            false,
        ),
        (
            "meltokio-0389",
            "7ca4f276d25604e34affbc99efe6f1d85d5f454a674725b444af256edbdf43d3",
            0,
            true,
        ),
        (
            "mizuho-0326",
            "3b56b1f22e2d8548b5c14add7ac18bf961e87cbdb2242a52a57cf43033de9060",
            1,
            true,
        ),
        (
            "ozette-0425",
            "f76908b41360effd36cd98b87f649c7da995b563602792d4382f6c40ee7607b0",
            1,
            false,
        ),
        (
            "palmacosta-0054",
            "85249be6d67a947d7c1fe3713d1b207ddf794a2e30542eb5797129e4ea6391c7",
            0,
            true,
        ),
        (
            "palmacosta-0055",
            "db23d0e601f3e3588b9ae0b7962d4787df547e3e2bdced53c7aef5d9e5b0c128",
            1,
            false,
        ),
        (
            "palmacosta-0056",
            "372ebee68a6cb8d8a426b6c819aad3afde8699883c688182ddaf074eecf45b73",
            1,
            false,
        ),
        (
            "sybak-0434",
            "9a6a09989ad8443fd0625c9f0fb7d9d99afb70d1ac4e1b0abc916a9498d6be51",
            1,
            true,
        ),
        (
            "sybak-0435",
            "63d75af9bfe0ae7895775114bf28493086bb86122122f9b1c3f63ab35c053a89",
            1,
            false,
        ),
        (
            "thoda-dock-0082",
            "c6ca822421ffc9b65d8c8f93300139bb555844b34557d50582f34139a6587547",
            1,
            false,
        ),
        (
            "thoda-geyser-0006",
            "218f51b9ac4b0136139f15cbbeba2707e718213595cd9b32ffd1a15ead9d7800",
            2,
            false,
        ),
        (
            "triet-0485",
            "6d21b7b493195e22cceb41b15bf51ea11c8321550ae2931779c450e990a8f636",
            0,
            true,
        ),
        (
            "triet-0486",
            "c4a6860185dc67d1824517b075599d4921de2c2991523691a86eb1c0313faa3b",
            1,
            false,
        ),
    ] {
        let bytes = fs::read(root.join(field).join("scripts/scenario.ssb")).unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(digest, hash, "{field}");
        let number = field.rsplit('-').next().unwrap().parse::<u16>().unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(number))
                .unwrap();
        let bank_uses = |value| {
            result
                .identities
                .iter()
                .filter(|id| id.domain == "story::StoryFlagBase" && id.value == value)
                .collect::<Vec<_>>()
        };
        let dog_uses = bank_uses(1214);
        assert_eq!(dog_uses.len(), dogs, "{field}");
        assert_eq!(bank_uses(900).len(), usize::from(maps), "{field}");
        assert_eq!(bank_uses(911).len(), usize::from(maps), "{field}");
        assert!(dog_uses.iter().all(|id| id.symbolic
            && (id.native == "indexed::test_event_bit"
                || (number == 6 && id.native == "indexed::set_event_bit"))));
        let compact = result.source.split_whitespace().collect::<String>();
        if dogs > 0 {
            for expected in [
                "memory::i32[0x05FC]=0",
                "memory::i32[0x05FC]<30",
                "+=native::test_event_bit(bit:(story::StoryFlagBase::DogNamingBank+memory::i32[0x05FC]))",
                "==30",
                "get_title(character:characters::CharacterId::Colette,packed_title:characters::TitleId::ColetteDogLover)==0",
                "learn_title(packed_title:characters::TitleId::ColetteDogLover)",
            ] {
                assert!(compact.contains(expected), "{field}: {expected}");
            }
            assert_eq!(
                dog_uses
                    .iter()
                    .filter(|id| id.native == "indexed::test_event_bit")
                    .count(),
                1,
                "{field}"
            );
        }
        if number == 6 {
            assert_eq!(dogs, 2);
            assert!(compact.contains(
                "globals[story::GlobalId::Story]==story::Field0006StoryStage::DogFlagsDebugMenu"
            ));
            assert!(compact.contains(
                "set_event_bit(bit:(story::StoryFlagBase::DogNamingBank+memory::i32[0x05FC]))"
            ));
        }
        if maps {
            for (base, bound, choice) in [
                ("LongRangeSylvarantBank", 11, 15),
                ("LongRangeTetheallaBank", 10, 16),
            ] {
                assert!(
                    compact.contains(&format!("memory::i32[0x05FC]<{bound}")),
                    "{field}"
                );
                assert!(
                    compact.contains(&format!("memory::i32[0x0000]=={choice}")),
                    "{field}"
                );
                assert!(
                    compact.contains(&format!(
                        "set_event_bit(bit:(story::StoryFlagBase::{base}+memory::i32[0x05FC]))"
                    )),
                    "{field}"
                );
            }
            // The second loop is deliberately ten entries, not all flags in
            // the native Tethe'alla region/stone catalogue (921/922 stay out).
        }
        let formatted = symphonia_script_compiler::format("dog-map-banks", &result.source).unwrap();
        assert_eq!(
            scenario::compile("dog-map-banks", &formatted, &definitions).unwrap(),
            bytes,
            "{field}"
        );
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_party_assignment_banks_preserve_team_slot_and_leader_encodings() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets/fields");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, hash, count) in [
        (
            "asgard-ranch-0211",
            "a694aced2a697e0f29e3091cf6e12cbc7e5547556de3db534f2b9655d6950ff6",
            55,
        ),
        (
            "asgard-ranch-0214",
            "532b1116f536360b519caa7d9e25d585eb7b8d3a57eb0acc4964d114fe0f3c4a",
            20,
        ),
        (
            "asgard-ranch-0215",
            "e941afda5b9ca95045b594b1953e92bac1c985e1b92e41a205515b49f53969cb",
            14,
        ),
        (
            "asgard-ranch-0216",
            "d571edabd89aeb613c62b7d3b38cd3c01f9c3679c671be9991e204f89626df98",
            14,
        ),
        (
            "asgard-ranch-0217",
            "5796cb483264d566b20660d050fb3faea0dc7292916eb8e21d6eaffb983e4b7d",
            14,
        ),
        (
            "iselia-human-ranch-0193",
            "558aa7d8f672649e7881d0de369dd8c0ceb5714bbbf3ac5343265c3ed330cc91",
            17,
        ),
        (
            "iselia-human-ranch-0194",
            "0f631d00b08cbb1584de163b4b08fcaa29ae9dd13f97fd690c630b1760777139",
            41,
        ),
        (
            "iselia-human-ranch-0195",
            "d5abfd441928842d1b430acb7d98a201d92444aa8e92bc67382fca442e9e7901",
            17,
        ),
        (
            "iselia-human-ranch-0197",
            "de5793716b9d6f9ec818ec23c6476b772032ad5346d5ccc3d1bce9d9239d8961",
            17,
        ),
        (
            "sylvarant-base-0210",
            "6daf4cb175a54cf612f7f810cf5edfa2a2d115c544a96c2145488ad945fdb3db",
            14,
        ),
        (
            "tower-of-mana-0361",
            "38e35571d85941c5f9b08ee4ef2907a0e9493cc7b5e99731a11e5c390f394cf4",
            16,
        ),
        (
            "tower-of-mana-0362",
            "181b9c2ebb77863f219b509aca1037af12838bef76e643bdae261c25ac02f4c3",
            55,
        ),
        (
            "tower-of-mana-0366",
            "11bafcc00de057a8dfd7943bd1947357ab71b866e8c0a717ac4271d1396b1711",
            20,
        ),
    ] {
        let number = field.rsplit('-').next().unwrap().parse::<u16>().unwrap();
        let bytes = fs::read(root.join(field).join("scripts/scenario.ssb")).unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(digest, hash, "{field}");
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(number))
                .unwrap();
        let domain = format!("story::Field{number:04}FlagBase");
        let uses: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.domain == domain && id.value == 150)
            .collect();
        assert_eq!(uses.len(), count, "{field}");
        assert!(
            uses.iter()
                .all(|id| id.symbolic && id.native.starts_with("indexed::"))
        );
        let base = format!("{domain}::PartyAssignmentBank");
        assert_eq!(result.source.matches(&base).count(), count, "{field}");
        let compact = result.source.split_whitespace().collect::<String>();
        if number == 194 {
            // Reactor/rescue encodes all four slots, including slot zero.
            for (array, team_set) in [("0x0880", false), ("0x0888", true)] {
                for slot in 0..4 {
                    let record = format!("({base}+((memory::i16[{array}])[{slot}]*3))");
                    for (offset, set) in [(0, team_set), (1, slot & 2 != 0), (2, slot & 1 != 0)] {
                        let argument = if offset == 0 {
                            record.clone()
                        } else {
                            format!("({record}+{offset})")
                        };
                        let op = if set {
                            "set_event_bit"
                        } else {
                            "clear_event_bit"
                        };
                        assert!(
                            compact.contains(&format!("native::{op}(bit:{argument});")),
                            "{field} {slot} {offset}"
                        );
                    }
                }
            }
        }
        if number == 193 {
            assert!(compact.contains("memory::i32[0x08F8]<=9"));
            assert!(compact.contains("memory::i32[0x08F8]!=5"));
            for array in ["0x08E8", "0x08F0"] {
                for slot in 0..4 {
                    assert!(compact.contains(&format!(
                        "(memory::i16[{array}])[{slot}]=memory::i32[0x08F8]"
                    )));
                }
            }
        }
        if number == 361 {
            for skipped in [6, 7, 8] {
                assert!(compact.contains(&format!("memory::i32[0x0894]!={skipped}")));
            }
            assert_eq!(compact.matches("memory::i32[0x0894]<30").count(), 2);
            assert_eq!(
                compact
                    .matches(&format!(
                        "clear_event_bit(bit:({base}+memory::i32[0x0894]))"
                    ))
                    .count(),
                2
            );
            for array in ["0x0814", "0x081A"] {
                for slot in 0..3 {
                    assert!(compact.contains(&format!(
                        "(memory::i16[{array}])[{slot}]=memory::i32[0x0894]"
                    )));
                }
            }
        }
        if number == 214 {
            assert!(
                compact
                    .contains("set_event_bit(bit:story::Field0214Flag::LeaderLowBitSaveMiswrite)")
            );
            assert!(
                compact
                    .contains("clear_event_bit(bit:story::Field0214Flag::RestoredFieldLeaderBit2)")
            );
        }
        let formatted =
            symphonia_script_compiler::format("party-assignment", &result.source).unwrap();
        assert_eq!(
            scenario::compile("party-assignment", &formatted, &definitions).unwrap(),
            bytes,
            "{field}"
        );
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_derris_block_transfer_marks_keep_index_and_consumption_order() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets/fields");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, hash, expected_uses, index) in [
        (
            130,
            "f814310fcf3a2715eff640b9b87917ae349b8656391ee549c16863da86f8b312",
            2,
            "0x081C",
        ),
        (
            135,
            "09215965673fa1e7a20d2bda6961bc0edaa955ea15229f15ecd4a06d055eae21",
            3,
            "0x0830",
        ),
    ] {
        let path = root.join(format!("derris-kharlan-{field:04}/scripts/scenario.ssb"));
        let bytes = fs::read(path).unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(digest, hash);
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        let domain = format!("story::Field{field:04}FlagBase");
        let uses: Vec<_> = result
            .identities
            .iter()
            .filter(|id| id.domain == domain)
            .collect();
        assert_eq!(uses.len(), expected_uses);
        assert!(uses.iter().all(|id| id.value == 164 && id.symbolic));
        let actual: Vec<_> = uses.iter().map(|id| id.native.as_str()).collect();
        assert_eq!(
            actual,
            if field == 130 {
                vec!["indexed::set_event_bit", "indexed::set_event_bit"]
            } else {
                vec![
                    "indexed::test_event_bit",
                    "indexed::clear_event_bit",
                    "indexed::clear_event_bit",
                ]
            }
        );
        let compact = result.source.split_whitespace().collect::<String>();
        let slot = format!("memory::i32[{index}]");
        for expected in [
            format!("{slot}=0"),
            format!("{slot}<8"),
            format!("({slot})++"),
        ] {
            assert!(compact.contains(&expected), "{field}: {expected}");
        }
        if field == 130 {
            for expected in [
                format!("actor_id:(1000+{slot})"),
                "==1125".into(),
                "==-1125".into(),
                "story::StoryFlag::DerrisKharlanPastDoorInscriptionRead".into(),
                "story::StoryFlag::DerrisKharlanFutureDoorInscriptionRead".into(),
            ] {
                assert!(compact.contains(&expected), "{expected}");
            }
            assert!(compact.contains(
                "clear_event_bit(bit:story::Field0130Flag::FutureSideBlockTransferSelected)"
            ));
            assert!(compact.contains(
                "set_event_bit(bit:story::Field0130Flag::FutureSideBlockTransferSelected)"
            ));
        } else {
            for expected in [
                format!("actor_id:(3000+{slot})"),
                format!("resource:(-1179648+{slot})"),
                "x:-1282,y:-2092".into(),
                "x:1282,y:-2092".into(),
            ] {
                assert!(compact.contains(&expected), "{expected}");
            }
            // Keep the original future-side clear inside the per-block loop.
            // Moving it after the loop would change multiple queued transfers.
            let consume = format!(
                "native::clear_event_bit(bit:({domain}::BlockTransferPending+{slot}));native::clear_event_bit(bit:story::Field0135Flag::FutureSideBlockTransferSelected);"
            );
            assert!(compact.contains(&consume), "{consume}");
        }
        let formatted =
            symphonia_script_compiler::format("derris-transfer", &result.source).unwrap();
        assert_eq!(
            scenario::compile("derris-transfer", &formatted, &definitions).unwrap(),
            bytes
        );
        // A receiver field does not establish the same meaning in Altessa 0138.
        let unrelated =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(138))
                .unwrap();
        assert!(!unrelated.source.contains("FlagBase::BlockTransferPending"));
    }
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn reviewed_identity_evidence_matches_original_native_calls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let mut scripts = BTreeMap::new();
    let mut original_bytes = BTreeMap::new();
    let mut world_callbacks = BTreeMap::new();
    for row in evidence() {
        let pc = u32::from_str_radix(row[5].trim_start_matches("0x"), 16).unwrap();
        let value = row[1].parse::<i32>().unwrap();
        if matches!(
            row[3],
            "memory::constant32"
                | "memory::snapshot32"
                | "memory::battle_outcome"
                | "memory::flag_constant32"
        ) {
            // The corpus is immutable during this audit. Validate each image
            // once, while still checking every referenced original instruction.
            let bytes = original_bytes.entry(row[2]).or_insert_with(|| {
                let bytes = fs::read(root.join(row[2])).unwrap();
                Program::decode(&bytes).unwrap();
                bytes
            });
            let bytes = bytes.as_slice();
            // Retained events need not be reachable from the runtime registry.
            // Verify their original instructions directly, with the same bounds
            // and canonical-encoding checks as live instructions.
            let instruction = |pc| {
                let (instruction, diagnostics) =
                    symphonia_script::scenario::decode_at(bytes, pc).unwrap();
                assert!(
                    diagnostics.is_empty() && !instruction.raw,
                    "invalid evidence {row:?}"
                );
                instruction
            };
            let (address, operator) = row[4].split_once('/').unwrap();
            let address = if row[3] == "memory::battle_outcome" {
                let (address, origin) = address.split_once(" from ").unwrap();
                assert_eq!(origin, "battle", "{row:?}");
                // Recompute the must-origin proof below: every incoming path
                // must establish a battle result before this load or its copy.
                address
            } else if row[3] == "memory::snapshot32" {
                let (address, origin) = address.split_once(" from ").unwrap();
                let origin = u16::from_str_radix(origin.trim_start_matches("0x"), 16).unwrap();
                assert!(
                    (0x40..0x440).contains(&origin) && origin % 4 == 0,
                    "{row:?}"
                );
                // The subsequent context-aware identity check recomputes the
                // must-origin CFG proof against this original image.
                address
            } else {
                address
            };
            let address = u16::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
            let operator = u8::from_str_radix(operator.trim_start_matches("0x"), 16).unwrap();
            if row[0].starts_with("bound_before(") {
                assert_eq!(operator, 0x22, "{row:?}");
            }
            if matches!(row[3], "memory::snapshot32" | "memory::battle_outcome") {
                assert!((0x20..=0x25).contains(&operator), "{row:?}");
            }
            let load = instruction(pc);
            assert_eq!(load.mnemonic, "load.s32", "{row:?}");
            assert_eq!(load.operands, [i64::from(address)], "{row:?}");
            let constant = instruction(load.end_pc());
            assert!(
                matches!(
                    constant.mnemonic,
                    "push.s8" | "push.s16" | "push.u16" | "push.s32"
                ),
                "{row:?}"
            );
            assert_eq!(constant.operands, [i64::from(value)], "{row:?}");
            let calculate = instruction(constant.end_pc());
            assert_eq!(calculate.mnemonic, "calc", "{row:?}");
            assert_eq!(calculate.operands, [i64::from(operator)], "{row:?}");
            let domain = reviewed_symbol(row[0]).rsplit_once("::").unwrap().0;
            if domain.ends_with("Stage")
                || domain == "story::SideEventState"
                || matches!(
                    row[3],
                    "memory::snapshot32" | "memory::battle_outcome" | "memory::flag_constant32"
                )
            {
                let result = scripts.entry(row[2]).or_insert_with(|| {
                    let path = root.join(row[2]);
                    scenario::decompile_with_context(bytes, &definitions, Context::from_path(&path))
                        .unwrap()
                });
                assert!(
                    result.identities.iter().any(|id| id.pc == pc
                        && id.native == row[3]
                        && id.parameter == row[4]
                        && id.domain == domain
                        && id.value == value
                        && id.symbolic),
                    "missing reviewed identity {} in {} at {pc:#X}",
                    row[0],
                    row[2]
                );
            }
            continue;
        }
        if matches!(row[3], "memory::load8" | "memory::load16") {
            let bytes = original_bytes.entry(row[2]).or_insert_with(|| {
                let bytes = fs::read(root.join(row[2])).unwrap();
                Program::decode(&bytes).unwrap();
                bytes
            });
            let mnemonic = if row[3] == "memory::load8" {
                "load.s8"
            } else {
                "load.s16"
            };
            assert!(
                is_reviewed_memory_load(bytes, pc, value, mnemonic),
                "missing reviewed narrow address {} in {} at {pc:#X}",
                row[0],
                row[2]
            );
            continue;
        }
        if row[3] == "memory::global_alias" {
            let bytes = original_bytes
                .entry(row[2])
                .or_insert_with(|| fs::read(root.join(row[2])).unwrap());
            let address = value * 4 + 0x40;
            assert_eq!(row[4], format!("{address:#x}"));
            assert!(is_reviewed_global_load(bytes, pc, value), "{row:?}");
            let result = scripts.entry(row[2]).or_insert_with(|| {
                let path = root.join(row[2]);
                scenario::decompile_with_context(bytes, &definitions, Context::from_path(&path))
                    .unwrap()
            });
            let domain = reviewed_symbol(row[0]).rsplit_once("::").unwrap().0;
            assert!(
                result.identities.iter().any(|id| id.pc == pc
                    && id.native == row[3]
                    && id.parameter == row[4]
                    && id.domain == domain
                    && id.value == value
                    && id.symbolic),
                "{row:?}"
            );
            continue;
        }
        if row[3] == "memory::load32" {
            let bytes = original_bytes.entry(row[2]).or_insert_with(|| {
                let bytes = fs::read(root.join(row[2])).unwrap();
                Program::decode(&bytes).unwrap();
                bytes
            });
            assert!(
                is_reviewed_global_load(bytes, pc, value),
                "missing reviewed global {} in {} at {pc:#X}",
                row[0],
                row[2]
            );
            continue;
        }
        let exploration = row[0]
            .strip_prefix("story::WorldFlag::Exploration")
            .and_then(|s| s.strip_suffix("Caravan0247TransferMarked"))
            .map(|s| s.parse::<u32>().unwrap());
        let triet = row[0] == "story::WorldFlag::TrietCaravan0247TransferMarked";
        if exploration.is_some() || triet {
            assert_eq!(row[2], "world/scripts/scenario.ssb");
            assert_eq!(row[3], "set_event_bit");
            assert_eq!(row[4], "bit");
            let (analysis, program) = world_callbacks.entry(row[2]).or_insert_with(|| {
                let bytes = fs::read(root.join(row[2])).unwrap();
                (
                    symphonia_script::scenario::analyze(&bytes).unwrap(),
                    Program::decode(&bytes).unwrap(),
                )
            });
            let entry = analysis
                .records
                .iter()
                .filter(|r| r.pc <= pc)
                .max_by_key(|r| r.pc)
                .unwrap();
            assert_eq!(entry.kind, 1, "{row:?}");
            if let Some(index) = exploration {
                assert!(index <= 31);
                assert_eq!(value, 160 + index as i32);
                assert!([63 + index, 306 + index].contains(&entry.key), "{row:?}");
            } else {
                assert_eq!(value, 193);
                assert_eq!(entry.key, 54);
            }
            // Verify each original registry binding and the complete callback,
            // including the unchanged caravan destination.
            // Only the flag-write and field-transfer native calls are present.
            let mut expected = Vec::new();
            for (opcode, arguments) in [(0x68, vec![value]), (0x40, vec![247, 24, -143, -6, 175])] {
                for value in arguments {
                    expected.extend([Op::Push(value), Op::Calculate(0), Op::Argument]);
                }
                expected.push(Op::Native(opcode));
            }
            expected.push(Op::End);
            let mut at = entry.pc;
            for op in expected {
                let (actual, next) = program.instruction(at).unwrap();
                assert_eq!(actual, op, "{row:?} at {at:#X}");
                if op == Op::Native(0x68) {
                    assert_eq!(at, pc, "{row:?}");
                }
                at = next;
            }
        }
        let result = scripts.entry(row[2]).or_insert_with(|| {
            let path = root.join(row[2]);
            scenario::decompile_with_context(
                &fs::read(&path).unwrap(),
                &definitions,
                Context::from_path(&path),
            )
            .unwrap()
        });
        let domain = reviewed_symbol(row[0]).rsplit_once("::").unwrap().0;
        assert!(
            result.identities.iter().any(|id| id.pc == pc
                && id.native == row[3]
                && id.parameter == row[4]
                && id.domain == domain
                && id.value == value
                && id.symbolic),
            "missing reviewed {}={} in {} at {pc:#X}",
            row[0],
            value,
            row[2]
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_equipped_title_exchange_returns_the_previous_packed_halfword() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(section * 4) as usize + (address - base) as usize)
            })
            .unwrap()
    };
    let at = |address| word(offset(address));
    let start = offset(0x8004127c);
    assert_eq!(
        digest(&dol[start..start + 0x9c]),
        "07cbb0e024bf790c76d83f0436748841b21d7c388b6f880d1e5d7530fdce846d"
    );
    for (address, expected) in original_words::words::<u32>(
        "original_equipped_title_exchange_returns_the_previous_packed_halfword",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    let native = registry.get(0x8c).unwrap();
    assert_eq!(native.name, "set_equipped_title");
    assert_eq!(
        native.argument_types,
        ["characters::CharacterId", "characters::TitleId"]
    );
    assert_eq!(native.result_type.as_deref(), Some("characters::TitleId"));
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_story_flag_handlers_ignore_high_indices_without_a_query_result() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |at: usize| u32::from_be_bytes(dol[at..at + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    for (registry, opcode, address, length, hash, compare) in [
        (
            0x801e79fc,
            0x68,
            0x8004dde0,
            0x5c,
            "033b3b6890b78df2e7b43cb61ce9710d86df41fb1f45c51698de4c147aedcc72",
            0x2c030800,
        ),
        (
            0x801e7a04,
            0x69,
            0x8004dd84,
            0x5c,
            "c6482b7615680a2fe5ab713dcacb081022f1f45c3cccb6b1a4b2cd8647bee21a",
            0x2c030800,
        ),
        (
            0x801e7a0c,
            0x6a,
            0x8004dcdc,
            0xa8,
            "5c513f287cefe6a47acf0c9fdb853676d0ad197460cf36489f380ae3a9fd4a7b",
            0x2c050800,
        ),
    ] {
        assert_eq!(word(offset(registry)), opcode << 16);
        assert_eq!(word(offset(registry + 4)), address);
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + length]), hash);
        // Pop the argument before the signed upper-bound comparison. Returning
        // here skips every flag-bank write and the query's value-frame push.
        assert_eq!(word(start + 0x0c), 0x90830240);
        assert_eq!(word(start + 0x18), compare);
        assert_eq!(word(start + 0x1c), 0x4c800020); // bgelr
    }
    // Only the query's in-range path stores a value and increments frame depth.
    assert_eq!(word(offset(0x8004dd4c)), 0x90040020);
    assert_eq!(word(offset(0x8004dd54)), 0x38850001);
    assert_eq!(word(offset(0x8004dd5c)), 0x90830018);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_actor_property_default_preserves_no_result_and_message_depth_alias() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // The field getter pushes zero for an absent actor outside party selectors
    // 100..112; an unsupported selector for an existing actor pushes nothing.
    // Battle mode uses its separate manager ABI and always pushes a result.
    // The common field resolver returns fixed records even without a loaded model.
    // Terminal calc0 after no push changes depth0 to -1. Number formatting reads
    // state + 0x20 + depth*8, hence the depth word at state+0x18: it prints -1.
    for (address, expected) in original_words::words::<u32>(
        "original_actor_property_default_preserves_no_result_and_message_depth_alias",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    // Verify every switch hole, rather than assuming all unknown fields return0.
    for selector in 0..=122 {
        let supported = matches!(selector, 1..=67 | 100..=105 | 112..=122);
        assert_eq!(
            at(0x801e8594 + selector * 4) != 0x800573f4,
            supported,
            "field actor property {selector}"
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable and battle REL at local/extracted"]
fn original_battle_rule_exchange_keeps_native_widths_and_percentages() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let dol = fs::read(root.join("sys/main.dol")).unwrap();
    let rel = fs::read(root.join("files/US_r_Top2Btl.rel")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        digest(&rel),
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    let word = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(&dol, 0x48 + section * 4);
                let size = word(&dol, 0x90 + section * 4);
                (address >= base && address - base < size).then(|| {
                    word(
                        &dol,
                        word(&dol, section * 4) as usize + (address - base) as usize,
                    )
                })
            })
            .unwrap()
    };
    // E7 selects a stored field, returns its PREVIOUS value and consumes two
    // arguments. Unknown selectors retain result0. Stores truncate to u16/u8;
    // the three adjustment results sign-extend their old bytes. Selector2
    // exchanges bit1 of the coliseum flags and treats any nonzero input as true.
    for (address, expected) in original_words::words::<u32>(
        "original_battle_rule_exchange_keeps_native_widths_and_percentages",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let sections = word(&rel, 0x10) as usize;
    let text = (word(&rel, sections + 8) & !3) as usize;
    assert_eq!(word(&rel, 0), 1);
    assert_eq!(text, 0xcc);
    // Battle command setup clears the disabled bits and applies coliseum flags.
    // The damage path reads each signed adjustment, multiplies the respective
    // stat, divides by100 using the compiler's signed reciprocal sequence, and
    // adds it back. Intelligence is applied on both sides of its damage path.
    for (offset, expected) in original_words::words::<usize>(
        "original_battle_rule_exchange_keeps_native_widths_and_percentages-2",
    ) {
        assert_eq!(word(&rel, text + offset), expected, "REL text+{offset:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    let native = registry.get(0xe7).unwrap();
    assert_eq!(native.name, "configure_battle_rules");
    assert_eq!(native.arguments, ["rule", "value"]);
    assert_eq!(native.argument_types, ["scenario::BattleRule", "i32"]);
    use symphonia_constants::BattleRule;
    assert_eq!(BattleRule::ALL.map(BattleRule::raw), [0, 1, 2, 3, 4, 5]);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_equipment_selectors_and_script_inventory_sequences_match() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // 2B/2D use script order weapon/body/head/accessory1/accessory2/arm:
    // the six saved halfwords have physical offsets 1EA,1EC,1EE,1F2,1F4,1F0
    // relative to session + character*118. Controlled999999 uses the leader.
    // Getter starts with result0 and signed-loads only valid character/slot IDs.
    // Unequip returns the old item if possible but unconditionally clears it.
    // 2C's occupied non-accessory branches pass NEW item r31 to both add1 and
    // remove1. Accessory consumes first, fills the first empty slot, or returns
    // the old second accessory and overwrites it. Capacity failure is ignored.
    // The item-category helper separates arm kind3 from accessory kind4.
    for (address, expected) in original_words::words::<u32>(
        "original_equipment_selectors_and_script_inventory_sequences_match",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    use symphonia_constants::EquipmentSlot;
    assert_eq!(
        EquipmentSlot::ALL.map(EquipmentSlot::raw),
        [0, 1, 2, 3, 4, 5]
    );
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    for opcode in [0x2b, 0x2d] {
        assert_eq!(
            registry.get(opcode).unwrap().argument_types,
            ["characters::CharacterId", "items::EquipmentSlot"]
        );
    }
    assert_eq!(
        registry.get(0x2b).unwrap().result_type.as_deref(),
        Some("items::ItemId")
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_figurine_flags_and_party_snapshots_keep_their_distinct_banks() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // 8A tests the bit at session+1E98+id/8 before changing anything. It
    // pushes previous ownership, then opcode0 sets and opcode1 clears; every
    // other opcode only queries. IDs>=328 return0, independently of the288
    // catalogue records. Negative IDs can alias earlier native memory.
    // New Game Plus benefit0800 copies42 bytes, including the reserved bits.
    // 89 copies eight live formation bytes E9D..EA4 to snapshot1F34..1F3B
    // for operation0. Operation1 clears the snapshot; others do nothing. It
    // consumes one argument and never pushes a result or restores the live list.
    for (address, expected) in original_words::words::<u32>(
        "original_figurine_flags_and_party_snapshots_keep_their_distinct_banks",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0x8a).unwrap().argument_types,
        ["figurines::FigurineOperation", "figurines::FigurineId"]
    );
    assert_eq!(
        registry.get(0x89).unwrap().argument_types,
        ["scenario::PartySnapshotOperation"]
    );
    assert_eq!(
        symphonia_constants::FigurineOperation::ALL
            .map(symphonia_constants::FigurineOperation::raw),
        [0, 1, 2]
    );
    assert_eq!(
        symphonia_constants::PartySnapshotOperation::ALL
            .map(symphonia_constants::PartySnapshotOperation::raw),
        [0, 1]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_render_controls_keep_settings_enable_and_copy_plane_abis_distinct() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // 46: command128 changes enable bit0 using nonzero truth, command129
    // clears initialization bit7, signed command<=7 stores value in slot(cmd&7).
    // Other commands ignore the value; this service returns nothing.
    // AB: setters0/1, readers2/3 and resets4/5 address two independent floats.
    // Inputs divide by100; result is the old float multiplied by100 then fctiwz.
    // Read/reset argument evaluation is retained although its value is ignored.
    // Setters update their enable byte using depth!=0; resets restore174/190.
    for (address, expected) in original_words::words::<u32>(
        "original_render_controls_keep_settings_enable_and_copy_plane_abis_distinct",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(registry.get(0x46).unwrap().arguments, ["command", "value"]);
    assert_eq!(
        registry.get(0xab).unwrap().arguments,
        ["command", "depth_hundredths"]
    );
    assert_eq!(
        symphonia_constants::RenderingCommand::ALL.map(symphonia_constants::RenderingCommand::raw),
        [0, 1, 2, 3, 4, 5, 6, 7, 128, 129]
    );
    assert_eq!(
        symphonia_constants::ScreenCopyCommand::ALL
            .map(symphonia_constants::ScreenCopyCommand::raw),
        [0, 1, 2, 3, 4, 5]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_voice_preloads_resolve_the_named_archive_directory_and_buffer_roles() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // Native7E pops buffer selector then packed voice reference, ignores the
    // low member word, and looks up the high word by exact directory base.
    // Zero reference/highword skips loading. Selector0 chooses the primary
    // handle/buffer; every nonzero selector chooses the secondary. Repeated
    // archive loads compare the existing handle before issuing CRI loading.
    // Both the lookup data and filename bytes establish archive identities;
    // physical CRI handles are separate from these packed reference bases.
    for (address, expected) in original_words::words::<u32>(
        "original_voice_preloads_resolve_the_named_archive_directory_and_buffer_roles",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    use symphonia_constants::VoiceArchiveReference;
    let byte = |address: u32| (at(address & !3) >> (24 - (address & 3) * 8)) as u8;
    for (index, (archive, pointer, handle)) in [
        (
            VoiceArchiveReference::EventScenes001Through009,
            0x801df850,
            0x00000000,
        ),
        (
            VoiceArchiveReference::EventScenes011Through020,
            0x801df864,
            0x00010000,
        ),
        (
            VoiceArchiveReference::EventScenes021Through030,
            0x801df878,
            0x00020000,
        ),
        (
            VoiceArchiveReference::EventScenes031Through040,
            0x801df88c,
            0x00030000,
        ),
        (
            VoiceArchiveReference::EventScenes041Through050,
            0x801df8a0,
            0x00040000,
        ),
        (
            VoiceArchiveReference::EventScenes051Through060,
            0x801df8b4,
            0x00050000,
        ),
        (
            VoiceArchiveReference::EventScenes061Through070,
            0x801df8c8,
            0x00060000,
        ),
        (
            VoiceArchiveReference::EventScenes071Through083,
            0x801df8dc,
            0x00070000,
        ),
        (
            VoiceArchiveReference::EventScenes084Through090,
            0x801df8f0,
            0x00080000,
        ),
        (
            VoiceArchiveReference::EventScenes091Through100,
            0x801df904,
            0x00090000,
        ),
        (
            VoiceArchiveReference::EventScenes101Through110,
            0x801df918,
            0x000a0000,
        ),
        (
            VoiceArchiveReference::EventScenes111Through120,
            0x801df92c,
            0x000b0000,
        ),
        (
            VoiceArchiveReference::EventScenes121Through132,
            0x801df940,
            0x000c0000,
        ),
        (VoiceArchiveReference::SkitCommon, 0x801df954, 0x000d0000),
        (VoiceArchiveReference::SkitDisc1, 0x801df968, 0x000e0000),
        (VoiceArchiveReference::SkitDisc2, 0x801df97c, 0x000f0000),
        (VoiceArchiveReference::BattleVoice, 0x801df990, 0x00780000),
    ]
    .into_iter()
    .enumerate()
    {
        let address = 0x802a30a0 + index as u32 * 12;
        assert_eq!(at(address), pointer);
        assert_eq!(at(address + 4), archive.raw());
        assert_eq!(at(address + 8), handle);
        let name = archive.resource_name().unwrap();
        let actual: Vec<_> = (0..=name.len()).map(|i| byte(pointer + i as u32)).collect();
        assert_eq!(actual, name.bytes().chain([0]).collect::<Vec<_>>());
    }
    assert_eq!(at(0x802a30a0 + 17 * 12), 0);
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0x7e).unwrap().arguments,
        ["voice_reference", "bank_slot"]
    );
    assert_eq!(
        registry.get(0x7e).unwrap().argument_types,
        [
            "resources::VoiceArchiveReference",
            "scenario::VoiceBankSlot"
        ]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_dialogue_voice_controls_keep_archive_members_and_sentinels() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // Formatter9 writes the evaluated32-bit value unchanged after byte9.
    // Dialogue consumes zero without an audio call and -1 through stop calls;
    // every other value reaches800EF150 while voiced playback is enabled.
    // The player splits high16 archive base and low16 member index, resolves
    // that base in802A30A0, then calls80169D04 with its physical handle and member.
    // High-nibble8 references have a separate direct-stream path; no archive
    // identity is inferred for them. The same archive directory is byte-pinned
    // by the original preload audit.
    for (address, expected) in original_words::words::<u32>(
        "original_dialogue_voice_controls_keep_archive_members_and_sentinels",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    assert_eq!(symphonia_constants::VoiceControl::None.raw(), 0);
    assert_eq!(symphonia_constants::VoiceControl::Stop.raw(), -1);
}

#[test]
#[ignore = "requires original voice archives on both discs and generated readable-script reports"]
fn dialogue_voice_references_resolve_playable_members_in_original_archives() {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let mut archives = BTreeMap::new();
    for (base, relative, length, digest) in [
        (
            0x000a0000,
            "extracted/disc1/files/EV/sce_001_009.afs",
            12781568u64,
            "05578c6c657a29cb923b8aadde08a76499ba77f390f0807fd5ecdf9750c58320",
        ),
        (
            0x000b0000,
            "extracted/disc1/files/EV/sce_011_020.afs",
            6397952u64,
            "d1c20b29604a8305c59317f8876bb5a0f38091ae1d3fcf2a08d097336f6cea99",
        ),
        (
            0x000c0000,
            "extracted/disc1/files/EV/sce_021_030.afs",
            19445760u64,
            "c889c5798e3c5d80eb1f203e9510819c4d3996de5d9685d4f61a0d2614550fb9",
        ),
        (
            0x000d0000,
            "extracted/disc1/files/EV/sce_031_040.afs",
            17235968u64,
            "f6fadcb2080a61fd5b2245ff928b4d3feed66f1de843e1f288ac3bf9b5cc177c",
        ),
        (
            0x000e0000,
            "extracted/disc1/files/EV/sce_041_050.afs",
            19605504u64,
            "f0ccdcd9a1b07eee8e7eb94615c05f3bb68aa975a08923e7952e93d301bd711d",
        ),
        (
            0x000f0000,
            "extracted/disc1/files/EV/sce_051_060.afs",
            18018304u64,
            "d5d46cb24a30597be6df813b1a9d5061102fb5d25ce244f9075a68afdfd0ee80",
        ),
        (
            0x00100000,
            "extracted/disc1/files/EV/sce_061_070.afs",
            18405376u64,
            "13096bfda355628d0a96fbe39568ebe50f4aad41d3f1125f984c62b1d0901c15",
        ),
        (
            0x00110000,
            "extracted/disc1/files/EV/sce_071_083.afs",
            23222272u64,
            "28aedc233c4d911ae709e04f19deb45ed64927e91d21030b9ab8ad42c50da6a7",
        ),
        (
            0x00140000,
            "extracted/disc2/files/EV/sce_084_090.afs",
            11425792u64,
            "d480666286b48926ab0853127b8a3b0ec689c6ec6d905f68c9c7ba01c9956efe",
        ),
        (
            0x00150000,
            "extracted/disc2/files/EV/sce_091_100.afs",
            17905664u64,
            "ce9eaeb0c997fbba99077e16e376a7bac0f12941fac478160d9203307a6a859b",
        ),
        (
            0x00160000,
            "extracted/disc2/files/EV/sce_101_110.afs",
            29564928u64,
            "d2a02b40eda173997e05d3640873ca0d98142c1402f01c6e6ec4d380742bf041",
        ),
        (
            0x00170000,
            "extracted/disc2/files/EV/sce_111_120.afs",
            16586752u64,
            "08d951dc691e34859e940f3a261b35af5b1dddb42e94fd9d8125838808c330cc",
        ),
        (
            0x00180000,
            "extracted/disc2/files/EV/sce_121_132.afs",
            36110336u64,
            "b533972666f755c2e9c2c3bf1b76b14b5d2999d6e6baf5fd33f3b9a9463a3de0",
        ),
    ] {
        let mut file = fs::File::open(root.join(relative)).unwrap();
        assert_eq!(file.metadata().unwrap().len(), length, "{relative}");
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let length = file.read(&mut buffer).unwrap();
            if length == 0 {
                break;
            }
            hasher.update(&buffer[..length]);
        }
        assert_eq!(
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            digest,
            "{relative}"
        );
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut header = [0u8; 8];
        file.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"AFS\0");
        let count = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        assert!((1..=65536).contains(&count));
        let mut table = vec![0u8; count * 8];
        file.read_exact(&mut table).unwrap();
        let entries: Vec<_> = table
            .as_chunks::<8>()
            .0
            .iter()
            .map(|row| {
                let offset = u32::from_le_bytes(row[..4].try_into().unwrap());
                let size = u32::from_le_bytes(row[4..].try_into().unwrap());
                assert!(
                    offset as usize >= 16 + count * 8
                        && size >= 20
                        && u64::from(offset) + u64::from(size) <= length
                );
                (offset, size)
            })
            .collect();
        archives.insert(base, (file, entries));
    }
    let report =
        fs::read_to_string(root.join("readable-scripts/message-identity-uses.tsv")).unwrap();
    let mut uses = 0;
    let mut checked = BTreeSet::new();
    for line in report.lines().skip(1) {
        let row: Vec<_> = line.split('\t').collect();
        if row[2] != "9"
            || row[3] != "resources::VoiceArchiveReference"
            || row[4] != "message::value"
        {
            continue;
        }
        let reference: u32 = row[6].parse().unwrap();
        uses += 1;
        if !checked.insert(reference) {
            continue;
        }
        let (file, entries) = archives.get_mut(&(reference & 0xffff0000)).unwrap();
        let member = (reference & 65535) as usize;
        let &(offset, size) = entries
            .get(member)
            .unwrap_or_else(|| panic!("missing voice{reference:#x}"));
        assert!(size >= 20);
        file.seek(SeekFrom::Start(u64::from(offset))).unwrap();
        let mut header = [0u8; 20];
        file.read_exact(&mut header).unwrap();
        assert_eq!(&header[..2], &[0x80, 0], "voice{reference:#x}");
        assert!(matches!(header[4], 0x10 | 0x11), "voice{reference:#x}");
        assert_eq!(header[7], 1, "voice{reference:#x}");
        assert!(u32::from_be_bytes(header[8..12].try_into().unwrap()) > 0);
        assert!(u32::from_be_bytes(header[12..16].try_into().unwrap()) > 0);
    }
    assert_eq!(uses, 8592);
    assert_eq!(checked.len(), 6900);
    assert_eq!(archives.len(), 13);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_camera_track_commands_preserve_playback_option_and_query_encodings() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    assert_eq!(
        Sha256::digest(&dol)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(word(section * 4) as usize + (address - base) as usize))
            })
            .unwrap()
    };
    // AD resolves the first argument as a resource, pauses for any nonzero
    // full-width second argument, and loops only for exactly1 in the third.
    // AE dispatches commands0..14. It stores repeat as a byte and reads that
    // byte back; rates divide by200 and reads truncate the raw frame step.
    // Target offsets convert through f32 before their integer result; target
    // actor returns its original key. The default branch pushes zero.
    // Initializer/consumer bytes independently identify the playback fields:
    // +C repeat, +D completion, +E advance, +F active, and +88 target actor.
    // The consumer treats any nonzero stored repeat byte as looping.
    for (address, expected) in original_words::words::<u32>(
        "original_camera_track_commands_preserve_playback_option_and_query_encodings",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let targets = [
        0x80048f38, 0x80048f88, 0x80048fd4, 0x80049028, 0x80049090, 0x80049100, 0x8004914c,
        0x800491a4, 0x800491fc, 0x80049254, 0x800492a0, 0x800492ec, 0x80049338, 0x800493ac,
        0x80049420,
    ];
    for (command, target) in targets.into_iter().enumerate() {
        assert_eq!(at(0x801e81a0 + command as u32 * 4), target);
    }
    // The camera samples and publishes its pose, resolves the retained raw
    // actor key, then advances once. Queries/seeks do not run this block.
    // Advancement uses single-precision adds and one signed wrap only;
    // completion is set after an upper-end crossing and never cleared here.
    for (address, expected) in original_words::words::<u32>(
        "original_camera_track_commands_preserve_playback_option_and_query_encodings-2",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    // Native5F consumes eight operands but maps position/start/end only.
    // The duration/range division and position offset multiplication use
    // double precision, followed by lower/upper bounds and one final frsp.
    // There is no zero-range guard; IEEE infinities clamp and NaN survives.
    for (address, expected) in original_words::words::<u32>(
        "original_camera_track_commands_preserve_playback_option_and_query_encodings-3",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(
        registry.get(0xad).unwrap().arguments,
        ["resource_handle", "start_mode", "repeat_mode"]
    );
    assert_eq!(
        registry.get(0xad).unwrap().argument_types,
        [
            "i32",
            "scenario::CameraTrackStart",
            "scenario::CameraTrackRepeat"
        ]
    );
    assert_eq!(
        registry.get(0xae).unwrap().argument_types,
        ["scenario::CameraTrackCommand", "i32"]
    );
    assert_eq!(
        symphonia_constants::CameraTrackCommand::ALL
            .map(symphonia_constants::CameraTrackCommand::raw),
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable, battle REL and cooked scripts at local"]
fn original_battle_camera_and_movement_option_names_follow_native_dispatch() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(root.join("extracted/disc1/sys/main.dol")).unwrap();
    let rel = fs::read(root.join("extracted/disc1/files/US_r_Top2Btl.rel")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    assert_eq!(
        digest(&rel),
        "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214"
    );
    let word = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let at = |address: u32| {
        (0..18)
            .find_map(|section| {
                let base = word(&dol, 0x48 + section * 4);
                let size = word(&dol, 0x90 + section * 4);
                (address >= base && address - base < size).then(|| {
                    word(
                        &dol,
                        word(&dol, section * 4) as usize + (address - base) as usize,
                    )
                })
            })
            .unwrap()
    };
    // Native93 is mode9-only and always pushes zero. Its command2 initializes
    // position/pitch/heading,3/4 set absolute/relative position targets,5/6
    // pitch targets,7/8 heading targets,9 a heading-relative displacement,
    // and10 the FOV. The high-bit timing forms route to distance/angle divided
    // by a frame count; ordinary values route to hundredths-of-unit speed.
    // Command10 forwards only its first value, ignoring the retained timing.
    // Native18 forwards all five operands in battle; its field branch uses
    // actor, heading, distance, speed-or-duration and any-nonzero heading lock.
    // NativeC0 addresses one-based formation preference slots, returns their old
    // signed byte, treats -1 as query and stores other values masked to3bits.
    // Invalid slots force a read-only query of slot1, not a character lookup.
    for (address, expected) in original_words::words::<u32>(
        "original_battle_camera_and_movement_option_names_follow_native_dispatch",
    ) {
        assert_eq!(at(address), expected, "DOL {address:#x}");
    }
    for (command, target) in [
        0x80043050, 0x80043080, 0x800430ac, 0x800430d8, 0x8004314c, 0x800431c4, 0x80043234,
        0x800432a0, 0x80043310, 0x80043380, 0x800434b4,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(at(0x801e8060 + command as u32 * 4), target);
    }
    let sections = word(&rel, 0x10) as usize;
    let text = (word(&rel, sections + 8) & !3) as usize;
    let rodata = (word(&rel, sections + 4 * 8) & !3) as usize;
    // Pin complete original initializer and camera service bodies, including
    // absolute/relative float stores and timed rate calculation branches.
    assert_eq!(
        digest(&rel[text + 0x5cea4..text + 0x5cf9c]),
        "83987ef33d54728fe7222da53948c9504779212b42921ad6690f879eb0adfda6"
    );
    assert_eq!(
        digest(&rel[text + 0x5c7f4..text + 0x5cdd0]),
        "6095e2a7b321407138f4ea435d303900d013beb8195bcae064b850eb4ca416fe"
    );
    // The per-update consumer visits pitch then heading, passing the stored
    // target/rate to a scalar f32 helper and clearing the rate on arrival.
    // That helper compares the absolute residual with a fixed increment;
    // neither a remaining-frame interpolation nor shortest-angle wrapping.
    assert_eq!(
        digest(&rel[text + 0x508dc..text + 0x5096c]),
        "487200215abc3a9aeb7fd161cbf9da7fd47ec0eb0fd0265ba07e8bf25a2f6862"
    );
    assert_eq!(
        digest(&rel[text + 0x4d750..text + 0x4d798]),
        "3ccb143a3b2997e9b9558857f004e3d32a57f0e758e08e32ef83cbd2d9faf055"
    );
    assert_eq!(word(&rel, rodata + 0x2968), 0_f32.to_bits());
    assert_eq!(word(&rel, rodata + 0x296c), 0x3c8e_fa33);
    assert_eq!(word(&rel, rodata + 0x2970), 500_f32.to_bits());
    assert_eq!(word(&rel, rodata + 0x32a4), 1_f32.to_bits());
    assert_eq!(word(&rel, rodata + 0x32a0), 0.01_f32.to_bits());
    assert_eq!(word(&rel, rodata + 0x32a8), 0.1_f32.to_bits());
    assert_eq!(word(&rel, text + 0x5cdac), 0xec020032); // FOV argument *0.1
    assert_eq!(word(&rel, text + 0x5cdb0), 0xd0030024); // lens field
    assert_eq!(word(&rel, text + 0x53680), 0x90030000); // publish export table
    // The battle actor consumer searches the active formation roster by actor
    // key, then reads the matching formation index at save+EA5. These four
    // preferences follow slots, rather than character IDs or controller ports.
    for (offset, expected) in original_words::words::<usize>(
        "original_battle_camera_and_movement_option_names_follow_native_dispatch-2",
    ) {
        assert_eq!(word(&rel, text + offset), expected, "REL text+{offset:#x}");
    }
    let imports = word(&rel, 0x28) as usize;
    let mut relocations = BTreeSet::new();
    for import in (imports..imports + word(&rel, 0x2c) as usize).step_by(8) {
        let module = word(&rel, import);
        let mut cursor = word(&rel, import + 4) as usize;
        let (mut section, mut offset) = (0, 0usize);
        loop {
            let delta = u16::from_be_bytes(rel[cursor..cursor + 2].try_into().unwrap());
            let kind = rel[cursor + 2];
            let target = rel[cursor + 3];
            let addend = word(&rel, cursor + 4);
            cursor += 8;
            match kind {
                203 => break,
                202 => {
                    section = target;
                    offset = 0;
                }
                _ => {
                    offset += usize::from(delta);
                    relocations.insert((module, section, offset, kind, target, addend));
                }
            }
        }
    }
    for relocation in [
        (1, 1, 0x5366a, 6, 5, 0x5380),
        (1, 1, 0x5367a, 4, 5, 0x5380),
        (0, 1, 0x53672, 6, 9, 0x8035a514),
        (0, 1, 0x53682, 4, 9, 0x8035a514),
        (1, 5, 0x53c8, 1, 1, 0x5cea4),
        (1, 5, 0x53d0, 1, 1, 0x5c7f4),
        (1, 1, 0x5cd8a, 6, 4, 0x32a8),
        (1, 1, 0x5cd9e, 4, 4, 0x32a8),
        (1, 1, 0xceee, 4, 6, 0x540),
        (1, 1, 0xcf06, 6, 6, 0x540),
        (0, 1, 0xcf02, 6, 9, 0x8035a768),
        (0, 1, 0xcf0e, 4, 9, 0x8035a768),
    ] {
        assert!(
            relocations.contains(&relocation),
            "missing relocation {relocation:?}"
        );
    }
    let registry = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(registry.get(0x93).unwrap().name, "configure_battle_camera");
    assert_eq!(
        registry.get(0x18).unwrap().arguments,
        [
            "actor_id",
            "heading_degrees",
            "distance",
            "speed_or_duration",
            "facing"
        ]
    );
    assert_eq!(
        registry.get(0xc0).unwrap().arguments,
        ["party_slot", "mode"]
    );
    assert_eq!(
        registry.get(0xc0).unwrap().argument_types,
        ["battle::BattleControlSlot", "battle::BattleControlMode"]
    );
    assert_eq!(
        symphonia_constants::BattleCameraCommand::ALL
            .map(symphonia_constants::BattleCameraCommand::raw),
        [2, 3, 4, 5, 6, 7, 8, 9, 10]
    );
    assert_eq!(
        symphonia_constants::BattleControlSlot::ALL
            .map(symphonia_constants::BattleControlSlot::raw),
        [1, 2, 3, 4]
    );
    let settings =
        fs::read(root.join("all-assets/fields/new-game-settings/scripts/scenario.ssb")).unwrap();
    assert!(
        settings
            .windows(b"Auto\nSemi-Auto\nManual".len())
            .any(|bytes| bytes == b"Auto\nSemi-Auto\nManual")
    );
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    let source = scenario::decompile(&settings, &definitions).unwrap().source;
    for mode in ["Auto", "SemiAuto", "Manual"] {
        assert!(source.contains(&format!("battle::BattleControlMode::{mode}")));
    }
    assert_eq!(symphonia_constants::BattleControlMode::Auto.raw(), 2);
    assert_eq!(symphonia_constants::BattleControlMode::SemiAuto.raw(), 1);
    assert_eq!(symphonia_constants::BattleControlMode::Manual.raw(), 0);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_party_queries_narrow_character_inputs_and_preserve_slot_results() {
    use sha2::{Digest, Sha256};
    let dol = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
    )
    .unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    for (address, size, expected) in [
        (
            0x80054100,
            0x128,
            "2b978926016fa3e5dbc9391a63280f2148e4005726b63a96e0733466caf5ed9e",
        ),
        (
            0x80080b48,
            0xe0,
            "7b1f5225f78cdb78aba6340ad41b43f94adf5e543ef9295115a42a79e5613d5d",
        ),
    ] {
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + size]), expected, "{address:#x}");
    }
    // Native27 switches on the high word. Invalid low-word slots report and
    // join the normal result push with initialized zero; valid slots load a
    // signed save byte. The other branch calls the full eight-cell helper.
    for (address, expected) in original_words::words::<u32>(
        "original_party_queries_narrow_character_inputs_and_preserve_slot_results",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    for (slot, selector) in symphonia_constants::FormationSlotSelector::ALL
        .into_iter()
        .enumerate()
    {
        assert_ne!(selector.raw() & !0xffff, 0);
        assert_eq!(selector.raw() & 0xffff, slot as i32);
    }
    let native = symphonia_script::semantics::NativeRegistry::gqseaf();
    assert_eq!(native.get(0x27).unwrap().name, "find_party_member");
    assert_eq!(native.get(0x27).unwrap().arguments, ["character_or_slot"]);
}

#[test]
#[ignore = "requires the original GQSEAF executable and field-debug scenario at local"]
fn original_field_camera_bank_has_eight_initialized_byte_selected_templates() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(root.join("extracted/disc1/sys/main.dol")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    // Pin the full original initializer, its mode1 follow-preset helper and
    // NativeA0 dispatcher. The initializer owns exactly eight 0x94-byte rows.
    for (address, size, expected) in [
        (
            0x80060100,
            0x27c,
            "c506a3e76131a75460fa60ffb6f3dd5d58eddf9f3ea84b53c359b81c1c3173ec",
        ),
        (
            0x8005fd2c,
            0x3d4,
            "7c3ce20ad39c259fff21a234114b7e2e1de7602db5e3e8e26029a895af8e1ab4",
        ),
        (
            0x8004a8c8,
            0x448,
            "73979971dd4f28f96b03bae747341d994a42afbe3afc1501c752758b6388b021",
        ),
    ] {
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + size]), expected, "{address:#x}");
    }
    for (address, expected) in original_words::words::<u32>(
        "original_field_camera_bank_has_eight_initialized_byte_selected_templates",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    for (address, value) in [
        (0x8035b658, -10000_f32),
        (0x8035b65c, 10000_f32),
        (0x8035b67c, 8_f32),
        (0x8035b680, 322_f32),
        (0x8035b684, 42_f32),
        (0x8035b688, 1890_f32),
        (0x8035b68c, 87_f32),
        (0x8035b698, 27_f32),
    ] {
        assert_eq!(word(offset(address)), value.to_bits());
    }
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for value in 0..8 {
        assert_eq!(
            symphonia_constants::CameraSelector::try_from(value)
                .unwrap()
                .name(),
            format!("Template{value}")
        );
    }
    let input = "fields/field-debug-menu/scripts/scenario.ssb";
    let bytes = fs::read(root.join("all-assets").join(input)).unwrap();
    let result = scenario::decompile_with_context(&bytes, &definitions, Context::Field).unwrap();
    let uses = result
        .identities
        .iter()
        .filter(|id| id.domain == "scenario::CameraSelector" && id.value == 6)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 3);
    assert_eq!(
        uses.iter().map(|id| id.pc).collect::<BTreeSet<_>>(),
        [0x1ac, 0x463d, 0x4a02].into()
    );
    for identity in uses {
        assert!(identity.symbolic && !identity.partial_mask);
        assert_eq!(identity.native, "select_camera");
        let (instruction, diagnostics) =
            symphonia_script::scenario::decode_at(&bytes, identity.pc).unwrap();
        assert!(diagnostics.is_empty() && !instruction.raw);
        assert_eq!(instruction.mnemonic, "proc");
        assert_eq!(instruction.operands, [0xa0]);
    }
    assert_eq!(
        result
            .source
            .matches("scenario::CameraSelector::Template6")
            .count(),
        3
    );
    assert_eq!(
        scenario::compile(input, &result.source, &definitions).unwrap(),
        bytes
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable and dialogue scenarios at local"]
fn original_dialogue_wait_fallbacks_and_default_grid_preserve_retained_encodings() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(root.join("extracted/disc1/sys/main.dol")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    // Pin the full unmodified argument handler, current-window resolver,
    // complete native dispatcher and dialogue configurator.
    for (address, size, expected) in [
        (
            0x8004e224,
            0x3c,
            "3a350d4934ed5ea30ab3f5e1fa15ec6495c6cdbbac84634f8fc599e2c6d0b520",
        ),
        (
            0x80005760,
            0x34,
            "cd151a89b23615c187e8be4c846366965efb4c0b27ae46042618a1e6f6396981",
        ),
        (
            0x8005b3f0,
            0x121c,
            "2ff8ac48d473ff8c6adc3f7c20c2bdebf69d739211adff90660349171ef44269",
        ),
        (
            0x8005a504,
            0x864,
            "6cdc29f172439cca8ca358450b14d0f9e4c9028418599344efc71d8c052a6fef",
        ),
    ] {
        let start = offset(address);
        assert_eq!(digest(&dol[start..start + size]), expected, "{address:#x}");
    }
    for (address, expected) in [
        (0x801e79dc, 0x00640000),
        (0x801e79e0, 0x8004e224),
        (0x801e7724, 0x000c0000),
        (0x801e7728, 0x8005a504),
        (0x8004e254, 0x90a3000c),
        (0x8004e258, 0x90030010),
        (0x80005760, 0x2c030003), // full signed selector compared with3
        (0x80005764, 0x4180000c),
        (0x80005768, 0x806d83e4),
        (0x8000577c, 0x38043660), // the real windows have stride0x13660
        (0x8005b410, 0x807f000c),
        (0x8005b428, 0x801f0010),
        (0x801e881c, 0x8005b4d8),
        (0x801e8820, 0x8005b4fc),
        (0x8005b4d8, 0x4bfaa289),
        (0x8005b4e0, 0x88033542),
        (0x8005b4e4, 0x28000000), // closed requires status0
        (0x8005b4fc, 0x4bfaa265),
        (0x8005b504, 0x88033542),
        (0x8005b508, 0x28000005),
        (0x8005b50c, 0x418010ec),
        (0x8005a550, 0x2c030003),
        (0x8005a55c, 0x41800010),
        (0x8005a970, 0x80030230),
        (0x8005a974, 0x28000008),
        (0x8005a978, 0x418100d0), // grid index >8 selects default coordinates
        (0x8005aa48, 0xc02287d0),
        (0x8005aa50, 0xc00287d4),
        (0x8035b610, 320_f32.to_bits()),
        (0x8035b614, 240_f32.to_bits()),
        (0x8035b620, 318_f32.to_bits()),
    ] {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (input, pc, domain, value, native, symbol) in [
        (
            "fields/hot-spring-0089/scripts/scenario.ssb",
            0x24e0,
            "scenario::DialogueSlot",
            8,
            "yield_command",
            "scenario::DialogueSlot::MainFallback8",
        ),
        (
            "fields/iselia-human-ranch-0193/scripts/scenario.ssb",
            0x31e0,
            "scenario::DialogueSlot",
            20,
            "yield_command",
            "scenario::DialogueSlot::MainFallback20",
        ),
        (
            "fields/meltokio-0409/scripts/scenario.ssb",
            0xcc5,
            "scenario::DialogueGrid",
            200,
            "configure_dialogue",
            "scenario::DialogueGrid::DefaultCenter200",
        ),
    ] {
        let bytes = fs::read(root.join("all-assets").join(input)).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::Field).unwrap();
        assert!(result.identities.iter().any(|id| id.pc == pc
            && id.domain == domain
            && id.value == value
            && id.native == native
            && id.symbolic));
        assert_eq!(result.source.matches(symbol).count(), 1);
        assert_eq!(
            scenario::compile(input, &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable and Ymir scenarios at local"]
fn original_scene_model_handles_select_local_packages_and_keep_constructor_bytes() {
    use sha2::{Digest, Sha256};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let dol = fs::read(root.join("extracted/disc1/sys/main.dol")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&dol),
        "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
    );
    let word = |offset: usize| u32::from_be_bytes(dol[offset..offset + 4].try_into().unwrap());
    let offset = |address: u32| {
        (0..18)
            .find_map(|section| {
                let start = word(0x48 + section * 4);
                let size = word(0x90 + section * 4);
                (address >= start && address - start < size)
                    .then(|| word(section * 4) as usize + (address - start) as usize)
            })
            .unwrap()
    };
    // Full model resolver: FFEE resources take the current field MAP pointer,
    // narrow their local index to a byte and read section16+index. No generic
    // resource-loader, actor-key or motion-slot identity is implied.
    let start = offset(0x8005d138);
    assert_eq!(
        digest(&dol[start..start + 0x1a0]),
        "af096ca16b4c58c24ebc34e3c0199a13506d0b7b32f691b679883a31319cd4e6"
    );
    for (address, expected) in original_words::words::<u32>(
        "original_scene_model_handles_select_local_packages_and_keep_constructor_bytes",
    ) {
        assert_eq!(word(offset(address)), expected, "{address:#x}");
    }
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, expected) in [
        (
            519,
            &[(0, 3), (1, 3), (2, 2), (3, 1), (7, 3), (10, 6), (11, 2)][..],
        ),
        (
            520,
            &[
                (0, 4),
                (1, 2),
                (3, 1),
                (5, 1),
                (6, 3),
                (7, 11),
                (8, 10),
                (10, 10),
                (11, 2),
            ][..],
        ),
        (
            521,
            &[
                (0, 2),
                (1, 2),
                (2, 1),
                (5, 1),
                (7, 7),
                (8, 4),
                (9, 1),
                (10, 3),
                (11, 2),
            ][..],
        ),
    ] {
        let input = format!("fields/ymir-forest-{field:04}/scripts/scenario.ssb");
        let bytes = fs::read(root.join("all-assets").join(&input)).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        let domain = format!("resources::Field{field:04}ModelId");
        let uses = result
            .identities
            .iter()
            .filter(|id| id.domain == domain)
            .collect::<Vec<_>>();
        for &(index, count) in expected {
            let handle = 0xffee_0000u32 + index;
            assert_eq!(
                uses.iter().filter(|id| id.value == handle as i32).count(),
                count,
                "field{field} local model{index}"
            );
        }
        assert_eq!(
            uses.len(),
            expected.iter().map(|(_, count)| count).sum::<usize>()
        );
        assert!(uses.iter().all(|id| id.symbolic
            && matches!(
                id.native.as_str(),
                "spawn_actor"
                    | "spawn_interaction_actor"
                    | "create_scene_actor"
                    | "spawn_enemy_actor"
            )));
        assert_eq!(
            scenario::compile(&input, &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires the original Ymir event sound bank and scenarios at local"]
fn original_ymir_animal_sound_cues_keep_event_bank_programs_and_script_operands() {
    use sha2::{Digest, Sha256};
    use symphonia_constants::EffectId;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    let bank = fs::read(root.join("extracted/disc1/files/S/se_ev03.snd")).unwrap();
    let digest = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        digest(&bank),
        "3477f49469ae45fe93c8773a73e534ee9aa0a5e84161e79e94ae83bb3cc19332"
    );
    let word = |offset: usize| u32::from_be_bytes(bank[offset..offset + 4].try_into().unwrap());
    let half = |offset: usize| u16::from_be_bytes(bank[offset..offset + 2].try_into().unwrap());
    assert_eq!(word(0), 4);
    let descriptor = word(4) as usize;
    let sounds = descriptor + word(descriptor + 28) as usize;
    let pool = word(12) as usize;
    let object = |id: u16| {
        let mut cursor = pool + word(pool) as usize;
        loop {
            let stride = word(cursor);
            assert_ne!(stride, u32::MAX, "missing event program {id}");
            assert!(stride >= 8);
            if half(cursor + 4) == id {
                break &bank[cursor + 8..cursor + stride as usize];
            }
            cursor += stride as usize;
        }
    };
    for (cue, program, sample, length, hash, target) in [
        (
            EffectId::YmirBoarCall,
            663u16,
            308u16,
            96usize,
            "8ec4bd08767dda025c070fc4e3bdba1e4497c5e18c9015315d5a5e965acd3111",
            "cue-0456",
        ),
        (
            EffectId::YmirFishCall,
            666u16,
            309u16,
            56usize,
            "2e978fd9db993a8faad93c2f8da7996e6ef2d28eb0915b9b98af7d6f7e74fc1d",
            "cue-0457",
        ),
        (
            EffectId::YmirBoarMovement,
            662u16,
            308u16,
            104usize,
            "2f8627f8535856c7aba1324dcbae04005ed8b58f97b4ca98ef2b71c5128ff3af",
            "cue-0458",
        ),
        (
            EffectId::YmirBoarImpact,
            661u16,
            307u16,
            104usize,
            "1f41b8b63c6202db6e978305a4ea5a6a709ac271ab446bdc37201b7b1b58d303",
            "cue-0459",
        ),
        (
            EffectId::YmirBirdCall,
            668u16,
            310u16,
            56usize,
            "eec03ae46d3b93b74f753ac3a466ec01eb648b1a38429b3b5336d2ef9738d126",
            "cue-0460",
        ),
        (
            EffectId::YmirDefaultAnimalCall,
            667u16,
            309u16,
            112usize,
            "a75d1df38b28b54f60bcff79ca1cc204e645ffc8831c61a30240d7223e7d0118",
            "cue-0461",
        ),
    ] {
        let record = (0..usize::from(half(sounds)))
            .map(|index| sounds + 4 + index * 10)
            .find(|&offset| half(offset) == cue.raw())
            .unwrap();
        assert_eq!(half(record + 2), program);
        assert_eq!(&bank[record + 4..record + 10], &[255, 10, 127, 64, 60, 0]);
        let body = object(program);
        assert_eq!(body.len(), length);
        assert_eq!(digest(body), hash);
        let starts = body
            .as_chunks::<8>()
            .0
            .iter()
            .map(|command| u32::from_be_bytes(command[..4].try_into().unwrap()))
            .filter(|command| command & 0xff == 0x10)
            .collect::<Vec<_>>();
        assert_eq!(starts, [u32::from(sample) << 8 | 0x10]);
        assert_eq!(cue.resource_name(), Some(target));
    }

    // These are all the original script uses, including the tutorial's
    // default call, repeated boar sequences and pre-unlock call branches.
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, counts) in [
        (519, [3usize, 1, 4, 4, 3, 7]),
        (520, [4usize, 2, 8, 8, 2, 5]),
        (521, [2usize, 2, 2, 2, 3, 5]),
    ] {
        let input = format!("fields/ymir-forest-{field:04}/scripts/scenario.ssb");
        let bytes = fs::read(root.join("all-assets").join(&input)).unwrap();
        let result =
            scenario::decompile_with_context(&bytes, &definitions, Context::FieldScene(field))
                .unwrap();
        for (index, count) in counts.into_iter().enumerate() {
            let cue = 456 + index as i32;
            let uses = result
                .identities
                .iter()
                .filter(|id| id.domain == "effects::EffectId" && id.value == cue)
                .collect::<Vec<_>>();
            assert_eq!(uses.len(), count, "field{field} cue{cue}");
            assert!(uses.iter().all(|id| id.symbolic
                && id.native
                    == if cue == 458 {
                        "set_actor_ambient_sound"
                    } else {
                        "play_sound_simple"
                    }));
        }
        assert_eq!(
            scenario::compile(&input, &result.source, &definitions).unwrap(),
            bytes
        );
    }
}

#[test]
#[ignore = "requires original shared-route scenarios at local/all-assets"]
fn original_mana_leaf_route_uses_one_word_across_ymir_heimdall_latheon_and_earlier_warp_state() {
    use symphonia_constants::{GlobalId, ManaLeafHerbStage};
    assert_eq!(GlobalId::ManaLeafHerbRoute.raw(), 59);
    assert_eq!(0x40 + 4 * GlobalId::ManaLeafHerbRoute.raw(), 0x12c);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let definitions = Definitions::from_sources(&resonance_script_content::modules()).unwrap();
    for (field, code_count) in [
        ("ymir-forest-0519", 40),
        ("ymir-forest-0520", 41),
        ("ymir-forest-0521", 41),
        ("heimdall-0282", 4),
        ("heimdall-0289", 11),
        ("latheon-gorge-0492", 11),
        ("latheon-gorge-0495", 3),
        ("latheon-gorge-0496", 10),
        ("latheon-gorge-0497", 12),
        ("latheon-gorge-0499", 11),
        ("palmacosta-ranch-0205", 2),
        ("palmacosta-ranch-0206", 16),
    ] {
        let input = format!("fields/{field}/scripts/scenario.ssb");
        let bytes = fs::read(root.join(&input)).unwrap();
        let analysis = symphonia_script::scenario::analyze(&bytes).unwrap();
        let code_references = analysis
            .instructions
            .values()
            .filter(|i| i.mnemonic == "load.s32" && i.operands[0] == 0x12c)
            .count();
        assert_eq!(
            code_references, code_count,
            "{field} original code references"
        );
        let result = scenario::decompile_with_context(
            &bytes,
            &definitions,
            Context::from_path(Path::new(&input)),
        )
        .unwrap();
        if field.starts_with("palmacosta-ranch-") {
            assert!(
                result
                    .source
                    .contains("globals[story::PalmacostaGlobalId::WarpRoomState]")
            );
            // The earlier warp-room assignments reuse the later quest word.
            // Audit their actual operands rather than counting source spellings.
            for id in result
                .identities
                .iter()
                .filter(|id| id.domain == "story::PalmacostaGlobalId" && id.value == 59)
            {
                let (load, diagnostics) =
                    symphonia_script::scenario::decode_at(&bytes, id.pc).unwrap();
                assert!(diagnostics.is_empty());
                assert_eq!((load.mnemonic, load.operands), ("load.s32", vec![0x12c]));
            }
        } else {
            assert!(
                result
                    .source
                    .contains("globals[story::GlobalId::ManaLeafHerbRoute]")
            );
        }
        for id in result
            .identities
            .iter()
            .filter(|id| id.domain == "story::ManaLeafHerbStage" && [0, 7].contains(&id.value))
        {
            assert!(
                !id.symbolic,
                "earlier card masks must not acquire quest phases"
            );
        }
        assert_eq!(
            scenario::compile(&input, &result.source, &definitions).unwrap(),
            bytes
        );
    }
    assert_eq!(ManaLeafHerbStage::YmirFruitGivenToBoy.raw(), 3000);
    assert_eq!(ManaLeafHerbStage::ElfEldersStaffReceived.raw(), 5000);
    assert_eq!(ManaLeafHerbStage::ManaLeafHerbObtained.raw(), 13000);
    assert_eq!(
        ManaLeafHerbStage::StorytellerMithosHistoryHeard.raw(),
        14000
    );
    // Debug menu's numeric editor and message display refer to the same word.
    // Its original label is s_leaf, distinct from the adjacent Linkite word.
    let input = "fields/field-debug-menu/scripts/scenario.ssb";
    let bytes = fs::read(root.join(input)).unwrap();
    let result = scenario::decompile_with_context(&bytes, &definitions, Context::Field).unwrap();
    assert!(result.source.contains("sjis(\"s_leaf-----\")"));
    assert!(
        result.source.contains(
            "scenario::MessageControl::Number, globals[story::GlobalId::ManaLeafHerbRoute]"
        )
    );
    assert!(
        result
            .source
            .contains("initial: globals[story::GlobalId::ManaLeafHerbRoute]")
    );
    assert_eq!(
        scenario::compile(input, &result.source, &definitions).unwrap(),
        bytes
    );
}
