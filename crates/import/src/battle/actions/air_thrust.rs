//! Recover Air Thrust's stored callback and captured target-center placement.
use super::*;
use resonance_content::battle::{
    actions::air_thrust::AirThrustRecipe,
    effects::{EffectBank, EffectId},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub lifetime: u16,
    pub projectile_tick: u16,
    pub presentation: StoredSpellPresentation,
    pub target_min_height: f32,
    pub effect_scale: f32,
}

pub(super) fn cook(tables: &Tables, definition: &Definition) -> Result<AirThrustRecipe> {
    ensure!(
        definition.native_id == 209 && definition.flags == 0x0044018b,
        "unexpected Air Thrust technique binding"
    );
    let bundle = tables.bundle(209)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0),
        "unexpected Air Thrust action bundle"
    );
    let parameters = &tables.elemental.air_thrust;
    let recipe = AirThrustRecipe {
        lifetime: parameters.lifetime,
        projectile_tick: parameters.projectile_tick,
        rule: bundle.phase_rule(0, 0)?,
        projectile: EffectId {
            bank: EffectBank::Magic(9),
            id: 1,
        },
        effect: EffectId {
            bank: EffectBank::Magic(9),
            id: 1,
        },
        presentation: parameters.presentation,
        target_min_height: parameters.target_min_height,
        effect_scale: parameters.effect_scale,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0x1238 + 9 * 4)?;
    for (phase, handler) in [0x83100, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Air Thrust phase {phase} implementation"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x83098)),
        "missing Air Thrust active callback"
    );
    // Verify the fixed callback age, pool/index, duration and captured center.
    for (offset, instruction) in [
        (0x830b0, 0x2c00001e),
        (0x830cc, 0x38600002),
        (0x830d4, 0x38e00001),
        (0x83150, 0x38c00096),
        (0x8316c, 0x8085195c),
        (0x83170, 0x80051960),
        (0x8317c, 0x80051964),
        (0x831c8, 0x38a00001),
    ] {
        ensure!(
            word(rel.at((1, offset))?, 0)? == instruction,
            "unexpected Air Thrust callback timing or target placement"
        );
    }
    let settings = rel.at((4, 0x7220))?;
    Ok(Parameters {
        lifetime: half(rel.at((1, 0x83150))?, 2)?,
        projectile_tick: half(rel.at((1, 0x830b0))?, 2)?,
        presentation: stored_parameters::presentation(settings)?,
        target_min_height: float(settings, 12)?,
        effect_scale: float(settings, 16)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{KnockbackDirection, ProjectileMovement};

    #[test]
    #[ignore = "requires the privately extracted US disc; parses records without encoding assets"]
    fn original_air_thrust_retains_its_target_center_rule_and_callback() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let definition = catalogue.definition(71).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let tables = Tables::original(&extracted, &rel, &usual, &[209]).unwrap();
        let recipe = cook(&tables, definition).unwrap();
        assert_eq!((recipe.lifetime, recipe.projectile_tick), (150, 30));
        assert_eq!((recipe.target_min_height, recipe.effect_scale), (176., 1.));
        assert_eq!(recipe.presentation.color, [16, 24, 24, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2700., 13.)
        );
        assert!(matches!(
            recipe.rule.element,
            HitElement::Element(resonance_content::menu_data::Element::Wind)
        ));
        assert_eq!(
            (
                recipe.rule.power,
                recipe.rule.hitstun,
                recipe.rule.contact_cooldown,
                recipe.rule.knockback_delay
            ),
            (70, 55, 10, 8)
        );
        let effects =
            crate::battle::effects::cook(&extracted, &BTreeSet::from([recipe.projectile])).unwrap();
        let projectile = effects.projectile(recipe.projectile).unwrap();
        assert_eq!(projectile.lifetime, 90);
        assert_eq!(
            (projectile.shape.radius, projectile.shape.height),
            (192., 192.)
        );
        assert!(matches!(projectile.shape.kind, HitShapeKind::Sphere));
        assert!(matches!(projectile.knockback, KnockbackDirection::Velocity));
        assert!(matches!(
            projectile.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 0., 0.],
                acceleration: [0., 0., 0.],
                steering: None
            }
        ));
        assert_eq!(projectile.hit_offset, [0.; 3]);
        assert_eq!(
            (projectile.shape.reaction, projectile.behavior.repeat_limit),
            (6, 7)
        );
        assert!(
            projectile.persist_after_hit
                && projectile.spawn_effect.is_none()
                && projectile.trail_effect.is_none()
                && projectile.ground_effect.is_none()
                && projectile.shadow.is_none()
        );
        assert_eq!(projectile.birth_bank, Some(EffectBank::Magic(9)));
        let callback = rel.sections[1].0 + 0x830b0;
        rel.bytes[callback + 3] = 31;
        assert!(
            read_parameters(&rel).is_err(),
            "a changed callback must not use the fixed age-30 recipe"
        );
    }
}
