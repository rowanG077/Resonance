//! Decode Wind Blade's shared projectile template and native field overrides.
use super::*;
use resonance_content::battle::{
    actions::wind_blade::WindBladeRecipe,
    effects::{EffectBank, KnockbackDirection, ProjectileRecipe},
};

pub(super) fn cook(
    parameters: &ordinary_parameters::WindBlade,
    source: &bundle::Bundle,
    definition: &Definition,
    shared_contact: ProjectileRecipe,
) -> Result<WindBladeRecipe> {
    ensure!(
        definition.native_id == 208 && definition.flags == 0x00444186,
        "unexpected Wind Blade technique binding"
    );
    ensure!(
        source
            .phases
            .iter()
            .skip(1)
            .all(|phase| phase.duration == 0),
        "unsupported Wind Blade phase variants"
    );
    let recipe = WindBladeRecipe {
        lifetime: source.phases[0].duration,
        effect_tick: parameters.effect_tick,
        effect: parameters.effect,
        pulses: VolleySchedule {
            first_tick: 10,
            interval: 10,
            count: 3,
        },
        contact: contact(shared_contact, parameters.contact_size)?,
        rule: source.rule(0)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn validate_controller(rel: &Rel) -> Result<()> {
    let dispatch = rel.pointer(DATA, 0x1238 + 8 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x64660)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37e48)
            && rel.local_targets().contains(&(1, 0x64528)),
        "unexpected Wind Blade native dispatch"
    );
    for (at, expected) in [
        (0x64688, 0x8064195c), // initializer retains target's sampled body anchor
        (0x64550, 0x2c000005),
        (0x64580, 0x38800001),
        (0x64588, 0x38a0001b),
        (0x645bc, 0x2c04000a),
        (0x645c4, 0x2c040023),
        (0x645d0, 0x3884fff6),
        (0x645e8, 0x1c00000a),
        (0x64620, 0x38e0000a),
        (0x64628, 0x39000000),
        (0x6462c, 0x39200002),
        (0x64630, 0x39400006),
        (0x64640, 0x4bfbbe89), // shared contact allocator
        (0x20518, 0x816b5fa4),
        (0x2052c, 0x386b0190),
        (0x20560, 0x38800001),
        (0x20564, 0x38a00001),
        (0x20574, 0xb37f000c),
        (0x20578, 0x9bdf0013),
        (0x2057c, 0x9b9f0015),
        (0x20580, 0x9bbf0016),
        (0x20584, 0xd3df0044),
        (0x20588, 0xd3ff0048),
    ] {
        ensure!(
            word(rel.at((1, at))?, 0)? == expected,
            "unexpected Wind Blade callback/helper at {at:#x}"
        );
    }
    Ok(())
}

fn contact(mut contact: ProjectileRecipe, size: f32) -> Result<ProjectileRecipe> {
    contact.birth_bank = Some(EffectBank::Techniques);
    contact.lifetime = 10;
    contact.shape.reaction = 6;
    contact.shape.kind = HitShapeKind::Box;
    contact.knockback = KnockbackDirection::AwayFromProjectile;
    contact.shape.radius = size;
    contact.shape.height = size;
    contact.validate()?;
    Ok(contact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{EffectId, ProjectileMovement};

    #[test]
    fn shared_contact_keeps_magic_classification_and_outward_knockback() {
        let bytes = concat!(
            "000000000000000000000409000a000000020001010000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "3f40000000000000000000000000000000000000000000000000000002000200",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000800000000000000000000000000000000000000000",
        );
        let mut row = [0; 400];
        for (i, pair) in bytes.as_bytes().chunks_exact(2).enumerate() {
            row[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        let bind = |row: &[u8]| {
            crate::battle::effects::projectile(
                row,
                EffectId {
                    bank: EffectBank::Techniques,
                    id: 1,
                },
                0.001,
            )
        };
        let recipe = contact(bind(&row).unwrap(), 50.).unwrap();
        assert_eq!(
            recipe.id,
            Some(EffectId {
                bank: EffectBank::Techniques,
                id: 1
            })
        );
        assert_eq!(recipe.birth_bank, Some(EffectBank::Techniques));
        assert!(matches!(
            recipe.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 0., 0.],
                acceleration: [0., 0., 0.],
                steering: None,
            }
        ));
        assert_eq!(
            (recipe.lifetime, recipe.shape.radius, recipe.shape.height),
            (10, 50., 50.)
        );
        assert_eq!(
            (
                recipe.shape.damage_kind,
                recipe.shape.hit_class,
                recipe.shape.reaction
            ),
            (2, 0, 6)
        );
        assert!(matches!(recipe.shape.kind, HitShapeKind::Box));
        assert!(matches!(
            recipe.knockback,
            KnockbackDirection::AwayFromProjectile
        ));
        assert!(recipe.persist_after_hit && !recipe.clashable && recipe.shadow.is_none());
        assert!(
            recipe.spawn_effect.is_none()
                && recipe.trail_effect.is_none()
                && recipe.ground_effect.is_none()
        );
        row[8..12].copy_from_slice(&0x409_u32.wrapping_add(0x800).to_be_bytes());
        assert!(
            bind(&row).and_then(|source| contact(source, 50.)).is_err(),
            "unrecovered template behavior remains an error"
        );
    }

    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn wind_blade_recovers_three_contacts_and_the_independent_visual() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
        let catalogue = crate::arte::read(&executable).unwrap();
        let definition = catalogue.definition(70).unwrap();
        let source =
            bundle::Bundle::decode(member(member(&usual, 9).unwrap(), 8).unwrap()).unwrap();
        let recipe = cook(
            &ordinary_parameters::Parameters::read(&rel)
                .unwrap()
                .wind_blade,
            &source,
            definition,
            source_contact(&rel, &usual).unwrap(),
        )
        .unwrap();
        assert_eq!(
            (recipe.lifetime, recipe.effect_tick, recipe.effect.id),
            (90, 5, 27)
        );
        assert_eq!(
            (0..=90)
                .filter(|&tick| recipe.pulses.shot(tick).is_some())
                .collect::<Vec<_>>(),
            [10, 20, 30]
        );
        assert_eq!(
            (
                recipe.rule.power_mode,
                recipe.rule.power,
                recipe.rule.hitstun,
                recipe.rule.contact_cooldown
            ),
            (1, 55, 20, 30)
        );
        assert!(matches!(
            recipe.rule.element,
            HitElement::Element(resonance_content::menu_data::Element::Wind)
        ));
        let mut wrong = definition.clone();
        wrong.native_id = 209;
        assert!(
            cook(
                &ordinary_parameters::Parameters::read(&rel)
                    .unwrap()
                    .wind_blade,
                &source,
                &wrong,
                source_contact(&rel, &usual).unwrap()
            )
            .is_err()
        );
    }
}
