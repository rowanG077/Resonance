//! Bounded native evidence for the remaining unnamed scene flags.
//! Direct branches and selected table consumers do not prove the absence of
//! indexed, indirect or inlined accesses elsewhere in the original engine.
#[path = "support/original_words.rs"]
mod original_words;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

const UNRESOLVED: [u16; 9] = [152, 153, 155, 171, 191, 192, 193, 198, 199];

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

struct OriginalDol(Vec<u8>);

impl OriginalDol {
    fn load() -> Self {
        let bytes = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/sys/main.dol"),
        )
        .unwrap();
        assert_eq!(
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            "72dabc9590bb96796b0a89942b637c8f5106d86ddbed6587e689e6caaf136493"
        );
        Self(bytes)
    }

    fn offset(&self, address: u32) -> usize {
        (0..18)
            .find_map(|section| {
                let base = word(&self.0, 0x48 + section * 4);
                let size = word(&self.0, 0x90 + section * 4);
                (address >= base && address - base < size)
                    .then(|| word(&self.0, section * 4) as usize + (address - base) as usize)
            })
            .unwrap()
    }

    fn at(&self, address: u32) -> u32 {
        word(&self.0, self.offset(address))
    }
}

fn direct_branch(address: u32, instruction: u32) -> Option<u32> {
    let displacement = match instruction >> 26 {
        18 => ((instruction & 0x03ff_fffc) << 6) as i32 >> 6,
        16 => (instruction as i16 as i32) & !3,
        _ => return None,
    };
    Some(if instruction & 2 != 0 {
        displacement as u32
    } else {
        address.wrapping_add(displacement as u32)
    })
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_direct_flag_readers_have_only_two_nonliteral_arguments() {
    let dol = OriginalDol::load();
    let mut instructions = Vec::new();
    for section in 0..7 {
        let file = word(&dol.0, section * 4) as usize;
        let base = word(&dol.0, 0x48 + section * 4);
        let size = word(&dol.0, 0x90 + section * 4) as usize;
        for offset in (0..size).step_by(4) {
            instructions.push((base + offset as u32, word(&dol.0, file + offset)));
        }
    }
    let targets: BTreeSet<_> = instructions
        .iter()
        .filter_map(|&(address, instruction)| direct_branch(address, instruction))
        .collect();
    let mut literal_count = 0;
    let mut dynamic = BTreeSet::new();
    for (address, instruction) in instructions {
        if direct_branch(address, instruction) != Some(0x8008_0fd4) {
            continue;
        }
        assert_eq!(instruction >> 26, 18); // unconditional direct call
        assert_eq!(instruction & 1, 1); // link bit
        let previous = dol.at(address - 4);
        if previous >> 16 == 0x3860 {
            // li r3, flag immediately precedes the call, with no direct branch
            // bypassing it. Indirect entry at the call is outside this audit.
            assert!(!targets.contains(&address), "branch into call {address:#x}");
            let flag = previous as u16;
            assert!(!UNRESOLVED.contains(&flag), "literal flag {flag}");
            literal_count += 1;
        } else {
            dynamic.insert(address);
        }
    }
    assert_eq!(literal_count, 130);
    assert_eq!(dynamic, BTreeSet::from([0x8005_0cec, 0x800b_8a30]));
    // The constructor passes its retained fourth VM operand. Its opening
    // callback later sets that same actor+0x834 flag (see identity_evidence).
    assert_eq!(dol.at(0x8005_0c6c), 0x7fc3_f378); // mr r3,r30
    assert_eq!(dol.at(0x8005_0d44), 0x93df_0834); // stw r30,0x834(r31)
    assert_eq!(dol.at(0x8000_e4f0), 0x807f_0834); // lwz r3,0x834(r31)
    assert_eq!(
        direct_branch(0x8000_e4f4, dol.at(0x8000_e4f4)),
        Some(0x8008_0f98)
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_manual_and_restore_loops_use_other_flag_ranges() {
    let dol = OriginalDol::load();
    // Nine chapters, five-byte flag rows, and a separate count per chapter.
    // The only byte-table argument to the pure flag reader is a manual topic.
    for (address, expected) in
        original_words::words::<u32>("original_manual_and_restore_loops_use_other_flag_ranges")
    {
        assert_eq!(dol.at(address), expected, "DOL {address:#x}");
    }
    let counts = dol.offset(0x8019_d808);
    assert_eq!(&dol.0[counts..counts + 9], &[5, 3, 2, 2, 5, 2, 5, 4, 2]);
    let flags = dol.offset(0x8019_d814);
    let mut topics = BTreeSet::new();
    for chapter in 0..9 {
        for topic in 0..usize::from(dol.0[counts + chapter]) {
            topics.insert(u16::from(dol.0[flags + chapter * 5 + topic]));
        }
    }
    assert_eq!(topics, (104..=133).collect());
    assert!(UNRESOLVED.iter().all(|flag| {
        !topics.contains(flag) && !(850..899).contains(flag) && !(900..1000).contains(flag)
    }));
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_additional_flag_reader_offsets_its_index_by_nine_hundred() {
    let dol = OriginalDol::load();
    let start = dol.offset(0x8008_10d4);
    assert_eq!(
        Sha256::digest(&dol.0[start..start + 104])
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "eb0bb9785ab947f06f9936452098eb043f449971df51679345650e9a7288bdc2"
    );
    for (address, expected) in original_words::words::<u32>(
        "original_additional_flag_reader_offsets_its_index_by_nine_hundred",
    ) {
        assert_eq!(dol.at(address), expected, "DOL {address:#x}");
    }
    // Every input that could select one of these bytes is negative before
    // add900, so the first guard rejects it. Large nonnegative inputs can
    // overflow add900 into negative indices; do not describe940 as a complete
    // accepted-input bound or treat this as a general memory-safety claim.
    for flag in UNRESOLVED {
        assert!(i32::from(flag) - 900 < 0);
    }
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_displacement_memory_operands_do_not_overlap_unresolved_flag_bytes() {
    let dol = OriginalDol::load();
    let watched: BTreeSet<_> = UNRESOLVED
        .iter()
        .map(|flag| 0xc9d + i32::from(flag / 8))
        .collect();
    assert_eq!(watched, BTreeSet::from([0xcb0, 0xcb2, 0xcb4, 0xcb5]));
    let mut checked = 0;
    for section in 0..7 {
        let file = word(&dol.0, section * 4) as usize;
        let base = word(&dol.0, 0x48 + section * 4);
        let size = word(&dol.0, 0x90 + section * 4) as usize;
        for offset in (0..size).step_by(4) {
            let instruction = word(&dol.0, file + offset);
            let width = match instruction >> 26 {
                32 | 33 | 36 | 37 | 48 | 49 | 52 | 53 => 4,
                34 | 35 | 38 | 39 => 1,
                40..=45 => 2,
                46 | 47 => 4 * (32 - ((instruction >> 21) & 31)) as i32,
                50 | 51 | 54 | 55 => 8,
                _ => continue,
            };
            let displacement = instruction as i16 as i32;
            assert!(
                watched
                    .iter()
                    .all(|&byte| !(displacement..displacement + width).contains(&byte)),
                "D-form memory span overlaps a watched save offset at {:#x}",
                base + offset as u32
            );
            checked += 1;
        }
    }
    assert!(checked > 100_000);
    // This excludes literal byte/halfword/word/doubleword and multiple-register
    // spans at those displacements, regardless of base register. It cannot
    // exclude indexed operations, adjusted bases, bulk copies or indirect calls.
}

#[test]
fn canonical_argument_audit_sees_retained_setters_without_executing_operands_or_auxiliary() {
    use symphonia_script::{
        NativeCall, Program,
        scenario::{self, calls},
    };
    let assemble = |extra: &str| {
        scenario::assemble(&format!(
        ".code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\nend\n{extra}push.s16 155\ncalc 0\narg\nproc 104\nend\n"
    )).unwrap()
    };
    let seed = assemble("");
    let pc = *canonical_native_pcs(&seed, 0x68).first().unwrap();
    let (instruction, _) = scenario::decode_at(&seed, pc).unwrap();
    let native_word = u16::from_be_bytes(
        seed[instruction.offset..instruction.offset + 2]
            .try_into()
            .unwrap(),
    );
    let mut bytes = assemble(&format!("push.s32 {native_word}\ncalc 0\n"));
    let auxiliary_words = (bytes.len() / 2) as u16;
    bytes[4..6].copy_from_slice(&auxiliary_words.to_be_bytes());
    bytes.extend_from_slice(&native_word.to_be_bytes());
    let program = Program::decode(&bytes).unwrap();
    assert!(
        calls::arguments(&bytes, NativeCall::SetEventBit, 1)
            .unwrap()
            .is_empty()
    );
    let analysis = canonical_analysis(&bytes);
    let setters = calls::decoded_arguments(&analysis, NativeCall::SetEventBit, 1).unwrap();
    assert_eq!(setters.len(), 1);
    let (&pc, arguments) = setters.first_key_value().unwrap();
    assert_eq!(arguments, &[Some(155)]);
    assert_eq!(canonical_native_pcs(&bytes, 0x68), BTreeSet::from([pc]));
    assert!(
        program.instruction(pc).is_none(),
        "audit roots must not become runtime entries"
    );
}

#[test]
#[ignore = "requires the original GQSEAF executable and supported REL files at local/extracted"]
fn original_literal_story_bank_base_sites_have_only_reviewed_consumers() {
    let dol = OriginalDol::load();
    let eligible = |instruction: u32| {
        instruction & 0xffff == 0xc9d
            && matches!(instruction >> 26, 7 | 8 | 12..=15 | 24..=29 | 32..=55)
    };
    let mut found = Vec::new();
    for section in 0..7 {
        let file = word(&dol.0, section * 4) as usize;
        let base = word(&dol.0, 0x48 + section * 4);
        let size = word(&dol.0, 0x90 + section * 4) as usize;
        for offset in (0..size).step_by(4) {
            let instruction = word(&dol.0, file + offset);
            if eligible(instruction) {
                found.push((base + offset as u32, instruction));
            }
        }
    }
    assert_eq!(
        found,
        [
            (0x800447c8, 0x38840c9d),
            (0x800449c8, 0x38840c9d),
            (0x8004dd28, 0x88e70c9d),
            (0x8004ddc8, 0x88a60c9d),
            (0x8004ddd8, 0x98060c9d),
            (0x8004de24, 0x88a60c9d),
            (0x8004de34, 0x98060c9d),
            (0x80080f80, 0x88a60c9d),
            (0x80080f90, 0x98060c9d),
            (0x80080fbc, 0x88a60c9d),
            (0x80080fcc, 0x98060c9d),
            (0x80080ff8, 0x88a50c9d),
            (0x8008111c, 0x88a50c9d),
        ]
    );
    for (file, expected_hash) in [
        (
            "US_r_Top2field.rel",
            "680027057d07cdab9de63e40cb7bacd945572b4f2df11b55ca67dc5e64ac28f8",
        ),
        (
            "US_r_Top2Btl.rel",
            "b2acfb222246fbbecf5ab8025fb08241c736da65031104fd4be9c51c301df214",
        ),
    ] {
        let bytes = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/extracted/disc1/files")
                .join(file),
        )
        .unwrap();
        assert_eq!(digest(&bytes), expected_hash);
        let table = word(&bytes, 0x10) as usize;
        for section in 0..word(&bytes, 0xc) as usize {
            let raw = word(&bytes, table + section * 8);
            if raw & 1 == 0 {
                continue;
            }
            let offset = (raw & !3) as usize;
            let size = word(&bytes, table + section * 8 + 4) as usize;
            for at in (0..size).step_by(4) {
                assert!(
                    !eligible(word(&bytes, offset + at)),
                    "{file} section{section} offset{at:#x}"
                );
            }
        }
    }
    // Literal arithmetic/logical-immediate and D-form memory operands only. This
    // does not exclude adjusted/indexed pointers, bulk copies or indirect calls.
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_reviewed_incoming_images_have_no_new_setters_for_unresolved_scene_flags() {
    use symphonia_script::{NativeCall, Program, scenario::calls};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets");
    let watched = |destination| match destination {
        138 => &[199][..],
        139 => &[198],
        254 => &[152, 153],
        317 => &[171],
        482 => &[191, 192, 193],
        _ => &[],
    };
    let mut total = 0;
    let mut transfers = 0;
    for (input, hash, count, incoming) in [
        (
            "fields/altessas-house-0138/scripts/scenario.ssb",
            "3e5dd6e76bed2167ac23e1f3195f5b00c73883df54ca9552339ed1a0100ced7d",
            7,
            &[(0x42A, 139), (0x83BD, 139), (0x8513, 139)][..],
        ),
        (
            "fields/altessas-house-0139/scripts/scenario.ssb",
            "5fdc7cf839a080b646150f7c08b9467553db1188d8bdb89cf933dcb0fa23137a",
            19,
            &[
                (0x73A, 138),
                (0x1782, 138),
                (0x34EB, 138),
                (0x9B5C, 138),
                (0xA55D, 139),
                (0xAC3F, 139),
            ][..],
        ),
        (
            "fields/altessas-house-0142/scripts/scenario.ssb",
            "a044c13a07646261678d42def8dd6780f137b3a9fc11a0b4cce552ed5793cfdd",
            3,
            &[(0x2F1, 139), (0x25D7, 139)][..],
        ),
        (
            "fields/altessas-house-0143/scripts/scenario.ssb",
            "b3fe840a47a0249e93a226087943a4d822b8cda0352c1205a9e7e613bd29c846",
            4,
            &[(0x7560, 139), (0x7A36, 139), (0x7D7B, 139)][..],
        ),
        (
            "fields/flanoir-0310/scripts/scenario.ssb",
            "fbb7754732de701c288943ddb01241960c4551f4b47be108d2c3ecd2ad305ced",
            20,
            &[(0x14C6, 138)][..],
        ),
        (
            "fields/flanoir-0313/scripts/scenario.ssb",
            "4414894a2fdf7522b11881a02fef9dede86defd2969b3ba0a7b9ccf9b27ad905",
            1,
            &[
                (0x1285, 317),
                (0x25AB, 317),
                (0x3517, 317),
                (0x4556, 317),
                (0x5624, 317),
                (0x66A8, 317),
                (0x7428, 317),
                (0x8566, 317),
            ][..],
        ),
        (
            "fields/flanoir-0316/scripts/scenario.ssb",
            "f2838bc82d8ecebc2aba5feffeef38c78b6b859fd06f5265dc81faf56d222886",
            1,
            &[(0x1AF, 317)][..],
        ),
        (
            "fields/flanoir-0322/scripts/scenario.ssb",
            "f81ce996520612a31d57423c47c39915a84106bcf02e2e81a06b60c30b7fe1a2",
            27,
            &[(0x17AB, 317)][..],
        ),
        (
            "fields/fooji-mountains-0416/scripts/scenario.ssb",
            "8336f865f1c58f17b5b2ca1f59c651adcc48f1feca1118d7f4dff90c81256229",
            3,
            &[(0x8BB0, 139)][..],
        ),
        (
            "fields/heimdall-0481/scripts/scenario.ssb",
            "5fdffe5c045146f566d2ce9a84da946f3e31ed43a4c1f0f721154bc771c79722",
            1,
            &[(0x2F7, 482)][..],
        ),
        (
            "fields/heimdall-0483/scripts/scenario.ssb",
            "942638934afef4263140a45e65dffdde8508d1ee60be9294693723ef7159902f",
            0,
            &[(0x229, 482)][..],
        ),
        (
            "fields/iselia-0344/scripts/scenario.ssb",
            "e5c4ec73c8650166980f4a748c175fdf301ba9873a1e9fb0fbb38da1d3303722",
            8,
            &[(0x40E, 139)][..],
        ),
        (
            "fields/sylvarant-base-0276/scripts/scenario.ssb",
            "f96bebf28333086ac60f18d177cfb91cde425f44d56b1874a91ed580a2063d7c",
            2,
            &[(0x41E6, 139)][..],
        ),
        (
            "fields/sylvarant-base-0281/scripts/scenario.ssb",
            "d3f9390e2016401ec087e2a247771e4cb4682c028d9592ab49aa77fa3647a156",
            0,
            &[(0x1136, 138)][..],
        ),
        (
            "fields/tethealla-base-0260/scripts/scenario.ssb",
            "c93ed69ad940a6c3d2d3e6582ae60efe85d8150d74b29eede77de1eea4df316b",
            1,
            &[(0x36D, 254)][..],
        ),
        (
            "fields/tethealla-base-0266/scripts/scenario.ssb",
            "f2f61e0548c7e27d598e2b2f9257490a93ac81b5730580fc0c4669c35429b7b5",
            1,
            &[(0x16B, 254)][..],
        ),
        (
            "fields/toize-valley-mine-0414/scripts/scenario.ssb",
            "ce6349fd9be0aeb047cc428c07c8352deca0dce62930de645d0899b7ac73bc5b",
            24,
            &[(0x4D01, 139)][..],
        ),
        (
            "fields/toize-valley-mine-0415/scripts/scenario.ssb",
            "72b0eb45d8dd373da79b0848763808650db129897bfda384f3355f0095c7116e",
            3,
            &[(0x3581, 139)][..],
        ),
        (
            "world/scripts/scenario.ssb",
            "4de7a30a3d53ef69e9f9131589a8ed37b8809595091e724f38eaf3e6afb4455b",
            71,
            &[(0x1ABE, 138)][..],
        ),
    ] {
        let bytes = fs::read(root.join(input)).unwrap();
        assert_eq!(digest(&bytes), hash);
        Program::decode(&bytes).unwrap();
        let analysis = canonical_analysis(&bytes);
        let setters = calls::decoded_arguments(&analysis, NativeCall::SetEventBit, 1).unwrap();
        assert_eq!(setters.len(), count, "{input}");
        assert_eq!(
            setters.keys().copied().collect::<BTreeSet<_>>(),
            canonical_native_pcs(&bytes, 0x68),
            "{input}"
        );
        assert!(
            setters.values().all(|a| a[0].is_some()),
            "unresolved setter in {input}"
        );
        let changes = calls::decoded_arguments(&analysis, NativeCall::ChangeField, 5).unwrap();
        let actual: Vec<_> = changes
            .iter()
            .filter_map(|(pc, a)| {
                a[0].filter(|dest| !watched(*dest).is_empty())
                    .map(|dest| (*pc, dest))
            })
            .collect();
        assert_eq!(actual, incoming, "{input}");
        let relevant: BTreeSet<_> = incoming
            .iter()
            .flat_map(|(_, dest)| watched(*dest).iter().copied())
            .collect();
        let candidates: Vec<_> = setters
            .iter()
            .filter_map(|(pc, a)| {
                a[0].filter(|value| relevant.contains(value))
                    .map(|value| (*pc, value))
            })
            .collect();
        if input == "world/scripts/scenario.ssb" {
            // These two existing writers belong to Hima and caravan callbacks,
            // not a recovered set-state meaning for Altessa's exit clear.
            assert_eq!(candidates, [(0x8f6, 199), (0x1444, 199)]);
        } else {
            assert!(candidates.is_empty(), "{input}: {candidates:?}");
        }
        if input.contains("altessas-house-0138/") {
            assert_eq!(setters.get(&0x84ff), Some(&vec![Some(1089)]));
            assert_eq!(setters.get(&0x87be), Some(&vec![Some(1091)]));
        }
        if input.contains("toize-valley-mine-0415/") {
            assert_eq!(setters.get(&0x3550), Some(&vec![Some(1063)]));
        }
        total += count;
        transfers += incoming.len();
    }
    assert_eq!(total, 196);
    assert_eq!(transfers, 36);
    // Reviewed image set only: no inference about transitive/dynamic incoming
    // edges, embedded message setters, indexed/bulk native writes or liveness.
}

#[test]
#[ignore = "requires the original cooked .ssb corpus at local/all-assets"]
fn original_reviewed_salvation_hub_predecessors_do_not_set_flag_one_fifty_five() {
    use symphonia_script::{NativeCall, scenario::calls};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets/fields");
    for (field, hash, setter_count, incoming_pcs) in [
        (
            "tower-of-salvation-0501",
            "c3b0a88257fe5786710a40afdcb4d6b57236500e77b87fb9415db4aecb7ef0f5",
            4,
            &[0xc1du32][..],
        ),
        (
            "tower-of-salvation-0503",
            "31950007adc60b5ff9f2875cd59035acf4a75975a0ee274862f26b49f9a99c38",
            8,
            &[0x10c8][..],
        ),
        (
            "derris-kharlan-0530",
            "4af7a3d58b01357e8bb60397cdef4c00ed8f58099236603529174aedbde4b0df",
            25,
            &[0xc13, 0xe5b][..],
        ),
        (
            "field-0238",
            "f7c02e8ea7ee89621de283709c6e682c99a19557dda7536e5c2c1fe7f0773d75",
            16,
            &[0x192, 0x1de][..],
        ),
    ] {
        let bytes = fs::read(root.join(field).join("scripts/scenario.ssb")).unwrap();
        assert_eq!(digest(&bytes), hash, "{field}");
        let analysis = canonical_analysis(&bytes);
        let setters = calls::decoded_arguments(&analysis, NativeCall::SetEventBit, 1).unwrap();
        let all_setters = canonical_native_pcs(&bytes, NativeCall::SetEventBit as u8);
        assert_eq!(
            setters.keys().copied().collect::<BTreeSet<_>>(),
            all_setters
        );
        assert_eq!(setters.len(), setter_count, "{field}");
        assert!(
            setters
                .values()
                .all(|a| a[0].is_some() && a[0] != Some(155)),
            "{field}: {setters:?}"
        );
        let transfers = calls::decoded_arguments(&analysis, NativeCall::ChangeField, 5).unwrap();
        let incoming: Vec<_> = transfers
            .iter()
            .filter(|(_, a)| a[0] == Some(502))
            .map(|(pc, _)| *pc)
            .collect();
        assert_eq!(incoming, incoming_pcs, "{field}");
    }
    // This covers direct setters in these four decoded code images, including
    // retained routines. It does not cover transitive predecessors, dynamic
    // destinations in other images, embedded messages or native callbacks.
}

// Independently census canonical encodings in the retained code view, rather
// than comparing two analyses with the same reachable roots. This is the same
// bounded sweep used for readable publication: avoid registry overlays, known
// instruction operands, overlaps, diagnostics and auxiliary data. It does not
// prove that a retained routine can execute in normal play.
fn canonical_analysis(bytes: &[u8]) -> symphonia_script::scenario::Analysis {
    use symphonia_script::scenario;
    let mut analysis = scenario::analyze(bytes).unwrap();
    let base = analysis.header.code_base();
    let mut offset = base.max(8 + analysis.records.len() * 12);
    while offset + 2 <= analysis.code_end() {
        let pc = ((offset - base) / 2) as u32;
        if let Some(i) = analysis.instructions.get(&pc) {
            offset += i.size_words as usize * 2;
            continue;
        }
        if analysis
            .instructions
            .range(..pc)
            .next_back()
            .is_some_and(|(_, i)| pc < i.end_pc())
        {
            offset += 2;
            continue;
        }
        let (i, diagnostics) = scenario::decode_at(bytes, pc).unwrap();
        let end = i.offset + i.size_words as usize * 2;
        let overlaps = analysis.instructions.range(pc..i.end_pc()).next().is_some();
        if !i.raw && diagnostics.is_empty() && !overlaps && end <= analysis.code_end() {
            offset = end;
            analysis.instructions.insert(pc, i);
        } else {
            offset += 2;
        }
    }
    // Each disconnected retained linear run has an audit entry. Keep these
    // roots separate from Program::decode's actual runtime entry registry.
    let mut previous: Option<&scenario::Instruction> = None;
    let mut entries = Vec::new();
    for i in analysis.instructions.values() {
        if previous.is_none_or(|before| {
            before.end_pc() != i.pc || before.control != scenario::Control::Fallthrough
        }) {
            entries.push(i.pc);
        }
        previous = Some(i);
    }
    analysis.roots.extend(entries);
    analysis
}

fn canonical_native_pcs(bytes: &[u8], opcode: u8) -> BTreeSet<u32> {
    canonical_analysis(bytes)
        .instructions
        .values()
        .filter(|i| i.mnemonic == "proc" && i.operands == [i64::from(opcode)])
        .map(|i| i.pc)
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Relocation {
    module: u32,
    source: u8,
    offset: u32,
    kind: u8,
    target: u8,
    addend: u32,
}

fn relocations(bytes: &[u8]) -> Vec<Relocation> {
    let mut result = Vec::new();
    for import in
        (word(bytes, 0x28) as usize..(word(bytes, 0x28) + word(bytes, 0x2c)) as usize).step_by(8)
    {
        let module = word(bytes, import);
        let mut cursor = word(bytes, import + 4) as usize;
        let (mut source, mut offset) = (0, 0);
        loop {
            let delta = u16::from_be_bytes(bytes[cursor..cursor + 2].try_into().unwrap());
            let (kind, target, addend) = (
                bytes[cursor + 2],
                bytes[cursor + 3],
                word(bytes, cursor + 4),
            );
            cursor += 8;
            if kind == 203 {
                break;
            }
            if kind == 202 {
                source = target;
                offset = 0;
                continue;
            }
            offset += u32::from(delta);
            result.push(Relocation {
                module,
                source,
                offset,
                kind,
                target,
                addend,
            });
        }
    }
    result
}

#[test]
#[ignore = "requires the original GQSEAF executable and field REL at local/extracted"]
fn original_field_change_resolves_the_registered_rel_callback() {
    let dol = OriginalDol::load();
    for (address, expected) in
        original_words::words::<u32>("original_field_change_resolves_the_registered_rel_callback")
    {
        assert_eq!(dol.at(address), expected, "{address:#x}");
    }
    let rel = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/US_r_Top2field.rel"),
    )
    .unwrap();
    assert_eq!(
        digest(&rel),
        "680027057d07cdab9de63e40cb7bacd945572b4f2df11b55ca67dc5e64ac28f8"
    );
    assert_eq!(word(&rel, 0), 2);
    let text = word(&rel, word(&rel, 0x10) as usize + 16) as usize & !3;
    assert_eq!(
        digest(&rel[text + 0x1c008..text + 0x1c194]),
        "9af1a2b2b964878920fd3bdcc599465a14063feb31ae4a899c2b04e2aa0bccb0"
    );
    assert_eq!(
        digest(&rel[text + 0x2bc7c..text + 0x2bcbc]),
        "f7e494cf7b158251f2e9189fb76c68b1a7c6c7113ed66efa31f72bc3d9cae4b8"
    );
    let at = |offset: usize| word(&rel, text + offset);
    for (offset, expected) in original_words::words::<usize>(
        "original_field_change_resolves_the_registered_rel_callback-2",
    ) {
        assert_eq!(at(offset), expected);
    }
    let relocations = relocations(&rel);
    for (module, offset, kind, target, addend) in [
        (2, 0x2bc86, 6, 2, 0x1c008),
        (2, 0x2bca2, 4, 2, 0x1c008),
        (0, 0x2bc9e, 6, 9, 0x8035a508),
        (0, 0x2bcb6, 4, 9, 0x8035a508),
        (0, 0x1c050, 10, 2, 0x800d6bc8),
        (0, 0x1c0a0, 10, 2, 0x801133b4),
        (0, 0x1c16c, 10, 2, 0x800edb24),
        (0, 0x1c17c, 10, 2, 0x800d7e9c),
    ] {
        assert!(relocations.contains(&Relocation {
            module,
            source: 2,
            offset,
            kind,
            target,
            addend
        }));
    }
    // Resolve the indirect edge instead of treating the callback as absent.
    // The callback's callees and later loading are outside this bounded check;
    // it establishes neither blanket flag clearing nor blanket preservation.
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_callback_marks_caption_history_in_separate_save_banks() {
    let dol = OriginalDol::load();
    let start = dol.offset(0x800e_db24);
    assert_eq!(
        digest(&dol.0[start..start + 220]),
        "f40ca71843967be26f57e0538fc5b4002e0c05afba768cf7779a68ef8429e784"
    );
    for (address, expected) in original_words::words::<u32>(
        "original_callback_marks_caption_history_in_separate_save_banks",
    ) {
        assert_eq!(dol.at(address), expected, "{address:#x}");
    }
    // The two 128-bit caption banks are not the temporary story-flag bytes.
    // This leaf does not clear them; the other callback callees and later
    // loading remain separate work rather than an assumed blanket policy.
    let stores: Vec<_> = (0x800e_db24..0x800e_dc00)
        .step_by(4)
        .filter(|&at| matches!(dol.at(at) >> 26, 36..=39 | 44..=45 | 47 | 52..=55))
        .collect();
    assert_eq!(stores, [0x800e_db34, 0x800e_db9c, 0x800e_dbf8]);
}

#[test]
#[ignore = "requires the original GQSEAF executable at local/extracted"]
fn original_caption_overlay_key_selects_its_renderer_without_aliasing_resources() {
    let dol = OriginalDol::load();
    for (address, length, hash) in [
        (
            0x8004_1ba4,
            624,
            "dde160ff22ecd8752dd6a776bf70a970dbfcc277e8655ca36c38e6512ab23b3f",
        ),
        (
            0x8007_e1bc,
            4152,
            "a358b744173f04278f2a7168634823a241494665fd20dbe4d5a42665bfb0b364",
        ),
    ] {
        let start = dol.offset(address);
        assert_eq!(digest(&dol.0[start..start + length]), hash);
    }
    for (address, expected) in original_words::words::<u32>(
        "original_caption_overlay_key_selects_its_renderer_without_aliasing_resources",
    ) {
        assert_eq!(dol.at(address), expected, "{address:#x}");
    }
    assert_eq!(
        symphonia_constants::OverlayActorId::LocationCaption.raw(),
        999989
    );
}
