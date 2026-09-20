//! Recover the shared defensive arte and its character-specific entry feedback.
use super::*;
use resonance_content::battle::contact_effects::ContactEffects;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    feedback: ContactEffects,
    entries: [Entry; 9],
    sound: u16,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    caption: String,
    effect: u8,
    voice: u16,
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<MartialCallback> {
    ensure!(
        variant == 0
            && source.phases[0].duration == 60
            && source.phases[0].recovery_ticks == 10
            && source.phases[0].buffer_until == 60
            && source.phases[0].combo_at == 45
            && (1..4).all(|phase| source.phases[phase].duration == 0),
        "changed Magic Guard authored phases"
    );
    Ok(MartialCallback::MagicGuard {
        contact_effects: p.feedback.elemental,
        contact_colors: p.feedback.flash,
    })
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0xd60 + 34 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x6413c)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "changed Magic Guard dispatch"
    );
    let code = rel.at((1, 0x6413c))?;
    ensure!(
        INITIALIZER
            .iter()
            .enumerate()
            .all(|(i, &word)| super::word(code, i * 4).ok() == Some(word)),
        "unreviewed Magic Guard initializer"
    );
    ensure!(
        (0..9).all(|character| rel
            .at((DATA, 0x3d30 + character * 0x1f0))
            .is_ok_and(|row| row[0x9e] == 0)),
        "unimplemented party-specific guard impact"
    );
    let mut feedback = super::super::super::contact_effects::cook(rel)?;
    // The defensive arte's neutral shield flash is selected directly.
    feedback.elemental[0] = 1;
    let entries = (0..9)
        .map(|variant| -> Result<Entry> {
            let name = rel.at(rel.pointer(4, 0x3b7c + variant * 4)?)?;
            let end = name
                .iter()
                .position(|&byte| byte == 0)
                .context("unterminated guard caption")?;
            Ok(Entry {
                caption: std::str::from_utf8(&name[..end])?.to_owned(),
                effect: rel.at((4, 0x3ba0))?[variant],
                voice: half(rel.at((4, 0x3bac))?, variant * 2)?,
            })
        })
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .ok()
        .context("expected nine guard entries")?;
    Ok(Parameters {
        feedback,
        entries,
        sound: 69,
    })
}

pub(super) fn expand(
    p: &Parameters,
    native: u16,
    variants: &mut Vec<TechniquePhase>,
) -> Result<()> {
    if native != 34 {
        return Ok(());
    }
    ensure!(
        variants.len() == 1
            && variants[0].variant == 0
            && variants[0].action.hits.is_empty()
            && variants[0].action.commands.is_empty()
            && !variants[0].action.loop_commands,
        "unreviewed Magic Guard tracks"
    );
    let phase = variants.pop().unwrap();
    for (variant, feedback) in p.entries.iter().enumerate() {
        let mut entry = phase.clone();
        entry.variant = variant as u8;
        entry.caption = Some(feedback.caption.clone());
        entry.effect = Some(u16::from(feedback.effect));
        entry.action.commands = vec![
            TimedCommand {
                tick: 0,
                command: ActionCommand::Sound(p.sound),
            },
            TimedCommand {
                tick: 0,
                command: ActionCommand::Voice {
                    id: feedback.voice,
                    priority: 2,
                },
            },
        ];
        variants.push(entry);
    }
    Ok(())
}

