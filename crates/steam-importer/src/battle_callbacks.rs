//! Executable-owned launch callbacks normalized to ordinary martial events.
use crate::{battle_normal::canonical, battle_projectiles, battle_scenes, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_martial::{Attack, gameplay_tick},
    battle_normal::Event,
    battle_projectile::{DamageKind, ProjectileOrigin},
};
mod finishers;

/// Both native owners share the verified callback's definitions; enemy clocks
/// retain raw ages so their own pose holds are applied by the shared compiler.
pub(crate) fn enemy_final_fury(
    source: &Source,
) -> Result<Vec<(u16, resonance_content::battle_action::EnemyEvent)>> {
    let catalogue = crate::battle_martial::read(source)?;
    let attack = catalogue.attack(200, 8)?;
    let mut launches = attack
        .events
        .iter()
        .filter_map(|(_, event)| matches!(event, Event::EmitProjectile { .. }).then_some(event));
    let mut events = Vec::new();
    for raw_at in [22, 352] {
        events.push((
            raw_at,
            resonance_content::battle_action::EnemyEvent::AuthoredCallback {
                skill: 200,
                event: Box::new(
                    launches
                        .next()
                        .context("missing Final Fury callback projectile")?
                        .clone(),
                ),
            },
        ));
    }
    ensure!(
        launches.next().is_none(),
        "extra Final Fury callback projectile"
    );
    for (at, event) in [
        (0, Event::TransitionProtection(600)),
        (
            0,
            Event::ActionScene {
                party_protection: 480,
            },
        ),
        (308, Event::Voice(crate::battle_voices::sound(0x5d7)?)),
    ] {
        events.push((
            at,
            resonance_content::battle_action::EnemyEvent::AuthoredCallback {
                skill: 200,
                event: Box::new(event),
            },
        ));
    }
    Ok(events)
}
mod guardian_field;
mod holy_binds;

/// A source bank-six contact belongs to this action's stored skill. Preserve
/// its member while making the shared decoder's local selector canonical.
pub(crate) fn canonical_hits(member: u16, rules: &mut [u8]) -> Result<()> {
    if member == 152 {
        let row = rules
            .get_mut(28..56)
            .context("missing Final Fury contact")?;
        ensure!(
            row[22] == 2 && row[24] == 6 && row[25..28] == [0; 3],
            "Final Fury local skill contact changed"
        );
        row[24] = 0;
    }
    Ok(())
}

pub(crate) fn attach(
    source: &Source,
    image: &pe::Image<'_>,
    member: u16,
    bytes: &[u8],
    attack: &mut Attack,
) -> Result<()> {
    match member {
        51 => grand_fall(source, image, bytes, attack),
        148 | 151 => holy_binds::attach(source, image, member, bytes, attack),
        150 => guardian_field::attach(image, bytes, attack),
        146 | 147 | 152 => finishers::attach(source, image, member, bytes, attack),
        _ => Ok(()),
    }
}

