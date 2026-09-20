//! Recover the falling lance and its delayed ground burst as separate contacts.
use super::*;
use resonance_content::battle::actions::fire_field::FireFieldRecipe;

pub(super) fn cook(tables: &Tables, definition: &Definition) -> Result<FireFieldRecipe> {
    ensure!(
        definition.native_id == 207 && definition.flags == 0x0044018b,
        "unexpected Flame Lance technique binding"
    );
    let bundle = tables.bundle(207)?;
    ensure!(
        bundle.phase(0)?.duration == 200
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0),
        "unexpected Flame Lance action bundle"
    );
    tables.stored.flame_lance.fire(207, bundle)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<stored_parameters::Ground> {
    let dispatch = rel.pointer(DATA, 0x1238 + 7 * 4)?;
    for (phase, handler) in [0x75520, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Flame Lance phase {phase} implementation"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x75448)),
        "missing Flame Lance active callback"
    );
    for (offset, instruction) in [
        (0x75474, 0x2c00000f), // First contact at age 15, first rule, pool 2 / projectile 1.
        (0x75494, 0x7fe8fb78),
        (0x754a0, 0x38600002),
        (0x754a4, 0x38e00001),
        (0x754c0, 0x2c00005a), // Second contact at age 90, next 28-byte rule.
        (0x754dc, 0x391f001c),
        (0x754e4, 0x38600002),
        (0x754e8, 0x38e00002),
        (0x75570, 0x38c000be), // Initialization overrides the bundle's duration.
        (0x75578, 0x38600000),
        (0x755b0, 0x38a00001),
    ] {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected Flame Lance callback timing or contact binding"
        );
    }
    let fallback = rel.at((4, 0x1c4c))?;
    ensure!(
        (0..3).all(|axis| float(fallback, axis * 4).ok() == Some(0.)),
        "unsupported stored spell target fallback"
    );
    stored_parameters::Ground::read(rel, 0x51a0, 190, &[(15, 1, 0), (90, 2, 1)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{
        EffectBank, EffectId, KnockbackDirection, ProjectileMovement,
    };

    #[test]
    #[ignore = "requires the privately extracted US disc; parses records without encoding assets"]
    fn original_flame_lance_keeps_both_contacts_and_the_falling_model_lifecycle() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let definition = catalogue.definition(69).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let tables = Tables::original(&extracted, &rel, &usual, &[207]).unwrap();
        let recipe = cook(&tables, definition).unwrap();
        assert_eq!(
            (recipe.native_id, recipe.lifetime, recipe.effect_scale),
            (207, 190, 1.)
        );
        assert_eq!(recipe.presentation.color, [32, 20, 20, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2650., 13.)
        );
        assert_eq!(
            (
                recipe.target_height,
                recipe.target_nudge_distance,
                recipe.target_nudge_threshold
            ),
            (0., 1., 0.5)
        );
        assert_eq!(
            recipe
                .pulses
                .iter()
                .map(|p| (p.tick, p.projectile.id, p.rule.flags, p.rule.power))
                .collect::<Vec<_>>(),
            [(15, 1, 32, 200), (90, 2, 40, 250)]
        );
        for pulse in &recipe.pulses {
            assert_eq!(
                (
                    pulse.rule.hitstun,
                    pulse.rule.contact_cooldown,
                    pulse.rule.stun_chance
                ),
                (40, 200, 5)
            );
        }
        let effects = crate::battle::effects::cook(
            &extracted,
            &recipe.pulses.iter().map(|p| p.projectile).collect(),
        )
        .unwrap();
        let lance = effects.projectile(recipe.pulses[0].projectile).unwrap();
        assert_eq!(lance.lifetime, 90);
        assert!(
            lance.behavior.stop_on_ground
                && lance.behavior.unlimited_range
                && lance.persist_after_hit
        );
        assert!(matches!(
            lance.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., -40., 40.],
                acceleration: [0., 0., 0.],
                steering: None
            }
        ));
        assert_eq!(lance.spawn_offset, [0., 1000., -1000.]);
        assert_eq!(lance.active, Some([5, 45]));
        assert_eq!((lance.shape.radius, lance.shape.height), (100., 100.));
        assert!(matches!(lance.shape.kind, HitShapeKind::Box));
        assert_eq!(
            lance.spawn_effect,
            Some(EffectId {
                bank: EffectBank::Magic(7),
                id: 2
            })
        );
        assert_eq!(
            lance.trail_effect,
            Some(EffectId {
                bank: EffectBank::Magic(7),
                id: 3
            })
        );
        assert_eq!(lance.trail_interval, 1);
        assert!(lance.ground_effect.is_none() && lance.shadow.is_some());
        let burst = effects.projectile(recipe.pulses[1].projectile).unwrap();
        assert_eq!(burst.lifetime, 8);
        assert_eq!(burst.spawn_offset, [0., 150., 0.]);
        assert_eq!(
            (burst.shape.radius, burst.shape.height, burst.shape.reaction),
            (200., 300., 8)
        );
        assert!(matches!(burst.shape.kind, HitShapeKind::Cylinder));
        assert!(matches!(burst.knockback, KnockbackDirection::Velocity));
        assert!(!burst.behavior.stop_on_ground && burst.shadow.is_none());
        assert!(
            burst.spawn_effect.is_none()
                && burst.trail_effect.is_none()
                && burst.ground_effect.is_none()
        );
        let callback = rel.sections[1].0 + 0x754c0;
        rel.bytes[callback + 3] = 91;
        assert!(
            read_parameters(&rel).is_err(),
            "changed burst timing must not use the fixed recipe"
        );
    }
}
