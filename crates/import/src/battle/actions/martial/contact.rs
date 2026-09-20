//! Grounded followups use the last contacted actor, not the selected target.
use super::*;
use resonance_content::battle::projectile_modifiers::ProjectileOverride;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    pub native: u16,
    tick: u16,
    projectile: EffectId,
    origin_height: f32,
    overrides: [ProjectileOverride; 2],
    reaction: Option<u8>,
    followup: Option<MartialEffect>,
}

pub(super) fn instructions(code: &[u8], expected: &[(usize, u32)]) -> Result<()> {
    for &(offset, instruction) in expected {
        ensure!(
            word(code, offset)? == instruction,
            "contact projectile instruction changed at {offset:#x}"
        );
    }
    Ok(())
}

pub(super) fn call(code: &[u8], base: usize, offset: usize, target: usize) -> Result<()> {
    let instruction = word(code, offset)?;
    let displacement = ((instruction as i32) << 6) >> 6;
    ensure!(
        instruction & 0xfc000003 == 0x48000001
            && (base + offset) as i64 + i64::from(displacement & !3) == target as i64,
        "contact projectile call changed"
    );
    Ok(())
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<MartialCallback> {
    ensure!(
        variant < 2
            && (0..2).all(|i| source.phases[i].duration > 30)
            && (2..4).all(|i| source.phases[i].duration == 0),
        "contact projectile phase table changed"
    );
    Ok(MartialCallback::ContactProjectile {
        entry: None,
        tick: p.tick,
        projectile: p.projectile,
        rule: source.phase_rule(usize::from(variant), 1)?,
        origin_height: p.origin_height,
        overrides: p.overrides,
        reaction: p.reaction,
        followup: p.followup,
    })
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (initializer, update, height, tick_at, projectile_at, birth_at, kind_at, class_at) =
        match native {
            85 => (0x65048, 0x64f98, 0x4080, 0x1a, 0x52, 0x8a, 0x8e, 0x96),
            87 => (0x846bc, 0x8459c, 0x77d8, 0x1e, 0x5a, 0x92, 0x9e, 0xa6),
            _ => bail!("unknown contact projectile native"),
        };
    let dispatch = rel.pointer(DATA, 0xd60 + usize::from(native) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "contact projectile dispatcher changed"
    );
    let entry = rel.at((1, initializer))?;
    instructions(
        entry,
        &[
            (8, 0x38800000 | u32::from(native)),
            (0x18, 0x8803107e),
            (0x1c, 0x5400e73e),
            (0x20, 0x20000009),
            (0x24, 0x7c000034),
            (0x28, 0x5405de3e),
            (0x38, 0x901f0018),
        ],
    )?;
    call(entry, initializer, 0x2c, 0x38118)?;
    ensure!(
        rel.local_targets().contains(&(1, update)),
        "missing contact projectile callback binding"
    );
    let code = rel.at((1, update))?;
    let followup = if native == 85 {
        instructions(
            code,
            &[
                (0x10, 0xa80301be),
                (0x14, 0x80630014),
                (0x18, 0x2c00001e),
                (0x1c, 0x80e3000c),
                (0x24, 0x80a4199c),
                (0x28, 0x28050000),
                (0x2c, 0x41820074),
                (0x34, 0xc04518c0),
                (0x3c, 0x3907001c),
                (0x40, 0xc00518c8),
                (0x4c, 0x38600001),
                (0x50, 0x38e00004),
                (0x74, 0x80a41990),
                (0x80, 0x28030000),
                (0x84, 0x4182001c),
                (0x88, 0x3800004b),
                (0x8c, 0x38800000),
                (0x90, 0x9803005d),
                (0x94, 0x38000001),
                (0x98, 0x98830011),
                (0x9c, 0x98030012),
            ],
        )?;
        call(code, update, 0x7c, 0x205ac)?;
        None
    } else {
        instructions(
            code,
            &[
                (0x14, 0xa80301be),
                (0x18, 0x80630014),
                (0x1c, 0x2c00001e),
                (0x20, 0x80a3000c),
                (0x28, 0x809f199c),
                (0x2c, 0x28040000),
                (0x30, 0x418200dc),
                (0x38, 0xc04418c0),
                (0x3c, 0xc00418c8),
                (0x40, 0x3905001c),
                (0x54, 0x38600001),
                (0x58, 0x38e00004),
                (0x7c, 0x80bf1990),
                (0x88, 0x28030000),
                (0x8c, 0x41820080),
                (0x90, 0x3800004b),
                (0x98, 0x9803005d),
                (0x9c, 0x39800000),
                (0xa4, 0x38800001),
                (0xa8, 0x99830011),
                (0xac, 0x38a00008),
                (0xb8, 0x98830012),
                (0xc4, 0x38800001),
                (0xc8, 0x98a30013),
                (0xd4, 0x38a0006b),
                (0x100, 0x80ff199c),
            ],
        )?;
        call(code, update, 0x84, 0x205ac)?;
        call(code, update, 0x108, 0x40210)?;
        Some(MartialEffect {
            effect: EffectId {
                bank: EffectBank::Techniques,
                id: half(code, 0xd6)?.try_into()?,
            },
            scale: float(rel.at((4, 0x77dc))?, 0)?,
        })
    };
    Ok(Parameters {
        native,
        tick: half(code, tick_at)?,
        projectile: EffectId {
            bank: EffectBank::Techniques,
            id: half(code, projectile_at)?.try_into()?,
        },
        origin_height: float(rel.at((4, height))?, 0)?,
        overrides: [
            ProjectileOverride::BirthEffect {
                id: half(code, birth_at)?.try_into()?,
            },
            ProjectileOverride::HitClassification {
                damage_kind: half(code, kind_at)?.try_into()?,
                hit_class: half(code, class_at)?.try_into()?,
            },
        ],
        reaction: (native == 87)
            .then(|| half(code, 0xae).and_then(|value| Ok(u8::try_from(value)?)))
            .transpose()?,
        followup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_rel(native: u16) -> Rel {
        // Pinned US callback followed by its initializer; these remain code only in tests.
        let (update, initializer, instructions): (usize, usize, &[u32]) = match native {
            85 => (
                0x64f98,
                0x65048,
                &[
                    0x9421ffe0, 0x7c0802a6, 0x7c641b78, 0x90010024, 0xa80301be, 0x80630014,
                    0x2c00001e, 0x80e3000c, 0x40820080, 0x80a4199c, 0x28050000, 0x41820074,
                    0x3c600000, 0xc04518c0, 0xc0230000, 0x3907001c, 0xc00518c8, 0x38c10008,
                    0xd0410014, 0x38600001, 0x38e00004, 0xd0210018, 0x81210014, 0xd001001c,
                    0x80a10018, 0x8001001c, 0x91210008, 0x90a1000c, 0x90010010, 0x80a41990,
                    0xc02418d0, 0x4bfbb599, 0x28030000, 0x4182001c, 0x3800004b, 0x38800000,
                    0x9803005d, 0x38000001, 0x98830011, 0x98030012, 0x80010024, 0x7c0803a6,
                    0x38210020, 0x4e800020, 0x9421fff0, 0x7c0802a6, 0x38800055, 0x90010014,
                    0x93e1000c, 0x7c7f1b78, 0x8803107e, 0x5400e73e, 0x20000009, 0x7c000034,
                    0x5405de3e, 0x4bfd30a5, 0x3c600000, 0x38030000, 0x901f0018,
                ],
            ),
            87 => (
                0x8459c,
                0x846bc,
                &[
                    0x9421ffb0, 0x7c0802a6, 0x90010054, 0x93e1004c, 0x7c7f1b78, 0xa80301be,
                    0x80630014, 0x2c00001e, 0x80a3000c, 0x408200e8, 0x809f199c, 0x28040000,
                    0x418200dc, 0x3c600000, 0xc04418c0, 0xc00418c8, 0x3905001c, 0xc0230000,
                    0x7fe4fb78, 0xd0410030, 0x38c10024, 0x38600001, 0x38e00004, 0xd0210034,
                    0x81210030, 0xd0010038, 0x80a10034, 0x80010038, 0x91210024, 0x90a10028,
                    0x9001002c, 0x80bf1990, 0xc03f18d0, 0x4bf9bf8d, 0x28030000, 0x41820080,
                    0x3800004b, 0x3c800000, 0x9803005d, 0x39800000, 0x38e40000, 0x38800001,
                    0x99830011, 0x38a00008, 0x81010030, 0x381f02c0, 0x98830012, 0x7fe6fb78,
                    0x81410034, 0x38800001, 0x98a30013, 0x38610018, 0x81610038, 0x38a0006b,
                    0x91010018, 0x39000000, 0xc0470000, 0x39200000, 0x9141001c, 0x39400000,
                    0x91610020, 0x91810008, 0x9181000c, 0x90010010, 0x80ff199c, 0xc03f18d0,
                    0x4bfbbb6d, 0x80010054, 0x83e1004c, 0x7c0803a6, 0x38210050, 0x4e800020,
                    0x9421fff0, 0x7c0802a6, 0x38800057, 0x90010014, 0x93e1000c, 0x7c7f1b78,
                    0x8803107e, 0x5400e73e, 0x20000009, 0x7c000034, 0x5405de3e, 0x4bfb3a31,
                    0x3c600000, 0x38030000, 0x901f0018,
                ],
            ),
            _ => unreachable!(),
        };
        let mut rel = Rel {
            bytes: vec![0; 0x98004],
            sections: vec![(0, 0), (4, 0x90000), (0, 0), (0, 0), (0x90004, 0x8000)],
            pointers: [
                ((DATA, 0xd60 + usize::from(native) * 4), (DATA, 0x100)),
                ((DATA, 0x100), (1, initializer)),
                ((DATA, 0x104), (1, 0x37fd4)),
            ]
            .into(),
            local_targets: [(1, update)].into(),
        };
        for (i, instruction) in instructions.iter().enumerate() {
            let at = 4 + update + i * 4;
            rel.bytes[at..at + 4].copy_from_slice(&instruction.to_be_bytes());
        }
        rel.bytes[0x90004 + 0x77dc..0x90004 + 0x77e0].copy_from_slice(&1_f32.to_be_bytes());
        rel
    }

    #[test]
    fn native_contact_callbacks_recover_phase_rules_and_reject_changed_targeting() {
        let mut source = vec![0; 128 + 4 * RULE_BYTES];
        source[..4].copy_from_slice(&128_u32.to_be_bytes());
        for at in [4, 8, 12] {
            let end = source.len() as u32;
            source[at..at + 4].copy_from_slice(&end.to_be_bytes());
        }
        for (variant, duration) in [(0, 85_u16), (1, 80)] {
            source[16 + variant * 28..18 + variant * 28].copy_from_slice(&duration.to_be_bytes());
            source[28 + variant * 28..32 + variant * 28]
                .copy_from_slice(&(variant as u32 * 2).to_be_bytes());
        }
        // Distinct descriptor bases and adjacent rules detect using the first or wrong phase rule.
        for (index, power) in [10_u16, 80, 20, 90].into_iter().enumerate() {
            let at = 128 + index * RULE_BYTES;
            source[at + 13] = 1;
            source[at + 14..at + 16].copy_from_slice(&power.to_be_bytes());
        }
        let source = bundle::Bundle::decode(&source).unwrap();
        for native in [85, 87] {
            let mut rel = native_rel(native);
            let parameters = read_parameters(&rel, native).unwrap();
            for variant in 0..2 {
                let MartialCallback::ContactProjectile {
                    entry,
                    tick,
                    projectile,
                    rule,
                    origin_height,
                    overrides,
                    reaction,
                    followup,
                } = callback(&parameters, &source, variant).unwrap()
                else {
                    panic!("missing contact callback")
                };
                assert!(entry.is_none());
                assert_eq!((tick, projectile.id, origin_height), (30, 4, 0.));
                assert_eq!(rule.power, 80 + u16::from(variant) * 10);
                assert!(matches!(
                    overrides,
                    [
                        ProjectileOverride::BirthEffect { id: 75 },
                        ProjectileOverride::HitClassification {
                            damage_kind: 0,
                            hit_class: 1
                        }
                    ]
                ));
                assert_eq!(reaction, (native == 87).then_some(8));
                assert_eq!(
                    followup.map(|effect| (effect.effect.id, effect.scale)),
                    (native == 87).then_some((107, 1.))
                );
            }
            assert!(callback(&parameters, &source, 2).is_err());
            let update = if native == 85 { 0x64f98 } else { 0x8459c };
            for offset in if native == 85 {
                vec![0x18, 0x24, 0x3c, 0x74, 0x84, 0x98]
            } else {
                vec![0x1c, 0x28, 0x40, 0x7c, 0x8c, 0xc8, 0x100]
            } {
                let at = 4 + update + offset;
                rel.bytes[at + 3] ^= 4;
                assert!(
                    read_parameters(&rel, native).is_err(),
                    "changed callback {native} at {offset:#x}"
                );
                rel.bytes[at + 3] ^= 4;
            }
        }
    }
}
