use super::*;
use resonance_content::battle::effects::{EffectBank, KnockbackDirection, ProjectileMovement};

#[test]
#[ignore = "requires original extracted US assets; parses source records without encoding assets"]
fn original_earth_retains_all_aliases_ground_pulses_and_projectile_zero() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let catalogue = crate::arte::read(&executable).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let tables = Tables::original(&extracted, &rel, &usual, &[214, 215]).unwrap();
    let mut aliases = vec![];
    for (menu, arte) in catalogue.definitions.iter().enumerate() {
        let native = arte.native_id as u16;
        if ![214, 215].contains(&native) {
            continue;
        }
        let recipe = cook(&tables, arte).unwrap();
        let casters = recovery::casters(
            &catalogue,
            &tables,
            menu as u16,
            arte,
            recovery::Release::Stored,
        )
        .unwrap();
        aliases.push((
            menu,
            native,
            casters.iter().map(|c| c.character).collect::<Vec<_>>(),
        ));
        assert_eq!(recipe.effect_scale, 1.);
        assert_eq!(
            (
                recipe.target_height,
                recipe.target_nudge_distance,
                recipe.target_nudge_threshold
            ),
            (0., 1., 0.5)
        );
        let expected: &[(u16, u8, u16, u8, u8)] = if native == 214 {
            assert_eq!(recipe.lifetime, 190);
            assert_eq!(recipe.presentation.color, [16, 12, 12, 255]);
            assert_eq!(
                (
                    recipe.presentation.camera_distance,
                    recipe.presentation.camera_elevation
                ),
                (2600., 18.)
            );
            &[(40, 1, 65, 8, 7)]
        } else {
            assert_eq!(recipe.lifetime, 215);
            assert_eq!(recipe.presentation.color, [24, 20, 20, 255]);
            assert_eq!(
                (
                    recipe.presentation.camera_distance,
                    recipe.presentation.camera_elevation
                ),
                (2450., 12.)
            );
            &[
                (34, 0, 110, 30, 0),
                (78, 1, 110, 30, 24),
                (88, 1, 110, 30, 24),
                (98, 1, 110, 30, 24),
                (108, 1, 110, 30, 24),
            ]
        };
        assert_eq!(
            recipe
                .pulses
                .iter()
                .map(|p| (
                    p.tick,
                    p.projectile.id,
                    p.rule.power,
                    p.rule.contact_cooldown,
                    p.rule.knockback_delay
                ))
                .collect::<Vec<_>>(),
            expected
        );
        for pulse in &recipe.pulses {
            assert_eq!(
                (
                    pulse.rule.flags,
                    pulse.rule.hitstun,
                    pulse.rule.power_mode,
                    pulse.rule.sound,
                    pulse.rule.impact_effect
                ),
                (32, 35, 1, 0, 0)
            );
        }
        let effects = crate::battle::effects::cook(
            &extracted,
            &recipe.pulses.iter().map(|p| p.projectile).collect(),
        )
        .unwrap();
        for pulse in &recipe.pulses {
            let p = effects.projectile(pulse.projectile).unwrap();
            let expected = match (native, pulse.projectile.id) {
                (214, 1) => (90, 350., 200., 10, 10, [0., 0., 0.]),
                (215, 0) => (8, 90., 300., 13, 1, [0., 0., 0.]),
                (215, 1) => (8, 80., 100., 6, 1, [0., 200., 0.]),
                _ => unreachable!(),
            };
            assert_eq!(
                (
                    p.lifetime,
                    p.shape.radius,
                    p.shape.height,
                    p.shape.reaction,
                    p.behavior.repeat_limit,
                    p.spawn_offset
                ),
                expected
            );
            assert!(matches!(p.shape.kind, HitShapeKind::Cylinder));
            assert!(matches!(
                p.movement,
                ProjectileMovement::Ballistic {
                    velocity: [0., 0., 0.],
                    acceleration: [0., 0., 0.],
                    steering: None
                }
            ));
            assert!(matches!(
                (native, p.knockback),
                (214, KnockbackDirection::Attacker) | (215, KnockbackDirection::AwayFromProjectile)
            ));
            assert_eq!(p.birth_bank, Some(EffectBank::Magic(native - 200)));
            assert!(
                p.persist_after_hit
                    && p.active.is_none()
                    && p.spawn_effect.is_none()
                    && p.trail_effect.is_none()
                    && p.ground_effect.is_none()
                    && p.shadow.is_none()
            );
        }
        // Control-flow changes must fail along with altered pulse/row/origin arguments.
        let mutations: &[usize] = if native == 214 {
            &[0x87a23, 0x87a47, 0x87a33, 0x87a2c, 0x87a3b]
        } else {
            &[0x7e0a3, 0x7e06f, 0x7e04f, 0x7e08c, 0x7e0d3, 0x7e0db]
        };
        for &offset in mutations {
            let at = rel.sections[1].0 + offset;
            let saved = rel.bytes[at];
            rel.bytes[at] ^= 1;
            assert!(
                read_parameters(&rel, native).is_err(),
                "changed Earth source {offset:#x}"
            );
            rel.bytes[at] = saved;
        }
    }
    assert_eq!(
        aliases,
        [
            (76, 214, vec![3]),
            (77, 215, vec![3]),
            (218, 215, vec![6, 9])
        ]
    );
}