const INITIALIZER: &[u32] = &[
    0x9421ff70, 0x7c0802a6, 0x3c800000, 0x3ca00000, 0x90010094, 0x38c40000, 0x3c800000, 0xbec10068,
    0x39240000, 0x7c7f1b78, 0x3880000a, 0x85850000, 0x82c60000, 0x82e60004, 0x83060008, 0x8326000c,
    0x83460010, 0x83660014, 0x83860018, 0x83c6001c, 0x83a60020, 0x81650004, 0x89450008, 0x81090000,
    0x80e90004, 0x80c90008, 0x80a9000c, 0xa0090010, 0x92c10044, 0x92e10048, 0x9301004c, 0x93210050,
    0x93410054, 0x93610058, 0x9381005c, 0x93c10060, 0x93a10064, 0x91810024, 0x91610028, 0x9941002c,
    0x91010030, 0x90e10034, 0x90c10038, 0x90a1003c, 0xb0010040, 0x4bfb99fd, 0xb07f1556, 0x7fe3fb78,
    0xa89f1556, 0x4bfd3691, 0xb07f1556, 0x7fe3fb78, 0x4bfd4031, 0x881f107e, 0x38810040, 0x7fe3fb78,
    0x38a00000, 0x5400f6ba, 0x7c84002e, 0x4bfb6b79, 0x809f18f0, 0x3bc00005, 0x807f18f4, 0x39800041,
    0x39600000, 0x381f02c0, 0x909f1944, 0x38a10023, 0x7fe6fb78, 0x7fe7fb78, 0x907f1948, 0x38610018,
    0x391f195c, 0x38800001, 0x83bf18f8, 0x39200000, 0x39400000, 0x93bf194c, 0x9bdf01b0, 0x999f0282,
    0x83df195c, 0x819f1960, 0x93c10018, 0x9181001c, 0x819f1964, 0x91810020, 0x91610008, 0x9161000c,
    0x90010010, 0x881f107e, 0x817f0004, 0x5400e73e, 0xc03f18d0, 0x7ca500ae, 0xc04b008c, 0x4bfdbf59,
    0x387f1a2c, 0x38800045, 0x38a00001, 0x4bfa5b71, 0x881f107e, 0x3881002e, 0x7fe3fb78, 0x38a00000,
    0x5400eefc, 0x38c00002, 0x7c84022e, 0x38e00002, 0x4800db8d, 0xbac10068, 0x80010094, 0x7c0803a6,
    0x38210090, 0x4e800020,
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_ex_immunity_roll_order_notice_and_presea_equipped_requirements() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        // EX30 OR EX85 uses one unsigned percent draw, then notice9/45 and bit4,
        // before EX164, shield resolution and the later Unison/HP branches.
        for (address, expected) in [
            (0x62968, 0x7ee3bb78),
            (0x6296c, 0x3880001e),
            (0x62970, 0x4bfb9efd),
            (0x62974, 0x2c030000),
            (0x62978, 0x40820018),
            (0x6297c, 0x7ee3bb78),
            (0x62980, 0x38800055),
            (0x62984, 0x4bfb9dc9),
            (0x62988, 0x2c030000),
            (0x6298c, 0x41820044),
            (0x62990, 0x4bfeb4b1),
            (0x62994, 0x3c8051ec),
            (0x62998, 0x3804851f),
            (0x6299c, 0x7c001816),
            (0x629a0, 0x5400d97e),
            (0x629a4, 0x1c000064),
            (0x629a8, 0x7c001850),
            (0x629ac, 0x28000005),
            (0x629b0, 0x40800020),
            (0x629b4, 0x7ee3bb78),
            (0x629b8, 0x38800009),
            (0x629bc, 0x38a0002d),
            (0x629c0, 0x38c00000),
            (0x629c4, 0x38e00000),
            (0x629c8, 0x4bfbf011),
            (0x629cc, 0x639c0004),
            (0x629d4, 0x388000a4),
            (0x62ac8, 0x73800026),
            (0x62f2c, 0x3800ffc9),
            (0x62f30, 0x7f9c0038),
            (0x62f7c, 0x4bfba8e9),
            (0x62f84, 0x73800104),
            // Notice storage: authored hold, pop16, opacity255 and fade0.
            (0x21a44, 0xb0bf0018),
            (0x21a48, 0x38a00010),
            (0x21a50, 0x388000ff),
            (0x21a58, 0x38000000),
            (0x21a60, 0xb09f001a),
            (0x21a68, 0xb01f001c),
            // Hold decrements first; the following32 updates subtract8 opacity.
            (0x65f28, 0xa8030260),
            (0x65f30, 0x41820054),
            (0x65f74, 0xa8830260),
            (0x65f78, 0x3804ffff),
            (0x65f7c, 0xb0030260),
            (0x65f80, 0x4800003c),
            (0x65f90, 0xa8830264),
            (0x65f94, 0x38040001),
            (0x65f98, 0xb0030264),
            (0x65f9c, 0xa8830262),
            (0x65fa0, 0x3804fff8),
            (0x65fa4, 0xb0030262),
            (0x65fac, 0x2c000000),
            (0x65fb0, 0x4181000c),
            (0x65fb4, 0x38000000),
            (0x65fb8, 0xb0030262),
        ] {
            assert_eq!(
                word(rel.at((1, address)).unwrap(), 0).unwrap(),
                expected,
                "source {address:#x}"
            );
        }
        use resonance_content::battle::ui::NoticeKind;
        let index = NoticeKind::ALL
            .iter()
            .position(|&kind| kind == NoticeKind::ExSkillEffect)
            .unwrap();
        assert_eq!(index, 8);
        for (column, expected) in ["EX SKILL", "EFFECT"].into_iter().enumerate() {
            let label = rel
                .at(rel.pointer(4, 0x14f4 + index * 8 + column * 4).unwrap())
                .unwrap();
            let end = label.iter().position(|&byte| byte == 0).unwrap();
            assert_eq!(std::str::from_utf8(&label[..end]).unwrap(), expected);
        }
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let presea = crate::dol::slice(&executable, 0x80208e60 + 6 * 196, 196).unwrap();
        assert_eq!(word(presea, 0).unwrap(), 24);
        assert_eq!(
            &presea[4 + 17 * 8..4 + 18 * 8],
            &[0, 85, 0, 2, 45, 48, 0, 0]
        );
        assert_eq!(
            &presea[4 + 23 * 8..4 + 24 * 8],
            &[0, 155, 0, 4, 2, 45, 13, 48]
        );
    }

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_magic_guard_input_cost_recoil_and_contact_contract() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        // Down edge/guard held, unmodified admission cost, EX/ring order, special
        // damage and force exclusion, sound/elemental effects, retained recoil.
        for (address, expected) in [
            (0x10cdc, 0x2c00ffd0),
            (0x10ce8, 0x64000004),
            (0x10d70, 0x7c003278),
            (0x10d74, 0x7cc00038),
            (0x10d78, 0x90050040),
            (0x33b28, 0x80040040),
            (0x33b2c, 0x5400035b),
            (0x33b38, 0x80840030),
            (0x33b48, 0xa0035670),
            (0x33b4c, 0x7c800039),
            (0x33b64, 0x48003cc1),
            (0x1f3b4, 0x2c000022),
            (0x1f3c8, 0xa8040028),
            (0x1f3cc, 0x1c00000a),
            (0x378c4, 0x3880004b),
            (0x378d8, 0x7c001e70),
            (0x378dc, 0x7c00e850),
            (0x378f0, 0x38800086),
            (0x37904, 0x7c001670),
            (0x37908, 0x7c1d0214),
            (0x37934, 0x7c000e70),
            (0x62afc, 0x54600673),
            (0x62b00, 0x408200e8),
            (0x62c10, 0x39c00014),
            (0x62c14, 0x38800078),
            (0x62c24, 0x39c0000f),
            (0x62cb0, 0x649c0001),
            (0x63288, 0x540004a5),
            (0x63290, 0x88170282),
            (0x63294, 0x54000673),
            (0x63298, 0x40820030),
            (0x3b2a4, 0x57c006f7),
            (0x3b2c0, 0x4bfceb79),
            (0x3b2c4, 0x57c003df),
            (0x3b2cc, 0x38e00041),
            (0x3b2d8, 0x54000421),
            (0x3b4fc, 0x38a0002f),
            (0x3b51c, 0x48000364),
            (0x3b520, 0x57e006f7),
            (0x3b528, 0x57e003df),
            (0x3b5a8, 0x28000005),
            (0x3b5b0, 0x391c195c),
            (0x3b5c8, 0x7ca300ae),
            (0x3b60c, 0x391c195c),
            (0x63668, 0x38e00001),
            (0x6366c, 0x4bffdbe1),
            (0x63680, 0x907c1944),
            (0x63684, 0x901c1948),
            (0x6368c, 0x901c194c),
            (0x3cccc, 0x881a0282),
            (0x3ccd0, 0x54000673),
            (0x3ccd4, 0x40820098),
            (0x29650, 0x99630282),
            (0xd0cc, 0x50a03e30),
            (0xd0d0, 0x981b01c9),
            (0x39c00, 0x50603672),
            (0x39c08, 0x981c01c9),
        ] {
            assert_eq!(
                word(rel.at((1, address)).unwrap(), 0).unwrap(),
                expected,
                "source {address:#x}"
            );
        }
        assert_eq!(
            (0..9)
                .map(|i| half(rel.at((4, 0x1af0)).unwrap(), i * 2).unwrap())
                .collect::<Vec<_>>(),
            [34, 202, 203, 203, 204, 34, 205, 206, 34]
        );
        assert!((0..9).all(|i| rel.at((DATA, 0x3d30 + i * 0x1f0)).unwrap()[0x9e] == 0));
    }

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_magic_guard_low_hp_immunity_and_survival() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        for (address, expected) in [
            (0x62cb4, 0x38800053),
            (0x62cc8, 0x4bfbaf0d),
            (0x62ccc, 0x2c03000a),
            (0x62cd4, 0x639c0004),
            (0x1dbd8, 0x80640024),
            (0x1dbdc, 0x80040020),
            (0x1dbe0, 0x1c630064),
            (0x1dbe4, 0x7c6303d6),
            (0x63330, 0x3880009b),
            (0x63340, 0x88170282),
            (0x63344, 0x54000673),
            (0x6335c, 0x38000001),
            (0x63360, 0x90030024),
        ] {
            assert_eq!(word(rel.at((1, address)).unwrap(), 0).unwrap(), expected);
        }
    }

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_absolute_guard_feedback_and_shared_actor_clocks() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        // Immunity/absorption ordering, guard-bit independence, elemental/contact
        // effects, critical ownership, flash colors and shared post-controller clocks.
        for (address, expected) in [
            (0x629d4, 0x388000a4),
            (0x629d8, 0x4bfb9d75),
            (0x629e4, 0x88170282),
            (0x629e8, 0x54000673),
            (0x629f0, 0x639c0004),
            (0x62ac8, 0x73800026),
            (0x61d60, 0x8819000b),
            (0x61d70, 0x40820010),
            (0x61d78, 0x5400063c),
            (0x61d7c, 0x98170282),
            (0x61e40, 0x639c0200),
            (0x62de8, 0x3c6051ec),
            (0x62dec, 0x7c0f01d6),
            (0x62f2c, 0x3800ffc9),
            (0x62f30, 0x7f9c0038),
            (0x62f34, 0x578006b5),
            (0x62f7c, 0x4bfba8e9),
            (0x62f84, 0x73800104),
            (0x3d624, 0x4bffdd4d),
            (0x3d644, 0x4bffdbe5),
            (0x3b714, 0x391c195c),
            (0x3b770, 0x39000000),
            (0x3b7cc, 0x38a0000c),
            (0x3b7f0, 0x88fc1a11),
            (0x3b828, 0x38a5000b),
            (0x3b868, 0x38a00002),
            (0x3b87c, 0x4bfeb105),
            (0x3b880, 0x57e005ad),
            (0x3b8ac, 0x7f66db78),
            (0x3b8b4, 0x7f87e378),
            (0x3b8c4, 0x38a00010),
            (0x3b948, 0x38000004),
            (0x3b94c, 0x981c1a11),
            (0x25628, 0x887f1a11),
            (0x25634, 0x3803ffff),
            (0x25638, 0x981f1a11),
            (0x31cb4, 0x800359dc),
            (0x31cc4, 0x5400063b),
            (0x31e14, 0x881f1183),
            (0x31e24, 0x28000012),
            (0x31e3c, 0x880359e8),
            (0x31e9c, 0x4e800421),
            (0x31ea4, 0x4bff3199),
            (0x26980, 0x98a30290),
            (0x26988, 0x980301dc),
            (0x26990, 0x980301dd),
            (0x26998, 0x980301de),
            (0x25bf4, 0x887f0290),
            (0x25c04, 0x981f0290),
            (0x522b8, 0x881601dc),
            (0x522c4, 0x881601dd),
            (0x522d0, 0x881601de),
        ] {
            assert_eq!(
                word(rel.at((1, address)).unwrap(), 0).unwrap(),
                expected,
                "source {address:#x}"
            );
        }
        let code = rel.at((1, 0)).unwrap();
        let references: Vec<_> = code
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, b)| {
                let word = u32::from_be_bytes(b.try_into().unwrap());
                (word & 0xffff == 0x1a11 && matches!(word >> 26, 34 | 38)).then_some(i * 4)
            })
            .collect();
        assert_eq!(references, [0x25628, 0x25638, 0x3b530, 0x3b7f0, 0x3b94c]);
        let colors = rel.at((DATA, 0x13d0)).unwrap();
        assert_eq!(
            std::array::from_fn::<_, 9, _>(
                |i| <[u8; 3]>::try_from(&colors[i * 4..i * 4 + 3]).unwrap()
            ),
            [
                [192, 128, 128],
                [64, 64, 192],
                [64, 192, 64],
                [192, 64, 64],
                [192, 192, 128],
                [160, 160, 192],
                [160, 160, 192],
                [192, 192, 192],
                [48, 48, 48]
            ]
        );
    }

    #[test]
    #[ignore = "requires original extracted GameCube assets"]
    fn original_magic_guard_closes_all_character_entry_tracks_and_feedback() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let menus = [34, 202, 203, 204, 205, 206];
        let actions =
            super::super::super::technique_actions(&extracted, &rel, &usual, &menus).unwrap();
        for technique in &actions {
            let TechniqueProgram::Martial { variants } = &technique.program else {
                panic!()
            };
            assert_eq!(variants.len(), 9);
            for (index, phase) in variants.iter().enumerate() {
                assert_eq!(phase.variant as usize, index);
                let Some(MartialCallback::MagicGuard { contact_colors, .. }) = phase.callback
                else {
                    panic!()
                };
                assert_eq!(
                    contact_colors,
                    [
                        [192, 128, 128],
                        [64, 64, 192],
                        [64, 192, 64],
                        [192, 64, 64],
                        [192, 192, 128],
                        [160, 160, 192],
                        [160, 160, 192],
                        [192, 192, 192],
                        [48, 48, 48]
                    ]
                );
                assert_eq!((phase.action.duration, phase.recovery_ticks), (60, 10));
                assert_eq!(phase.effect, Some(if index == 2 { 2 } else { 1 }));
                assert_eq!(
                    phase.caption.as_deref(),
                    Some(
                        [
                            "Guardian",
                            "Damage Guard",
                            "Force Field",
                            "Force Field",
                            "Guardian Seal",
                            "Guardian",
                            "Earthly Protection",
                            "Bastion",
                            "Guardian"
                        ][index]
                    )
                );
                assert!(matches!(
                    phase.action.animations.initial,
                    Some(AnimationCommand::Play {
                        clip: 22,
                        blend: 8,
                        rate: 0.5,
                        ..
                    })
                ));
                assert!(matches!(
                    phase.action.commands[0].command,
                    ActionCommand::Sound(69)
                ));
                assert!(
                    matches!(phase.action.commands[1].command,ActionCommand::Voice {id,priority:2} if id == [0x805c,0x80ca,0x8149,0x81bd,0x8224,0x82a5,0x830e,0x837c,0x83e3][index])
                );
            }
        }
        let catalogue = BattleActions {
            party: vec![],
            enemies: vec![],
            projectiles: vec![],
            techniques: actions,
            chains: None,
        };
        catalogue.validate().unwrap();
        let (sounds, voices) = catalogue.audio_ids();
        assert!(sounds.contains(&69));
        assert_eq!(voices.len(), 9);
        let mut closure = crate::battle::selection::Dependencies::actions(&catalogue).unwrap();
        closure.programs.extend(
            crate::battle::contact_effects::cook(&rel)
                .unwrap()
                .programs(),
        );
        for id in [0, 1, 2, 11, 12, 16, 29, 30, 31, 32, 33, 34, 47, 49] {
            assert!(closure.programs.contains(&EffectId {
                bank: EffectBank::Common,
                id
            }));
        }
        for id in [1, 2] {
            assert!(closure.programs.contains(&EffectId {
                bank: EffectBank::Techniques,
                id
            }));
        }
    }
}
