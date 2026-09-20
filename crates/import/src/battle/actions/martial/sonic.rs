//! Lloyd alone can replace Sonic Thrust with its hit-confirmed Lightning phase.
use super::{
    contact::{call, instructions},
    *,
};
use resonance_content::battle::{
    actions::ElementalEntry, projectile_modifiers::ProjectileOverride,
};
use serde::{Deserialize, Serialize};

const INITIALIZER: usize = 0x64ae4;
const UPDATE: usize = 0x64a28;

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    entry: ElementalEntry,
    tick: u16,
    projectile: EffectId,
    origin_height: f32,
    overrides: [ProjectileOverride; 2],
    caption: String,
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<Option<MartialCallback>> {
    ensure!(
        variant < 4
            && source.rule_count() == 2
            && source
                .phases
                .iter()
                .enumerate()
                .all(
                    |(p, phase)| phase.duration == if p == 0 || p == 3 { 50 } else { 65 }
                        && phase.hit_rule_root == 0
                ),
        "Sonic Thrust phase table changed"
    );
    Ok(if variant == 3 {
        Some(MartialCallback::ContactProjectile {
            entry: Some(p.entry),
            tick: p.tick,
            projectile: p.projectile,
            rule: source.phase_rule(3, 1)?,
            origin_height: p.origin_height,
            overrides: p.overrides,
            reaction: None,
            followup: None,
        })
    } else {
        None
    })
}

pub(super) fn caption(p: &Parameters, variant: u8) -> Result<Option<String>> {
    ensure!(variant < 4, "unexpected Sonic Thrust caption phase");
    Ok((variant == 3).then(|| p.caption.clone()))
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0xd60 + 10 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, INITIALIZER)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4)
            && rel.local_targets().contains(&(1, UPDATE)),
        "Sonic Thrust dispatcher changed"
    );
    let entry = rel.at((1, INITIALIZER))?;
    instructions(
        entry,
        &[
            (8, 0x388000d2),
            (0x28, 0x5400e73e),
            (0x30, 0x28000001),
            (0x3c, 0x3880000a),
            (0x44, 0x38600000),
            (0x48, 0x5060073e),
            (0x58, 0x28000001),
            (0x5c, 0x4082006c),
            (0x6c, 0x2c0000c8),
            (0x70, 0x4180002c),
            (0x78, 0x38800000),
            (0x7c, 0x38a00000),
            (0x8c, 0x20630005),
            (0xac, 0x3880000a),
            (0xb0, 0x38a00003),
            (0xc8, 0x20000009),
            (0xd0, 0x7c000034),
            (0xd4, 0x3880000a),
            (0xd8, 0x5405d97e),
            (0xdc, 0x38050001),
            (0xe0, 0x5405063e),
            (0x10c, 0x901f0018),
        ],
    )?;
    for (at, target) in [
        (0x64, 0x218e4),
        (0x80, 0x20704),
        (0xb4, 0x38118),
        (0xc0, 0x3823c),
        (0xe4, 0x38118),
        (0x100, 0x1ada0),
    ] {
        call(entry, INITIALIZER, at, target)?;
    }
    let code = rel.at((1, UPDATE))?;
    instructions(
        code,
        &[
            (0x10, 0x8803117f),
            (0x14, 0x5400073f),
            (0x18, 0x41820094),
            (0x1c, 0xa80401be),
            (0x20, 0x2c000019),
            (0x24, 0x40820088),
            (0x28, 0x80a4199c),
            (0x2c, 0x28050000),
            (0x30, 0x4182007c),
            (0x38, 0xc04518c0),
            (0x44, 0xc00518c8),
            (0x48, 0x38600001),
            (0x50, 0x38e00004),
            (0x78, 0x80a41990),
            (0x7c, 0x8108000c),
            (0x84, 0x3908001c),
            (0x8c, 0x28030000),
            (0x90, 0x4182001c),
            (0x94, 0x3800004a),
            (0x98, 0x38800000),
            (0x9c, 0x9803005d),
            (0xa0, 0x38000001),
            (0xa4, 0x98830011),
            (0xa8, 0x98030012),
        ],
    )?;
    call(code, UPDATE, 0x88, 0x205ac)?;
    Ok(Parameters {
        entry: ElementalEntry {
            character: half(entry, 0x5a)?.try_into()?,
            minimum_uses: half(entry, 0x6e)?,
            element: resonance_content::menu_data::Element::ALL
                [usize::from(half(entry, 0x8e)?) - 1],
        },
        tick: half(code, 0x22)?,
        projectile: EffectId {
            bank: EffectBank::Techniques,
            id: half(code, 0x52)?.try_into()?,
        },
        origin_height: float(rel.at((4, 0x3efc))?, 0)?,
        overrides: [
            ProjectileOverride::BirthEffect {
                id: half(code, 0x96)?.try_into()?,
            },
            ProjectileOverride::HitClassification {
                damage_kind: half(code, 0x9a)?.try_into()?,
                hit_class: half(code, 0xa2)?.try_into()?,
            },
        ],
        caption: read_caption(rel)?,
    })
}