fn grand_fall(
    source: &Source,
    image: &pe::Image<'_>,
    bytes: &[u8],
    attack: &mut Attack,
) -> Result<()> {
    let descriptor = pe::word(image.slice(0x8e98d0 + 51 * 4, 4)?, 0)?;
    let handlers = image.slice(descriptor, 8)?;
    ensure!(
        pe::word(handlers, 0)? == 0x4092a0
            && pe::word(handlers, 4)? == 0x427f20
            && pe::word(image.slice(0x4092bb, 4)?, 0)? == 0x4092d0,
        "Grand Fall action handlers changed"
    );
    for (address, expected) in [
        (0x4092d9, &[0x66, 0x83, 0xba, 0xbe, 1, 0, 0, 0x1e][..]),
        (0x4092f5, &[0xd9, 5, 0xfc, 0x2e, 0x88, 0]),
        (0x40933e, &[0x8d, 0x83, 0x90, 1, 0, 0]),
        (0x409356, &[0xb1, 8]),
    ] {
        ensure!(
            image.slice(address, expected.len())? == expected,
            "Grand Fall launch callback changed at {address:#x}"
        );
    }
    let skill = pe::word(image.slice(0x8d84b0 + 51 * 96 + 0x58, 4)?, 0)? + 1;
    let name = image.text(pe::word(image.slice(0x8e98a8 + skill * 4, 4)?, 0)?)?;
    let package = source.require(name)?;
    let projectiles =
        battle_scenes::member(&package, 0x18c)?.context("missing Grand Fall projectiles")?;
    let row = projectiles
        .get(400..800)
        .context("missing Grand Fall projectile one")?;
    let reset_scale = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    ensure!(reset_scale.is_finite(), "invalid projectile reset scale");
    let mut definition = resonance_importer::decode_projectile(
        &battle_projectiles::canonical_record(row)?,
        reset_scale,
    )?;
    ensure!(
        definition.unsupported_reason.is_none(),
        "unsupported Grand Fall projectile"
    );
    // Callback 0x4092d0 samples the target's X/Z, replaces world Y with 400,
    // and uses the stored skill bank for the falling object's effect programs.
    definition.motion.origin = ProjectileOrigin::TargetFloor;
    let height = f32::from_bits(pe::word(image.slice(0x882efc, 4)?, 0)?);
    ensure!(height.is_finite(), "invalid Grand Fall launch height");
    definition.spawn_offset[1] += height;
    definition.birth_effect.bank = 2;
    definition.trail_effect.bank = 2;
    let kind = match row[17] {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        value => anyhow::bail!("invalid Grand Fall projectile damage kind {value}"),
    };
    let rules = bytes
        .get(pe::word(bytes, 0)? as usize..pe::word(bytes, 4)? as usize)
        .context("missing Grand Fall hit rules")?;
    let rules = canonical(rules, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
    let hit = resonance_importer::decode_martial_hit(&rules, 0, kind, row[19])?;
    let poses = bytes
        .get(pe::word(bytes, 8)? as usize..pe::word(bytes, 12)? as usize)
        .context("missing Grand Fall poses")?;
    let mut holds = Vec::new();
    for row in poses.chunks_exact(12) {
        let at = pe::half(row, 0)? as i16;
        if at == -2 {
            break;
        }
        if at >= 0 && row[2] < 254 {
            holds.push((at as u16, row[3]));
        }
    }
    attack.events.push((
        gameplay_tick(30, 0, &holds)?,
        Event::EmitProjectile {
            definition: Box::new(definition),
            hit,
            feedback: None,
        },
    ));
    attack.events.sort_by_key(|(at, _)| *at);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use resonance_content::battle_scene::SceneAction;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; compiles the three stored finisher callbacks"]
    fn installed_finishers_preserve_skill_hits_and_admission() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let catalogue = crate::battle_martial::read(&source)?;
        let restored = resonance_importer::martial_attack_document(&catalogue)?.prepare()?;
        for (technique, character, member, count, protection) in [
            (260, 7, 146, 2, 345),
            (261, 7, 147, 5, 660),
            (200, 8, 152, 2, 600),
        ] {
            let binding = restored.bindings[&technique][&character];
            assert_eq!(binding.member, member);
            let attack = restored
                .variant(binding)
                .with_context(|| format!("finisher{member}"))?;
            assert!(attack.chain_at.is_none());
            assert_eq!(
                attack
                    .events
                    .iter()
                    .filter(|(_, e)| matches!(e, Event::EmitProjectile { .. }))
                    .count(),
                count
            );
            assert!(
                attack.events.iter().any(|(at, e)| *at == 0
                    && matches!(e,Event::TransitionProtection(n) if *n==protection))
            );
            assert!(
                attack
                    .events
                    .iter()
                    .any(|(at, e)| *at == 0 && matches!(e, Event::ActionScene { .. }))
            );
            if member == 152 {
                assert!(
                    attack
                        .events
                        .iter()
                        .any(|(_, e)| matches!(e,Event::Contact(c) if c.hit.effect==Some(2)))
                );
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; imports Guardian Field's damage and allied recovery"]
    fn installed_guardian_field_preserves_pulses_and_allied_recovery() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let catalogue = crate::battle_martial::read(&source)?;
        let binding = catalogue.bindings[&152][&9];
        assert_eq!((binding.member, binding.variant), (150, 0));
        let restored = resonance_importer::martial_attack_document(&catalogue)?.prepare()?;
        let attack = restored.variant(binding)?;
        let pulses = attack
            .events
            .iter()
            .filter_map(|(at, event)| match event {
                Event::EmitProjectile {
                    definition, hit, ..
                } => Some((*at, definition, hit)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pulses.len(), 3);
        assert_eq!(
            [pulses[1].0 - pulses[0].0, pulses[2].0 - pulses[1].0],
            [10, 10]
        );
        for (_, definition, hit) in &pulses {
            assert_eq!(definition.lifetime, Some(8));
            assert_eq!(definition.velocity, [0.; 3]);
            assert_eq!(
                definition.contact.shape,
                resonance_content::battle_projectile::HitShape::Cylinder
            );
            assert_eq!(
                definition.contact.recoil_direction,
                resonance_content::battle_projectile::RecoilDirection::AwayFromOwner
            );
            assert_eq!(hit.kind, DamageKind::Slash);
        }
        let recoveries = attack
            .events
            .iter()
            .filter_map(|(at, event)| match event {
                Event::HealOtherAllies { radius, percent } => Some((*at, *radius, *percent)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(recoveries, [(pulses[0].0 + 6, 220., 5)]);
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; imports both Holy Binds callbacks and projectile hit rows"]
    fn installed_holy_binds_uses_stored_projectiles_and_floor_origins() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let catalogue = crate::battle_martial::read(&source)?;
        let document = resonance_importer::martial_attack_document(&catalogue)?;
        let restored = document.prepare()?;
        for (technique, character, member) in [(249, 9, 148), (153, 6, 151)] {
            let binding = catalogue.bindings[&technique][&character];
            assert_eq!(binding.member, member);
            let attack = restored.variant(binding)?;
            let launches = attack
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::EmitProjectile {
                        definition,
                        hit,
                        feedback,
                    } => Some((*at, definition, hit, feedback)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(launches.len(), 4);
            assert_eq!(
                launches.iter().map(|row| row.0).collect::<Vec<_>>(),
                if technique == 249 {
                    vec![42, 96, 233, 297]
                } else {
                    vec![20, 54, 209, 273]
                }
            );
            assert!(launches.windows(2).all(|pair| pair[0].0 < pair[1].0));
            for (index, (_, definition, hit, feedback)) in launches.iter().enumerate() {
                assert_eq!(
                    definition.motion.origin,
                    if index >= 2 {
                        ProjectileOrigin::ActorFloor
                    } else {
                        ProjectileOrigin::Actor
                    }
                );
                assert_eq!(definition.spawn_offset[1], [0., 150., 200., 250.][index]);
                assert_eq!(hit.kind, DamageKind::Slash);
                assert!(feedback.is_none());
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; compiles Grand Fall's actual callback into the shared timeline"]
    fn installed_grand_fall_uses_shared_projectile_event() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let catalogue = crate::battle_martial::read(&source)?;
        let binding = catalogue
            .bindings
            .get(&51)
            .and_then(|rows| rows.get(&2))
            .context("missing Colette Grand Fall binding")?;
        assert_eq!((binding.member, binding.variant), (51, 0));
        let action = catalogue.variant(*binding)?;
        let launches = action
            .events
            .iter()
            .filter_map(|(at, event)| match event {
                Event::EmitProjectile {
                    definition,
                    hit,
                    feedback,
                } => Some((*at, definition, hit, feedback)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(launches.len(), 1);
        let (at, definition, hit, feedback) = launches[0];
        assert_eq!(at, 33);
        assert_eq!(definition.motion.origin, ProjectileOrigin::TargetFloor);
        // The native 400-unit world placement also retains the template's
        // authored 50-unit offset.
        assert_eq!(definition.spawn_offset[1], 450.);
        assert_eq!(
            (definition.birth_effect.bank, definition.trail_effect.bank),
            (2, 2)
        );
        assert!(feedback.is_none());
        assert_eq!(hit.kind, DamageKind::Magic);
        let document = resonance_importer::martial_attack_document(&catalogue)?;
        let restored = document.prepare()?;
        assert!(matches!(
            restored.members[&51].variants[&0],
            SceneAction::Prepared(_)
        ));
        let restored = restored.variant(*binding)?;
        assert!(
            restored
                .events
                .iter()
                .any(|(at, event)| *at == 33 && matches!(event, Event::EmitProjectile { .. }))
        );
        Ok(())
    }
}
