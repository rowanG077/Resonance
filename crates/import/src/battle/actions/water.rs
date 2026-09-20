//! Recover the two stored water callbacks and their distinct retained origins.
use super::*;
#[cfg(test)]
use crate::battle::effect_program::{MagicArchive, magic_member};
use resonance_content::battle::actions::water::{WaterOrigin, WaterRecipe, WaterSpell};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub origin: WaterOrigin,
    pub effect_scale: f32,
    pub presentation: StoredSpellPresentation,
}

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<WaterRecipe> {
    let native = arte.native_id as u16;
    let (kind, flags, parameters) = match native {
        202 => (WaterSpell::TidalWave, 0x00440193, &tables.stored.tidal_wave),
        203 => (WaterSpell::AquaLaser, 0x0044018b, &tables.stored.aqua_laser),
        _ => bail!("unsupported stored water native {native}"),
    };
    ensure!(arte.flags == flags, "unexpected stored water binding");
    let bundle = tables.bundle(native)?;
    ensure!(
        bundle.phase(0)?.duration == if native == 202 { 180 } else { 0 }
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected water action phases or hit rules"
    );
    let recipe = WaterRecipe {
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
    let (initializer, callback, settings, duration, tick) = match native {
        202 => (0x851a4, 0x8513c, 0x7998, 0x851f4, 0x85154),
        203 => (0x7c050, 0x7bfe8, 0x5a20, 0x7c0a0, 0x7c000),
        _ => bail!("unsupported stored water native {native}"),
    };
    let dispatch = rel.pointer(DATA, 0x1238 + usize::from(native - 200) * 4)?;
    for (phase, handler) in [initializer, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected water phase {phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, callback)),
        "missing water callback"
    );
    // Both callbacks copy the retained point and request pool2 projectile1 exactly once.
    for (offset, instruction) in [
        (tick, 0x2c000000 | if native == 202 { 25 } else { 30 }),
        (callback + 0x34, 0x38600002),
        (callback + 0x3c, 0x38e00001),
        (duration, 0x38c00000 | if native == 202 { 210 } else { 160 }),
    ] {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected stored water callback operation at {offset:#x}"
        );
    }
    let operations: &[(usize, u32)] = if native == 202 {
        &[
            (0x85224, 0xd01f0020),
            (0x85234, 0xd01f0024),
            (0x85244, 0xd01f0028),
            (0x85238, 0x38a00001),
        ]
    } else {
        &[
            (0x7c0b4, 0x387e18e4),
            (0x7c0c8, 0x387e195c),
            (0x7c0f8, 0xd01f0024),
            (0x7c10c, 0x38a00001),
        ]
    };
    for &(offset, instruction) in operations {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected retained water origin"
        );
    }
    let settings = rel.at((4, settings))?;
    Ok(Parameters {
        lifetime: half(rel.at((1, duration + 2))?, 0)?,
        projectile_tick: half(rel.at((1, tick + 2))?, 0)?,
        origin: if native == 202 {
            WaterOrigin::Battlefield {
                position: [float(settings, 12)?; 3],
            }
        } else {
            WaterOrigin::CasterAhead {
                distance: float(settings, 12)?,
                height: float(settings, 16)?,
            }
        },
        effect_scale: float(settings, if native == 202 { 16 } else { 20 })?,
        presentation: stored_parameters::presentation(settings)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{EffectBank, ProjectileMovement};

    #[test]
    #[ignore = "requires privately extracted US assets; reads recipes without encoding assets"]
    fn original_water_origins_and_dynamic_projectile_banks_are_distinct() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let tables = Tables::original(&extracted, &rel, &usual, &[202, 203]).unwrap();
        let archive = MagicArchive::read(&extracted).unwrap();
        for (technique, ticks, duration, power, projectile_lifetime) in
            [(64, 25, 210, 40, 120), (65, 30, 160, 165, 90)]
        {
            let recipe = cook(&tables, catalogue.definition(technique).unwrap()).unwrap();
            assert_eq!(
                (recipe.projectile_tick, recipe.lifetime, recipe.rule.power),
                (ticks, duration, power)
            );
            let package = recipe.kind as u16 - 200;
            let data = magic_member(archive.package(package).unwrap(), 252)
                .unwrap()
                .unwrap();
            let flight =
                crate::battle::effects::projectile(&data[400..800], recipe.kind.effect(1), 0.5)
                    .unwrap();
            assert_eq!(flight.lifetime, projectile_lifetime);
            assert_eq!(flight.birth_bank, Some(EffectBank::Magic(package)));
            assert!(flight.persist_after_hit && flight.shadow.is_none());
            match recipe.kind {
                WaterSpell::TidalWave => {
                    assert!(matches!(
                        recipe.origin,
                        WaterOrigin::Battlefield {
                            position: [0., 0., 0.]
                        }
                    ));
                    assert_eq!(
                        (
                            flight.shape.radius,
                            flight.shape.height,
                            flight.behavior.repeat_limit
                        ),
                        (950., 50., 12)
                    );
                    assert!(flight.spawn_effect.is_none() && flight.trail_effect.is_none());
                }
                WaterSpell::AquaLaser => {
                    assert!(matches!(
                        recipe.origin,
                        WaterOrigin::CasterAhead {
                            distance: 80.,
                            height: 100.
                        }
                    ));
                    assert_eq!(
                        (
                            flight.shape.radius,
                            flight.shape.height,
                            flight.behavior.repeat_limit
                        ),
                        (150., 150., 3)
                    );
                    assert!(matches!(
                        flight.movement,
                        ProjectileMovement::Ballistic {
                            velocity: [0., 0., 35.],
                            acceleration: [0., 0., 0.],
                            steering: None
                        }
                    ));
                    assert_eq!(flight.spawn_effect, Some(recipe.kind.effect(2)));
                    assert_eq!(
                        (flight.trail_effect, flight.trail_interval),
                        (Some(recipe.kind.effect(3)), 3)
                    );
                }
            }
        }
    }
}
