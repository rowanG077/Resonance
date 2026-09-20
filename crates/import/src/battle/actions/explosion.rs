//! Recover Explosion's stored presentation and two independent projectile rules.
use super::*;
use resonance_content::battle::actions::FireFieldRecipe;

pub(super) fn cook(tables: &Tables, definition: &Definition) -> Result<FireFieldRecipe> {
    ensure!(
        definition.native_id == 206 && definition.flags == 0x0044018b,
        "unexpected Explosion technique binding"
    );
    let bundle = tables.bundle(206)?;
    ensure!(
        bundle.phases.iter().all(|phase| phase.duration == 0) && bundle.rule_count() == 2,
        "unexpected Explosion action bundle"
    );
    tables.stored.explosion.fire(206, bundle)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<stored_parameters::Ground> {
    let dispatch = rel.pointer(DATA, 0x1238 + 6 * 4)?;
    for (phase, handler) in [0x7af90, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Explosion phase {phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x7aec4)),
        "missing Explosion callback"
    );
    for (at, instruction) in [
        (0x7aeec, 0x2c000003), // First projectile at age3, rule0.
        (0x7af10, 0x38600002),
        (0x7af18, 0x38e00001),
        (0x7af28, 0xc03d18d0),
        (0x7af2c, 0x4bfa5681),
        (0x7af34, 0x2c000041), // Second projectile at age65, rule1.
        (0x7af50, 0x391f001c),
        (0x7af54, 0x38600002),
        (0x7af5c, 0x38e00002),
        (0x7af6c, 0xc03d18d0),
        (0x7af70, 0x4bfa563d),
        (0x7afe0, 0x38c000be), // Initializer replaces the absent descriptor lifetime.
        (0x7afe4, 0x4bfbcb2d),
        (0x7b014, 0x391f0020),
        (0x7b018, 0x38a00001),
        (0x7b038, 0x889e1ac6),
        (0x7b040, 0x38040006),
        (0x7b050, 0x38600000),
        (0x7b05c, 0x4bfa4821),
    ] {
        ensure!(
            word(rel.at((1, at))?, 0)? == instruction,
            "unexpected Explosion source operation at {at:#x}"
        );
    }
    let fallback = rel.at((4, 0x1c4c))?;
    ensure!(
        (0..3).all(|i| float(fallback, i * 4).ok() == Some(0.)),
        "unsupported Explosion target fallback"
    );
    stored_parameters::Ground::read(
        rel,
        0x56b0,
        half(rel.at((1, 0x7afe2))?, 0)?,
        &[(3, 1, 0), (65, 2, 1)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the original extracted disc; parses source records without cooking"]
    fn original_explosion_has_two_distinct_hits_and_only_genis_as_a_learned_caster() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let tables = Tables::original(&extracted, &rel, &usual, &[206]).unwrap();
        let recipe = cook(&tables, catalogue.definition(68).unwrap()).unwrap();
        assert_eq!(
            (recipe.native_id, recipe.lifetime, recipe.effect_scale),
            (206, 190, 1.)
        );
        assert_eq!(recipe.presentation.color, [20, 16, 16, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2900., 15.5)
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
                .map(|p| (
                    p.tick,
                    p.projectile.id,
                    p.rule.power,
                    p.rule.contact_cooldown
                ))
                .collect::<Vec<_>>(),
            [(3, 1, 100, 50), (65, 2, 800, 90)]
        );
        assert!(recipe.pulses.iter().all(|p| p.rule.flags == 40
            && p.rule.power_mode == 1
            && matches!(
                p.rule.element,
                HitElement::Element(resonance_content::menu_data::Element::Fire)
            )));
        let learned = (1..=9)
            .filter(|&character| catalogue.learned_by(character).unwrap().contains(&68))
            .collect::<Vec<_>>();
        assert_eq!(learned, [3]);
    }
}