fn read_caption(rel: &Rel) -> Result<String> {
    ensure!(
        word(rel.at((4, 0x3ef4))?, 0)? == 0,
        "Sonic Thrust default caption changed"
    );
    let bytes = rel.at(rel.pointer(4, 0x3ef8)?)?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .context("unterminated Sonic Thrust caption")?;
    ensure!(end > 0, "empty Sonic Thrust caption");
    Ok(std::str::from_utf8(&bytes[..end])?.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn original() -> (Rel, bundle::Bundle) {
        // Complete US64A28/64AE4 code; original four descriptors and both rules.
        let words: [u32; 120] = [
            0x9421ffe0, 0x7c0802a6, 0x7c641b78, 0x90010024, 0x8803117f, 0x5400073f, 0x41820094,
            0xa80401be, 0x2c000019, 0x40820088, 0x80a4199c, 0x28050000, 0x4182007c, 0x3c600000,
            0xc04518c0, 0xc0230000, 0x38c10008, 0xc00518c8, 0x38600001, 0xd0410014, 0x38e00004,
            0xd0210018, 0x81010014, 0xd001001c, 0x80a10018, 0x8001001c, 0x91010008, 0x90a1000c,
            0x90010010, 0x81040014, 0x80a41990, 0x8108000c, 0xc02418d0, 0x3908001c, 0x4bfbbafd,
            0x28030000, 0x4182001c, 0x3800004a, 0x38800000, 0x9803005d, 0x38000001, 0x98830011,
            0x98030012, 0x80010024, 0x7c0803a6, 0x38210020, 0x4e800020, 0x9421ffe0, 0x7c0802a6,
            0x388000d2, 0x90010024, 0x93e1001c, 0x7c7f1b78, 0x3c600000, 0x84a30000, 0x881f107e,
            0x80630004, 0x5400e73e, 0x90a10008, 0x28000001, 0x9061000c, 0x40820008, 0x3880000a,
            0x881f117f, 0x38600000, 0x5060073e, 0x981f117f, 0x881f107e, 0x5400e73e, 0x28000001,
            0x4082006c, 0x7fe3fb78, 0x4bfbcd9d, 0x7c600734, 0x2c0000c8, 0x4180002c, 0x7fe3fb78,
            0x38800000, 0x38a00000, 0x4bfbbba1, 0x5463063e, 0x881f117f, 0x20630005, 0x7c630034,
            0x5060df3e, 0x981f117f, 0x881f117f, 0x5400073f, 0x41820018, 0x7fe3fb78, 0x3880000a,
            0x38a00003, 0x4bfd3581, 0x48000030, 0x7fe3fb78, 0x4bfd3699, 0x48000024, 0x20000009,
            0x7fe3fb78, 0x7c000034, 0x3880000a, 0x5405d97e, 0x38050001, 0x5405063e, 0x4bfd3551,
            0x881f117f, 0x38810008, 0x7fe3fb78, 0x38a00000, 0x540016ba, 0x7c84002e, 0x4bfb61bd,
            0x3c600000, 0x38030000, 0x901f0018, 0x83e1001c, 0x80010024, 0x7c0803a6, 0x38210020,
            0x4e800020,
        ];
        let mut rel = Rel {
            bytes: vec![0; 0x70004],
            sections: vec![(0, 0), (4, 0x68000), (0, 0), (0, 0), (0x68004, 0x8000)],
            pointers: [
                ((DATA, 0xd60 + 10 * 4), (DATA, 0x100)),
                ((DATA, 0x100), (1, INITIALIZER)),
                ((DATA, 0x104), (1, 0x37fd4)),
                ((4, 0x3ef8), (4, 0x3ee0)),
            ]
            .into(),
            local_targets: [(1, UPDATE)].into(),
        };
        for (i, word) in words.into_iter().enumerate() {
            let at = 4 + UPDATE + i * 4;
            rel.bytes[at..at + 4].copy_from_slice(&word.to_be_bytes());
        }
        rel.bytes[0x68004 + 0x3ee0..0x68004 + 0x3ef1].copy_from_slice(b"Lightning Thrust\0");
        let mut source = vec![0; 184];
        source[..184].copy_from_slice(&[
            0, 0, 0, 128, 0, 0, 0, 184, 0, 0, 1, 184, 0, 0, 2, 48, 0, 50, 0, 10, 0, 50, 0, 25, 0,
            0, 0, 34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 65, 0, 5, 0, 50, 0, 25, 0,
            0, 0, 34, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 12, 0, 65, 0, 5, 0, 50, 0, 25,
            0, 0, 0, 34, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 5, 0, 0, 0, 24, 0, 50, 0, 10, 0, 50, 0,
            25, 0, 0, 0, 34, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 8, 0, 0, 0, 36, 0, 32, 0, 40, 30, 1,
            1, 1, 0, 0, 0, 0, 0, 1, 0, 140, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 32, 5, 40, 30,
            10, 0, 2, 0, 0, 0, 0, 0, 1, 0, 50, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
        ]);
        // This callback consumes phase scalars and rules, not program streams.
        for at in [4, 8, 12] {
            source[at..at + 4].copy_from_slice(&184_u32.to_be_bytes());
        }
        for phase in 0..4 {
            source[32 + phase * 28..44 + phase * 28].fill(0);
        }
        // A decoy first rule cannot change the native pointer+28 rule selection.
        source[142..144].copy_from_slice(&999_u16.to_be_bytes());
        (rel, bundle::Bundle::decode(&source).unwrap())
    }
    #[test]
    fn source_selects_lloyd_lightning_phase_and_uses_second_contact_rule() {
        let (mut rel, mut source) = original();
        let p = read_parameters(&rel).unwrap();
        let p: Parameters = serde_json::from_value(serde_json::to_value(p).unwrap()).unwrap();
        for variant in 0..3 {
            assert!(callback(&p, &source, variant).unwrap().is_none());
            assert!(caption(&p, variant).unwrap().is_none());
        }
        let Some(MartialCallback::ContactProjectile {
            entry: Some(entry),
            tick,
            projectile,
            rule,
            origin_height,
            overrides,
            reaction,
            followup,
        }) = callback(&p, &source, 3).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (entry.character, entry.minimum_uses, entry.element),
            (1, 200, resonance_content::menu_data::Element::Lightning)
        );
        assert_eq!((tick, projectile.id, origin_height), (25, 4, 0.));
        assert_eq!(
            (rule.power, rule.flags, rule.hitstun, rule.contact_cooldown),
            (50, 0x20, 40, 30)
        );
        assert!(matches!(
            rule.element,
            HitElement::Element(resonance_content::menu_data::Element::Lightning)
        ));
        assert!(matches!(
            overrides,
            [
                ProjectileOverride::BirthEffect { id: 74 },
                ProjectileOverride::HitClassification {
                    damage_kind: 0,
                    hit_class: 1
                }
            ]
        ));
        assert!(reaction.is_none() && followup.is_none());
        assert_eq!(caption(&p, 3).unwrap().as_deref(), Some("Lightning Thrust"));
        assert!(callback(&p, &source, 4).is_err());
        source.phases[3].duration = 306;
        assert!(callback(&p, &source, 3).is_err());
        source.phases[3].duration = 50;
        rel.bytes[4 + INITIALIZER + 0xb3] = 2;
        assert!(read_parameters(&rel).is_err());
        assert!(callback(&p, &source, 0).unwrap().is_none());
    }
}
