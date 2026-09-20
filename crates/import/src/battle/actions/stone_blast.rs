//! Stone Blast applies its cooked dimensions and pulse schedule to a shared contact template.
use super::*;
use resonance_content::battle::{
    actions::stone_blast::StoneBlastRecipe,
    effects::{EffectBank, EffectId, KnockbackDirection},
};

pub(super) fn cook(
    parameters: &ordinary_parameters::GroundContact,
    source: &bundle::Bundle,
    definition: &Definition,
    mut contact: ProjectileRecipe,
) -> Result<StoneBlastRecipe> {
    ensure!(
        definition.native_id == 212 && definition.flags == 0x00440186,
        "unexpected Stone Blast technique binding"
    );
    ensure!(
        source
            .phases
            .iter()
            .skip(1)
            .all(|phase| phase.duration == 0)
            && source.rule_count() == 1,
        "unexpected Stone Blast phase/rule closure"
    );
    contact.birth_bank = Some(EffectBank::Techniques);
    contact.lifetime = 10;
    contact.shape.reaction = 6;
    contact.shape.kind = HitShapeKind::Cylinder;
    contact.knockback = KnockbackDirection::Velocity;
    contact.shape.radius = parameters.radius;
    contact.shape.height = parameters.height;
    let recipe = StoneBlastRecipe {
        lifetime: source.phases[0].duration,
        origin: parameters.origin,
        effect: EffectId {
            bank: EffectBank::Techniques,
            id: 21,
        },
        effect_scale: parameters.effect_scale,
        pulses: VolleySchedule {
            first_tick: 30,
            interval: 5,
            count: 3,
        },
        contact,
        rule: source.rule(0)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn validate_controller(rel: &Rel) -> Result<()> {
    let dispatch = rel.pointer(DATA, 0x1238 + 12 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x60f78)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37e48)
            && rel.local_targets().contains(&(1, 0x60ecc)),
        "unexpected Stone Blast dispatch"
    );
    // Pin the complete recovered callbacks/helper, including the age range/modulus,
    // copied target anchor, pool7 template1, and every post-allocation override.
    for (offset, size, digest) in [
        (
            0x60ecc,
            0xac,
            "cdc1bcdac9ecd60bfa9b6018532016aaf40a22c1809a3ea8e1e9bb80fef0fd0c",
        ),
        (
            0x60f78,
            0xa0,
            "55cb185be2f64adb826c26855506ef15e59b9b05af9c977942b5fb175b7499d4",
        ),
        (
            0x204c8,
            0xe4,
            "0b0b3660c1fed0b113e511b21f06024e5197ad0782c1d46399b10ae5eef04901",
        ),
        (
            0x37ed0,
            0x104,
            "be2bbe2ecebd4d1c73aded956430fd0410c293984067b2c23334721a992eafec",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Stone Blast callback")?
            ) == digest,
            "unrecovered Stone Blast callback operation at {offset:#x}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires privately extracted US assets; source recipe recovery only"]
    fn original_earth_aliases_keep_distinct_contacts_and_every_enemy_binding() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let techniques = technique_actions(&extracted, &rel, &usual, &[74, 75, 217]).unwrap();
        for technique in &techniques {
            match &technique.program {
                TechniqueProgram::StoneBlast { casters, recipe } => {
                    assert_eq!(
                        casters.iter().map(|c| c.character).collect::<Vec<_>>(),
                        if technique.technique == 74 {
                            vec![3]
                        } else {
                            vec![6, 9]
                        }
                    );
                    assert_eq!(
                        (recipe.lifetime, recipe.rule.power, recipe.contact.lifetime),
                        (90, 65, 10)
                    );
                    assert_eq!(
                        (recipe.contact.shape.radius, recipe.contact.shape.height),
                        (85., 250.)
                    );
                    assert!(matches!(recipe.contact.shape.kind, HitShapeKind::Cylinder));
                    assert!(matches!(
                        recipe.contact.knockback,
                        KnockbackDirection::Velocity
                    ));
                    assert_eq!(
                        (0..90)
                            .filter(|&age| recipe.pulses.shot(age).is_some())
                            .collect::<Vec<_>>(),
                        [30, 35, 40]
                    );
                    assert_eq!(
                        recipe.origin.capture([0., 500., 0.], [300., 200., 400.]),
                        [299.4, 0., 399.2]
                    );
                }
                TechniqueProgram::EarthField {
                    casters, recipe, ..
                } => {
                    assert_eq!(casters.iter().map(|c| c.character).collect::<Vec<_>>(), [3]);
                    assert_eq!((recipe.kind as u16, recipe.lifetime), (213, 245));
                    assert_eq!(
                        recipe
                            .pulses
                            .iter()
                            .map(|p| (p.tick, p.projectile.id, p.rule.power, p.rule.sound))
                            .collect::<Vec<_>>(),
                        [(44, 0, 95, 58), (60, 1, 95, 58), (90, 2, 95, 58)]
                    );
                    let archive =
                        crate::battle::effect_program::MagicArchive::read(&extracted).unwrap();
                    let bytes = crate::battle::effect_program::magic_member(
                        archive.package(13).unwrap(),
                        252,
                    )
                    .unwrap()
                    .unwrap();
                    for pulse in &recipe.pulses {
                        let at = usize::from(pulse.projectile.id) * 400;
                        let flight = crate::battle::effects::projectile(
                            &bytes[at..at + 400],
                            pulse.projectile,
                            0.5,
                        )
                        .unwrap();
                        assert_eq!(flight.birth_bank, Some(EffectBank::Magic(13)));
                        assert!(flight.persist_after_hit && flight.shadow.is_none());
                    }
                }
                _ => panic!("Earth alias lost typed native identity"),
            }
            assert_eq!(technique.enemy_spell().unwrap() as u16, technique.native_id);
        }
        let inventory = crate::battle::action_inventory::recover(&extracted).unwrap();
        assert_eq!(inventory.artes.artes.len(), 253);
        assert_eq!(inventory.enemies.packages.len(), 251);
        let bindings = inventory
            .enemies
            .packages
            .iter()
            .flat_map(|e| {
                e.actions.iter().filter_map(move |a| {
                    a.native_technique
                        .filter(|n| matches!(n, 212 | 213))
                        .map(|n| (e.monster, a.id, n))
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bindings,
            [
                (43, 4, 212),
                (62, 1, 213),
                (68, 2, 212),
                (69, 2, 212),
                (76, 2, 213),
                (93, 3, 212),
                (106, 4, 212),
                (194, 9, 213),
                (196, 8, 213),
                (196, 9, 212),
                (210, 5, 213),
                (228, 1, 213),
                (228, 6, 212),
                (229, 4, 213),
                (229, 5, 212)
            ]
        );
        let text = rel.sections[1].0;
        rel.bytes[text + 0x60ecc + 0x30] ^= 1;
        assert!(
            validate_controller(&rel).is_err(),
            "a changed native operation cannot silently retain the old recipe"
        );
    }
}
