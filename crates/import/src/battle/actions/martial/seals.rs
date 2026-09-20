//! Recover the shared seal gate and Pinion's retained contact callback.
use super::*;
use resonance_content::battle::{conditions::StatDebuff, ui::NoticeKind};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    pub native: u16,
    tick: u16,
    level: SealLevel,
    impact: SealImpact,
    projectile: Option<Projectile>,
}

#[derive(Serialize, Deserialize)]
struct Projectile {
    tick: u16,
    projectile: EffectId,
    rule_index: usize,
}

fn instructions(code: &[u8], expected: &[(usize, u32)]) -> Result<()> {
    for &(offset, instruction) in expected {
        ensure!(
            word(code, offset)? == instruction,
            "seal callback changed at {offset:#x}"
        );
    }
    Ok(())
}

fn call(code: &[u8], base: usize, offset: usize, target: usize) -> Result<()> {
    let word = word(code, offset)?;
    ensure!(
        word & 0xfc00_0003 == 0x4800_0001
            && (base + offset) as i64 + i64::from(((word as i32) << 6 >> 6) & !3) == target as i64,
        "seal callback call changed"
    );
    Ok(())
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<MartialCallback> {
    ensure!(
        variant == 0 && (1..4).all(|i| source.phases[i].duration == 0),
        "seal has an unexpected alternate phase"
    );
    let recipe = SealRecipe {
        tick: p.tick,
        level: p.level,
        impact: p.impact,
        projectile: p
            .projectile
            .as_ref()
            .map(|shot| -> Result<_> {
                Ok(SealProjectile {
                    tick: shot.tick,
                    projectile: shot.projectile,
                    rule: source.rule(shot.rule_index)?,
                })
            })
            .transpose()?,
    };
    recipe.validate(source.phases[0].duration)?;
    Ok(MartialCallback::Seal { recipe })
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (base, initializer, update, contact, rodata, stat, notice) = match native {
        63..=65 => (
            63,
            0x7fbf0,
            0x7f9cc,
            0x7f880,
            0x6590,
            StatDebuff::DefenseDown,
            NoticeKind::DefenseDown,
        ),
        66..=68 => (
            66,
            0x826b0,
            0x8249c,
            0x82360,
            0x6f58,
            StatDebuff::EvasionDown,
            NoticeKind::EvasionDown,
        ),
        69..=71 => (
            69,
            0x80068,
            0x7fe54,
            0x7fd18,
            0x65f8,
            StatDebuff::AccuracyDown,
            NoticeKind::AccuracyDown,
        ),
        _ => bail!("unknown seal callback"),
    };
    let dispatch = rel.pointer(DATA, 0xd60 + usize::from(native) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "seal dispatch changed"
    );
    let entry = rel.at((1, initializer))?;
    call(entry, initializer, 0x14, 0x3823c)?;
    instructions(
        entry,
        &[(0x18, 0x3c600000), (0x1c, 0x38030000), (0x20, 0x901f0018)],
    )?;
    ensure!(
        rel.local_targets().contains(&(1, update)) && rel.local_targets().contains(&(1, contact)),
        "seal callback binding is absent"
    );
    let code = rel.at((1, update))?;
    instructions(
        code,
        &[
            (0x1c, 0xa0031176),
            (0x24, 0x28000000 | u32::from(base + 1)),
            (0x30, 0xa81e01be),
            (0x34, 0x2c000028),
            (0x4c, 0x3903001c),
            (0x50, 0x38600001),
            (0x58, 0x38e0000c),
            (0x68, 0x80be1990),
            (0x6c, 0xc03e18d0),
            (0x84, 0x90030138),
            (0x88, 0xa81e01be),
            (0x8c, 0x2c00000f),
            (0xb0, 0x88050038),
            (0xb8, 0x28070000 | u32::from(base)),
            (0xc0, 0x54c6d97e),
            (0xc4, 0x1ca60064),
            (0xc8, 0x7c001670),
            (0xd0, 0x7c651850),
            (0xd8, 0x7c801a14),
            (0xe4, 0x3860004b),
            (0xfc, 0x28070000 | u32::from(base + 2)),
            (0x104, 0x3be00001),
            (0x10c, 0x28070000 | u32::from(base + 1)),
            (0x12c, 0x83be19a0),
            (0x140, 0x801d195c),
            (0x174, 0x391d195c),
            (0x17c, 0x38800001),
        ],
    )?;
    for (at, target) in [(0x70, 0x205ac), (0x94, 0x4de40), (0x19c, 0x40210)] {
        call(code, update, at, target)?;
    }
    let power = base == 63;
    let tail = if power { 0x10 } else { 0 };
    instructions(
        code,
        &[
            (0x1b4 + tail, 0x3cc00000 | ((stat.bit() >> 16) as u32)),
            (0x1bc + tail, 0x38e0fff6),
            (0x1d0 + tail, 0x38800008),
            (0x1e4 + tail, 0x38a0002d),
        ],
    )?;
    for (at, target) in [(0x1c8, 0x207e4), (0x1d4, 0x21938), (0x1f4, 0x219d8)] {
        call(code, update, at + tail, target)?;
    }
    if power {
        instructions(
            code,
            &[
                (0x1a4, 0x3bc00384),
                (0x1a8, 0x38800023),
                (0x1b8, 0x3bc00546),
            ],
        )?;
        call(code, update, 0x1ac, 0x1c86c)?;
    } else {
        instructions(code, &[(0x1a4, 0x38800384)])?;
        call(code, update, 0x1a8, 0x7f738)?;
    }
    let on_hit = rel.at((1, contact))?;
    instructions(
        on_hit,
        &[
            (0xc, 0x54c006f7),
            (0x3c, 0x88050038),
            (0x4c, 0x1ca60064),
            (0x64, 0x2800004b),
            (0x6c, 0x801f195c),
            (0xa0, 0x391f195c),
            (0xa8, 0x38800001),
            (0xb8, word(code, 0x18c)?),
            (0xe0 + tail, 0x3cc00000 | ((stat.bit() >> 16) as u32)),
            (0xe8 + tail, 0x38e0fff6),
            (0xfc + tail, 0x38800008),
            (0x110 + tail, 0x38a0002d),
        ],
    )?;
    for (at, target) in [
        (0x24, 0x4de40),
        (0xc8, 0x40210),
        (0xf4 + tail, 0x207e4),
        (0x100 + tail, 0x21938),
        (0x120 + tail, 0x219d8),
    ] {
        call(on_hit, contact, at, target)?;
    }
    let ro = rel.at((4, rodata))?;
    ensure!(
        float(ro, 0)? == 0. && float(ro, 4)? == 1.,
        "seal effect transform changed"
    );
    let level = match native - base {
        0 => SealLevel::Basic,
        1 => SealLevel::Pinion,
        2 => SealLevel::Absolute,
        _ => unreachable!(),
    };
    Ok(Parameters {
        native,
        tick: half(code, 0x8e)?,
        level,
        impact: SealImpact {
            stat,
            notice,
            effect: EffectId {
                bank: EffectBank::Techniques,
                id: half(code, 0x18e)?.try_into()?,
            },
            amount: half(code, 0x1be + tail)? as i16,
            duration: half(code, 0x1a6)? as i16,
            tint: rel.at((4, 0x1584))?[..3].try_into()?,
            hold: half(code, 0x1e6 + tail)?,
        },
        projectile: (level == SealLevel::Pinion)
            .then(|| -> Result<_> {
                Ok(Projectile {
                    tick: half(code, 0x36)?,
                    projectile: EffectId {
                        bank: EffectBank::Techniques,
                        id: half(code, 0x5a)?.try_into()?,
                    },
                    rule_index: 1,
                })
            })
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the locally extracted original disc"]
    fn original_seals_recover_nine_native_recipes_and_all_callback_resources() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let techniques = (126..=134).collect::<Vec<_>>();
        let actions = technique_actions(&extracted, &rel, &usual, &techniques).unwrap();
        for action in actions {
            assert_eq!(action.native_id, action.technique - 63);
            let TechniqueProgram::Martial { variants } = action.program else {
                panic!()
            };
            assert_eq!(variants.len(), 1);
            let phase = &variants[0];
            let Some(MartialCallback::Seal { recipe }) = phase.callback else {
                panic!()
            };
            assert_eq!(recipe.tick, 15);
            assert_eq!(recipe.impact.amount, -10);
            assert_eq!(recipe.impact.duration, 900);
            assert_eq!(recipe.impact.tint, [40, 40, 112]);
            assert_eq!(recipe.impact.hold, 45);
            assert_eq!(
                recipe.impact.effect.id,
                [134, 59, 135][usize::from((action.native_id - 63) / 3)]
            );
            if let Some(shot) = recipe.projectile {
                assert_eq!((shot.tick, shot.projectile.id), (40, 12));
                assert_eq!(shot.rule.flags, 0x22);
            }
            assert!(matches!(
                phase.action.animations.initial,
                Some(AnimationCommand::Play { .. })
            ));
            assert!(
                callback(
                    &read_parameters(&rel, action.native_id).unwrap(),
                    &bundle::Bundle::decode(
                        member(member(&usual, 8).unwrap(), usize::from(action.native_id)).unwrap()
                    )
                    .unwrap(),
                    1
                )
                .is_err()
            );
        }
    }
}
