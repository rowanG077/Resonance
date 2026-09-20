//! Recover the stored native227 initializer, three visual satellites and one contact.
use super::*;
#[cfg(test)]
use crate::battle::effect_program::{MagicArchive, magic_member};
use resonance_content::battle::actions::{
    lightning::GroundSpellOrigin, thunder_arrow::ThunderArrowRecipe,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    lifetime: u16,
    origin: GroundSpellOrigin,
    presentation: StoredSpellPresentation,
    heading_offset: f32,
    effect_scale: f32,
    satellite_tick: u16,
    satellite_count: u8,
    satellite_step: f32,
    satellite_radius: f32,
    satellite_heading: f32,
    projectile_tick: u16,
}

pub(super) fn cook(tables: &Tables, arte: &Definition) -> Result<ThunderArrowRecipe> {
    ensure!(
        arte.native_id as u16 == 227 && arte.flags == 0x00440193,
        "unexpected Thunder Arrow binding"
    );
    let bundle = tables.bundle(227)?;
    ensure!(
        bundle.phase(0)?.duration == 180
            && bundle.phases[1..].iter().all(|phase| phase.duration == 0)
            && bundle.rule_count() == 1,
        "unexpected Thunder Arrow action phases or hit rules"
    );
    let p = &tables.elemental.thunder_arrow;
    let recipe = ThunderArrowRecipe {
        lifetime: p.lifetime,
        origin: p.origin,
        presentation: p.presentation,
        heading_offset: p.heading_offset,
        effect_scale: p.effect_scale,
        satellite_tick: p.satellite_tick,
        satellite_count: p.satellite_count,
        satellite_step: p.satellite_step,
        satellite_radius: p.satellite_radius,
        satellite_heading: p.satellite_heading,
        projectile_tick: p.projectile_tick,
        rule: bundle.phase_rule(0, 0)?,
    };
    recipe.validate()?;
    Ok(recipe)
}

