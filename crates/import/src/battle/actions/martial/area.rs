//! Native contact-triggered area volumes use zero templates, not projectile table rows.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(in crate::battle::actions) struct Parameters {
    tick: u16,
    origin_height: f32,
    contact: InlineContactRecipe,
    followup: MartialEffect,
}

pub(super) fn callback(
    p: &Parameters,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<MartialCallback> {
    ensure!(
        variant < 3
            && source.phases[..3].iter().all(|phase| phase.duration == 50)
            && source.phases[3].duration == 0,
        "unexpected area phase table"
    );
    Ok(MartialCallback::ContactArea {
        tick: p.tick,
        origin_height: p.origin_height,
        contact: p.contact,
        rule: source.phase_rule(usize::from(variant), 1)?,
        followup: p.followup,
    })
}

pub(in crate::battle::actions) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0xd60 + 11 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x64808)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected native 11 area dispatch"
    );
    let entry = rel.at((1, 0x64808))?;
    for (at, instruction) in [
        (0x1c, 0x28000001),
        (0x2c, 0x20000009),
        (0x30, 0x3880000b),
        (0x3c, 0x38050001),
        (0x50, 0x901f0018),
    ] {
        ensure!(
            word(entry, at)? == instruction,
            "unexpected area initializer at {at:#x}"
        );
    }
    let code = rel.at((1, 0x646b4))?;
    for (at, instruction) in [
        (0x1c, 0xa80301be),
        (0x28, 0x2c000019),
        (0x34, 0x83a5000c),
        (0x44, 0x83fe199c),
        (0x48, 0x281f0000),
        (0x54, 0xc05f18c0),
        (0x58, 0xc01f18c8),
        (0x7c, 0x389d001c),
        (0x84, 0x39800000),
        (0x8c, 0x39600001),
        (0x94, 0x3800000b),
        (0xac, 0x38c00002),
        (0xb4, 0x39000409),
        (0xb8, 0x39200001),
        (0xc0, 0x39400001),
        (0xd0, 0x91810008),
        (0xd4, 0x9161000c),
        (0xd8, 0x90010010),
        (0xdc, 0xc07e18d0),
        (0xe0, 0x4bfd7341),
        (0x110, 0x7fe7fb78),
        (0x118, 0x38800001),
        (0x120, 0x38a0006b),
        (0x124, 0x39000000),
        (0x128, 0x39200000),
        (0x130, 0x39400000),
        (0x138, 0xc03e18d0),
        (0x13c, 0x4bfdba21),
    ] {
        ensure!(
            word(code, at)? == instruction,
            "unexpected area callback at {at:#x}"
        );
    }
    let values = rel.at((4, 0x3cc0))?;
    Ok(Parameters {
        tick: half(code, 0x2a)?,
        origin_height: float(values, 12)?,
        contact: InlineContactRecipe {
            lifetime: half(code, 0xae)?,
            velocity: [float(values, 0)?, float(values, 4)?, float(values, 8)?],
            offset: [0.; 3],
            shape: HitShape {
                radius: float(values, 16)?,
                height: float(values, 20)?,
                inner_radius: 0.,
                kind: HitShapeKind::Cylinder,
                damage_kind: 0,
                hit_class: 1,
                reaction: half(code, 0x96)?.try_into()?,
            },
        },
        followup: MartialEffect {
            effect: EffectId {
                bank: EffectBank::Techniques,
                id: half(code, 0x122)?.try_into()?,
            },
            scale: float(values, 24)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn area_recovers_all_character_tracks_and_complete_effect_dependencies() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let techniques =
            super::super::super::technique_actions(&extracted, &rel, &usual, &[11, 211]).unwrap();
        for technique in &techniques {
            assert_eq!(technique.native_id, 11);
            let TechniqueProgram::Martial { variants } = &technique.program else {
                panic!()
            };
            assert_eq!(variants.len(), 3);
            for phase in variants {
                assert_eq!(
                    (phase.action.duration, phase.recovery_ticks, phase.effect),
                    (50, 10, Some(13))
                );
                let Some(MartialCallback::ContactArea {
                    tick,
                    origin_height,
                    contact,
                    rule,
                    followup,
                }) = phase.callback
                else {
                    panic!()
                };
                assert_eq!((tick, origin_height, contact.lifetime), (25, 0., 2));
                assert_eq!((contact.velocity, contact.offset), ([0.; 3], [0.; 3]));
                assert!(matches!(contact.shape.kind, HitShapeKind::Cylinder));
                assert_eq!(
                    (
                        contact.shape.radius,
                        contact.shape.height,
                        contact.shape.reaction
                    ),
                    (100., 50., 11)
                );
                assert_eq!(
                    (rule.power, rule.hitstun, rule.contact_cooldown),
                    (120, 40, 30)
                );
                assert_eq!((followup.effect.id, followup.scale), (107, 1.));
                assert!(phase.callback.unwrap().projectiles().next().is_none());
            }
        }
        let actions = BattleActions {
            chains: None,
            party: vec![],
            enemies: vec![],
            projectiles: vec![],
            techniques,
        };
        actions.validate().unwrap();
        let dependencies = crate::battle::selection::Dependencies::actions(&actions).unwrap();
        assert!(dependencies.projectiles.is_empty());
        assert_eq!(
            dependencies.programs,
            [
                (EffectBank::Common, 21),
                (EffectBank::Techniques, 13),
                (EffectBank::Techniques, 107)
            ]
            .map(|(bank, id)| EffectId { bank, id })
            .into()
        );
        // The callback reads the selected descriptor's rule start, not a global row number.
        let mut source =
            bundle::Bundle::decode(member(member(&usual, 8).unwrap(), 11).unwrap()).unwrap();
        source.phases[1].hit_rule_root = RULE_BYTES;
        assert!(callback(&read_parameters(&rel).unwrap(), &source, 1).is_err());
    }
}
