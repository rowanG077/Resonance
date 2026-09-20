//! Recover Eruption's stored presentation and alternating ground contacts.
use super::*;
use resonance_content::battle::actions::FireFieldRecipe;

pub(super) fn cook(tables: &Tables, definition: &Definition) -> Result<FireFieldRecipe> {
    ensure!(
        definition.native_id == 205 && definition.flags == 0x0044018b,
        "unexpected Eruption technique binding"
    );
    let bundle = tables.bundle(205)?;
    ensure!(
        bundle.phase(0)?.duration == 190
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 3,
        "unexpected Eruption action bundle"
    );
    tables.stored.eruption.fire(205, bundle)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<stored_parameters::Ground> {
    let dispatch = rel.pointer(DATA, 0x1238 + 5 * 4)?;
    for (phase, handler) in [0x75360, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Eruption phase {phase}"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x75278)),
        "missing Eruption callback"
    );
    for (at, instruction) in [
        (0x75290, 0x2c06001e),
        (0x752a0, 0x2c060078),
        (0x752ac, 0x38e6ffe2),
        (0x752c8, 0x1c00001e),
        (0x752e0, 0x2c000001),
        (0x752f4, 0x38600002),
        (0x752fc, 0x38e00002),
        (0x75314, 0x4bfab299),
        (0x75328, 0x3908001c),
        (0x7532c, 0x38600002),
        (0x75334, 0x38e00003),
        (0x7534c, 0x4bfab261),
        (0x753b0, 0x38c000d2),
        (0x753b4, 0x4bfc275d),
        (0x753b8, 0x38600000),
        (0x753bc, 0x4bfaa4c1),
        (0x753ec, 0x391f0020),
        (0x753f0, 0x38a00001),
        (0x75410, 0x889e1ac6),
        (0x75418, 0x38040006),
    ] {
        ensure!(
            word(rel.at((1, at))?, 0)? == instruction,
            "unexpected Eruption source operation at {at:#x}"
        );
    }
    let fallback = rel.at((4, 0x1c4c))?;
    ensure!(
        (0..3).all(|i| float(fallback, i * 4).ok() == Some(0.)),
        "unsupported Eruption target fallback"
    );
    stored_parameters::Ground::read(
        rel,
        0x5140,
        half(rel.at((1, 0x753b2))?, 0)?,
        &[(30, 2, 0), (60, 3, 1), (90, 2, 0)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{EffectBank, EffectId};
    #[test]
    #[ignore = "requires original extracted disc; parses source records without encoding assets"]
    fn original_eruption_retains_alternating_rules_and_all_learned_aliases() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let tables = Tables::original(&extracted, &rel, &usual, &[205]).unwrap();
        let mut aliases = Vec::new();
        for (menu, definition) in catalogue.definitions.iter().enumerate() {
            if definition.native_id != 205 {
                continue;
            }
            let recipe = cook(&tables, definition).unwrap();
            let casters = recovery::casters(
                &catalogue,
                &tables,
                menu as u16,
                definition,
                recovery::Release::Stored,
            )
            .unwrap();
            aliases.push((
                menu,
                casters.iter().map(|c| c.character).collect::<Vec<_>>(),
            ));
            assert_eq!((recipe.lifetime, recipe.effect_scale), (210, 1.));
            assert_eq!(recipe.presentation.color, [24, 16, 16, 255]);
            assert_eq!(
                (
                    recipe.presentation.camera_distance,
                    recipe.presentation.camera_elevation
                ),
                (2800., 13.)
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
                        p.rule.flags,
                        p.rule.power,
                        p.rule.hitstun
                    ))
                    .collect::<Vec<_>>(),
                [
                    (30, 2, 34, 145, 60),
                    (60, 3, 32, 145, 45),
                    (90, 2, 34, 145, 60)
                ]
            );
            let effects = crate::battle::effects::cook(
                &extracted,
                &recipe.pulses.iter().map(|p| p.projectile).collect(),
            )
            .unwrap();
            for (id, reaction) in [(2, 12), (3, 11)] {
                let p = effects
                    .projectile(EffectId {
                        bank: EffectBank::Magic(5),
                        id,
                    })
                    .unwrap();
                assert_eq!(
                    (p.lifetime, p.shape.radius, p.shape.height, p.shape.reaction),
                    (10, 250., 300., reaction)
                );
                assert!(
                    p.persist_after_hit
                        && p.spawn_effect.is_none()
                        && p.trail_effect.is_none()
                        && p.shadow.is_none()
                );
                assert_eq!(p.birth_bank, Some(EffectBank::Magic(5)));
            }
        }
        assert_eq!(aliases, [(67, vec![3]), (214, vec![6, 9])]);
        let at = rel.sections[1].0 + 0x75293;
        rel.bytes[at] = 31;
        assert!(
            read_parameters(&rel).is_err(),
            "changed source timing must not retain old pulses"
        );
    }
}