pub(super) fn read_parameters(rel: &Rel) -> Result<Parameters> {
    let dispatch = rel.pointer(DATA, 0x1238 + 27 * 4)?;
    for (phase, handler) in [0x8a914, 0x37e48, 0x37dd8].into_iter().enumerate() {
        ensure!(
            rel.pointer(dispatch.0, dispatch.1 + phase * 4)? == (1, handler),
            "unexpected Thunder Arrow dispatch"
        );
    }
    ensure!(
        rel.local_targets().contains(&(1, 0x8a704)),
        "missing Thunder Arrow callback"
    );
    // Pin every operation, including captured +22.5-degree heading, all three
    // pool2 visual births, their world headings, and the distinct late pool2 contact.
    for (offset, size, digest) in [
        (
            0x8a704,
            0x210,
            "517e8b720d4da03f6964817dbbc7f93e1ca18b27ff6eac0baa3aadb60a505b3d",
        ),
        (
            0x8a914,
            0xfc,
            "45ceefee4a44138c8e84980ab89e44c06d47cc48b1bbc6eb73be1dfb2087b6e8",
        ),
        (
            0x37b10,
            0x194,
            "5f1fb01f4138f23580fa2464682d93b4d05f8c75883f8ccf9b42a66d1d3d3c3c",
        ),
        (
            0x205ac,
            0x158,
            "552cf2dc842171c40e9b3d0ac01bf778c1a025d8d145bb181eec9f657c39693f",
        ),
    ] {
        ensure!(
            crate::digest(
                rel.at((1, offset))?
                    .get(..size)
                    .context("truncated Thunder Arrow callback")?
            ) == digest,
            "unrecovered Thunder Arrow operation at {offset:#x}"
        );
    }
    let settings = rel.at((4, 0x87c0))?;
    let radians = float(settings, 12)?;
    Ok(Parameters {
        lifetime: 195,
        origin: GroundSpellOrigin {
            height: float(rel.at((4, 0x1c80))?, 0)?,
            nudge: 1.,
            direction_threshold: float(rel.at((4, 0x2800))?, 0)?,
        },
        presentation: StoredSpellPresentation {
            color: settings[..4].try_into()?,
            camera_distance: float(settings, 32)?,
            camera_elevation: float(settings, 36)?,
        },
        heading_offset: float(settings, 40)? * radians,
        effect_scale: float(settings, 8)?,
        satellite_tick: 10,
        satellite_count: 3,
        satellite_step: float(settings, 16)? * radians,
        satellite_radius: float(settings, 20)?,
        satellite_heading: float(settings, 4)?,
        projectile_tick: 60,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::effects::{EffectBank, KnockbackDirection, ProjectileMovement};

    #[test]
    #[ignore = "requires privately extracted US assets; original recipe and complete inventory recovery"]
    fn original_thunder_arrow_keeps_visual_satellites_separate_from_its_single_contact() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let actions = technique_actions(&extracted, &rel, &usual, &[89]).unwrap();
        let TechniqueProgram::ThunderArrow {
            casters, recipe, ..
        } = &actions[0].program
        else {
            panic!("lost native227 identity")
        };
        assert_eq!(casters.iter().map(|c| c.character).collect::<Vec<_>>(), [3]);
        assert_eq!(actions[0].enemy_spell().unwrap() as u16, 227);
        assert_eq!(
            (
                recipe.lifetime,
                recipe.satellite_tick,
                recipe.satellite_count,
                recipe.projectile_tick
            ),
            (195, 10, 3, 60)
        );
        assert!((recipe.heading_offset.to_degrees() - 22.5).abs() < 0.0001);
        assert!((recipe.satellite_step.to_degrees() - 120.).abs() < 0.0001);
        assert_eq!(
            (
                recipe.satellite_radius,
                recipe.satellite_heading,
                recipe.effect_scale
            ),
            (300., 0., 1.)
        );
        assert_eq!(recipe.presentation.color, [16, 16, 16, 255]);
        assert_eq!(
            (
                recipe.presentation.camera_distance,
                recipe.presentation.camera_elevation
            ),
            (2950., 18.)
        );
        assert_eq!(
            (
                recipe.rule.power,
                recipe.rule.contact_cooldown,
                recipe.rule.hitstun,
                recipe.rule.stun_chance
            ),
            (72, 8, 35, 10)
        );
        let archive = MagicArchive::read(&extracted).unwrap();
        let records = magic_member(archive.package(27).unwrap(), 252)
            .unwrap()
            .unwrap();
        let flight = crate::battle::effects::projectile(
            &records[400..800],
            ThunderArrowRecipe::effect(1),
            0.5,
        )
        .unwrap();
        assert_eq!(flight.lifetime, 80);
        assert_eq!((flight.shape.radius, flight.shape.height), (150., 600.));
        assert!(matches!(flight.shape.kind, HitShapeKind::Cylinder));
        assert!(matches!(flight.knockback, KnockbackDirection::Velocity));
        assert!(matches!(
            flight.movement,
            ProjectileMovement::Ballistic {
                velocity: [0., 0., 0.],
                acceleration: [0., 0., 0.],
                steering: None
            }
        ));
        assert_eq!(flight.spawn_offset, [0., 300., 0.]);
        assert_eq!(flight.birth_bank, Some(EffectBank::Magic(27)));
        assert!(
            flight.persist_after_hit
                && flight.shadow.is_none()
                && flight.spawn_effect.is_none()
                && flight.trail_effect.is_none()
        );
        let inventory = crate::battle::action_inventory::recover(&extracted).unwrap();
        assert_eq!(
            (
                inventory.artes.artes.len(),
                inventory.enemies.packages.len()
            ),
            (253, 251)
        );
        assert_eq!(
            inventory
                .artes
                .artes
                .iter()
                .filter(|a| a.native_id == 227)
                .map(|a| (a.id, a.owners.clone()))
                .collect::<Vec<_>>(),
            [(89, vec![3])]
        );
        assert_eq!(
            inventory
                .enemies
                .packages
                .iter()
                .flat_map(|e| e
                    .actions
                    .iter()
                    .filter(|a| a.native_technique == Some(227))
                    .map(move |a| (e.monster, e.variant_count, a.id)))
                .collect::<Vec<_>>(),
            [
                (203, 1, 6),
                (204, 1, 7),
                (208, 1, 7),
                (209, 1, 7),
                (234, 1, 12),
                (236, 1, 9),
                (237, 1, 8),
                (238, 1, 10),
                (240, 1, 10)
            ]
        );
        let text = rel.sections[1].0;
        rel.bytes[text + 0x8a704 + 0x30] ^= 1;
        assert!(
            read_parameters(&rel).is_err(),
            "changed native operations must not retain the old recipe"
        );
    }
}
