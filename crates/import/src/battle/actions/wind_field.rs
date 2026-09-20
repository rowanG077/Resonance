//! Recover Cyclone and Air Blade without replacing either native's retained point.
use super::*;
use resonance_content::battle::actions::wind_field::{WindField, WindFieldOrigin, WindFieldRecipe};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub origin: WindFieldOrigin,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
}

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<WindFieldRecipe> {
    let native = arte.native_id as u16;
    let (kind, flags, parameters) = match native {
        210 => (WindField::Cyclone, 0x00440193, &tables.stored.cyclone),
        211 => (WindField::AirBlade, 0x0044018b, &tables.stored.air_blade),
        _ => bail!("unsupported stored Wind native {native}"),
    };
    ensure!(arte.flags == flags, "unexpected stored Wind binding");
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phase(0)?.duration == if native == 210 { 190 } else { 0 }
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected Wind action phases or hit rules"
    );
    let recipe = WindFieldRecipe {
        kind,
        lifetime: parameters.lifetime,
        projectile_tick: parameters.projectile_tick,
        rule: bundle.phase_rule(0, 0)?,
        origin: parameters.origin,
        effect_scale: parameters.effect_scale,
        presentation: parameters.presentation,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel, native: u16) -> Result<Parameters> {
    let (kind, initializer, callback, settings, lifetime, tick) = match native {
        210 => (WindField::Cyclone, 0x7eb8c, 0x7eb24, 0x62d0, 220, 50),
        211 => (WindField::AirBlade, 0x7c1d8, 0x7c170, 0x5a88, 160, 30),
        _ => bail!("unsupported stored Wind native {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    for (phase, handler) in [initializer, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Wind phase {phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, callback)),
        "missing Wind callback"
    );
    for (offset, instruction) in [
        (callback + 0x18, 0x2c000000 | tick),
        (callback + 0x34, 0x38600002),
        (callback + 0x3c, 0x38e00001),
        (initializer + 0x50, 0x38c00000 | lifetime),
        (
            callback + 0x54,
            if native == 210 {
                0x4bfa1a35
            } else {
                0x4bfa43e9
            },
        ),
        (
            initializer + 0x54,
            if native == 210 {
                0x4bfb8f31
            } else {
                0x4bfbb8e5
            },
        ),
    ] {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected stored Wind operation at {offset:#x}"
        );
    }
    let operations: &[(usize, u32)] = match kind {
        WindField::Cyclone => &[
            (0x7ebec, 0x80df0020),
            (0x7ebf4, 0x80bf0024),
            (0x7ec18, 0x391f0020),
            (0x7ec1c, 0x38a00001),
            (0x7ec24, 0x817f0028),
            (0x7ec44, 0x38040006),
            (0x7ec4c, 0x4bfc15c5),
        ],
        WindField::AirBlade => &[
            (0x7c23c, 0x387e18e4),
            (0x7c250, 0x387e195c),
            (0x7c258, 0x38bf0020),
            (0x7c280, 0xd01f0024),
            (0x7c294, 0x38a00001),
            (0x7c2c8, 0x38040006),
            (0x7c2d0, 0x4bfc3f41),
        ],
    };
    for &(offset, instruction) in operations {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected retained Wind origin"
        );
    }
    let fallback = rel.at((4, 0x1c4c))?;
    ensure!(
        (0..3).all(|i| float(fallback, i * 4).ok() == Some(0.)),
        "unsupported stored target fallback"
    );
    let settings = rel.at((4, settings))?;
    Ok(Parameters {
        lifetime: half(rel.at((1, initializer + 0x52))?, 0)?,
        projectile_tick: half(rel.at((1, callback + 0x1a))?, 0)?,
        origin: match kind {
            WindField::Cyclone => WindFieldOrigin::TargetGround {
                height: float(rel.at((4, 0x1c80))?, 0)?,
                nudge: 1.,
                threshold: float(rel.at((4, 0x2800))?, 0)?,
            },
            WindField::AirBlade => WindFieldOrigin::CasterAhead {
                distance: float(settings, 12)?,
                height: float(settings, 16)?,
            },
        },
        effect_scale: float(settings, if native == 210 { 12 } else { 20 })?,
        presentation: stored_parameters::presentation(settings)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{EffectBank, KnockbackDirection, ProjectileMovement};

    #[test]
    #[ignore = "requires the privately extracted US disc; parses source records without encoding assets"]
    fn original_cyclone_and_air_blade_keep_distinct_anchors_and_full_projectile_closure() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let tables = Tables::original(&extracted, &rel, &usual, &[210, 211]).unwrap();
        let mut aliases = vec![];
        for (menu, arte) in catalogue.definitions.iter().enumerate() {
            let native = arte.native_id as u16;
            if ![210, 211].contains(&native) {
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
                casters
                    .iter()
                    .map(|caster| caster.character)
                    .collect::<Vec<_>>(),
            ));
            assert_eq!(recipe.effect_scale, 1.);
            let effects =
                crate::battle::effects::cook(&extracted, &BTreeSet::from([recipe.kind.effect(1)]))
                    .unwrap();
            let flight = effects.projectile(recipe.kind.effect(1)).unwrap();
            assert!(matches!(flight.shape.kind, HitShapeKind::Box));
            assert_eq!(flight.birth_bank, Some(EffectBank::Magic(native - 200)));
            assert!(
                flight.persist_after_hit
                    && flight.ground_effect.is_none()
                    && flight.shadow.is_none()
            );
            match recipe.kind {
                WindField::Cyclone => {
                    assert_eq!(recipe.presentation.color, [16, 20, 16, 255]);
                    assert_eq!(
                        (
                            recipe.presentation.camera_distance,
                            recipe.presentation.camera_elevation
                        ),
                        (3400., 15.5)
                    );
                    assert!(matches!(
                        recipe.origin,
                        WindFieldOrigin::TargetGround {
                            height: 0.,
                            nudge: 1.,
                            threshold: 0.5
                        }
                    ));
                    assert_eq!(
                        (
                            recipe.lifetime,
                            recipe.projectile_tick,
                            recipe.rule.flags,
                            recipe.rule.power,
                            recipe.rule.hitstun,
                            recipe.rule.contact_cooldown
                        ),
                        (220, 50, 34, 60, 45, 8)
                    );
                    assert_eq!(
                        (
                            flight.lifetime,
                            flight.shape.radius,
                            flight.shape.height,
                            flight.behavior.repeat_limit
                        ),
                        (96, 350., 500., 12)
                    );
                    assert_eq!(flight.spawn_offset, [0., 150., 0.]);
                    assert!(matches!(
                        flight.knockback,
                        KnockbackDirection::AwayFromProjectile
                    ));
                    assert!(flight.spawn_effect.is_none() && flight.trail_effect.is_none());
                }
                WindField::AirBlade => {
                    assert_eq!(recipe.presentation.color, [20, 24, 24, 255]);
                    assert_eq!(
                        (
                            recipe.presentation.camera_distance,
                            recipe.presentation.camera_elevation
                        ),
                        (2400., 8.)
                    );
                    assert!(matches!(
                        recipe.origin,
                        WindFieldOrigin::CasterAhead {
                            distance: 80.,
                            height: 100.
                        }
                    ));
                    assert_eq!(
                        (
                            recipe.lifetime,
                            recipe.projectile_tick,
                            recipe.rule.flags,
                            recipe.rule.power,
                            recipe.rule.hitstun,
                            recipe.rule.contact_cooldown
                        ),
                        (160, 30, 32, 170, 35, 4)
                    );
                    assert_eq!(
                        (
                            flight.lifetime,
                            flight.shape.radius,
                            flight.shape.height,
                            flight.behavior.repeat_limit
                        ),
                        (90, 150., 150., 3)
                    );
                    assert!(matches!(
                        flight.movement,
                        ProjectileMovement::Ballistic {
                            velocity: [0., 0., 35.],
                            acceleration: [0., 0., 0.],
                            steering: None
                        }
                    ));
                    assert!(matches!(flight.knockback, KnockbackDirection::Velocity));
                    assert_eq!(flight.spawn_effect, Some(recipe.kind.effect(2)));
                    assert_eq!(
                        (flight.trail_effect, flight.trail_interval),
                        (Some(recipe.kind.effect(3)), 2)
                    );
                }
            }
            let at = rel.sections[1].0 + if native == 210 { 0x7eb3c } else { 0x7c188 };
            let saved = rel.bytes[at + 3];
            rel.bytes[at + 3] += 1;
            assert!(
                read_parameters(&rel, native).is_err(),
                "changed callback age must not retain the reviewed recipe"
            );
            rel.bytes[at + 3] = saved;
        }
        assert_eq!(aliases, [(72, 210, vec![3]), (73, 211, vec![3])]);
    }
}
