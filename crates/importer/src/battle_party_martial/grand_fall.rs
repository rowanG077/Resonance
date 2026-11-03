//! Grand Fall's native callback becomes an ordinary authored projectile event.
use super::*;
use crate::{battle_projectiles, battle_scene_package, dol, rel::Rel};
use resonance_content::battle_projectile::{DamageKind, ProjectileOrigin};
use std::path::Path;

pub(crate) fn attach(
    root: &Path,
    usual: &[u8],
    module: &Rel,
    catalogue: &mut Catalogue,
) -> Result<()> {
    ensure!(
        module.pointer(5, 0xe2c)? == (5, 0x5df0),
        "Grand Fall dispatcher changed"
    );
    for (at, target) in [(0x5df0, 0x80120), (0x5df4, 0x37fd4), (0x5df8, 0x3823c)] {
        ensure!(
            module.pointer(5, at)? == (1, target),
            "Grand Fall phase changed"
        );
    }
    // Initializer 1:80120 installs callback 1:800a0. At raw frame30 it
    // samples target X/Z, replaces Y with 400, and emits skill projectile1.
    for (at, expected) in [
        (0x80138, 0x3c600000),
        (0x8013c, 0x38030000),
        (0x80140, 0x901f0018),
        (0x800b8, 0x2c00001e),
        (0x800c4, 0x80a41990),
        (0x800d8, 0x38600003),
        (0x800e0, 0x38e00001),
        (0x8010c, 0x4bfa04a1),
    ] {
        ensure!(
            word(module.slice(1, at, 4)?, 0)? == expected,
            "Grand Fall callback changed at {at:#x}"
        );
    }
    let height = float(module.slice(4, 0x6750, 4)?, 0)?;
    ensure!(
        height.is_finite() && height > 0.,
        "invalid Grand Fall height"
    );
    let (package, _) = crate::battle_skill_package::package(root, 51)?;
    let rows =
        battle_scene_package::member(&package, 0xfc)?.context("missing Grand Fall projectiles")?;
    let row = rows
        .get(400..800)
        .context("missing Grand Fall projectile1")?;
    let reset = float(module.slice(4, 0xce8, 4)?, 0)?;
    let mut definition = battle_projectiles::scene_record(row, reset)?;
    ensure!(
        definition.unsupported_reason.is_none(),
        "unsupported Grand Fall projectile"
    );
    definition.motion.origin = ProjectileOrigin::TargetFloor;
    definition.spawn_offset[1] += height;
    definition.birth_effect.bank = 2;
    definition.trail_effect.bank = 2;
    let kind = match row[17] {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        value => anyhow::bail!("unknown Grand Fall damage kind {value}"),
    };
    let directory = member(usual, 8)?;
    let bytes = directory
        .get(word(directory, 4 + 51 * 4)? as usize..)
        .context("missing Grand Fall action")?;
    let rules = bytes
        .get(word(bytes, 0)? as usize..word(bytes, 4)? as usize)
        .context("missing Grand Fall hit rules")?;
    let hit = crate::battle_party_native::hit_rule(rules, 0, kind, row[19])?;
    let poses = bytes
        .get(word(bytes, 8)? as usize..word(bytes, 12)? as usize)
        .context("missing Grand Fall poses")?;
    let mut holds = Vec::new();
    for row in poses.chunks_exact(12) {
        let at = dol::u16(row, 0)? as i16;
        if at == -2 {
            break;
        }
        if at >= 0 && row[2] < 254 {
            holds.push((at as u16, row[3]));
        }
    }
    let attack = match catalogue
        .members
        .get_mut(&51)
        .and_then(|member| member.variants.get_mut(&0))
    {
        Some(SceneAction::Prepared(attack)) => attack,
        _ => anyhow::bail!("Grand Fall action51 is not prepared"),
    };
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
    #[test]
    #[ignore = "requires extracted GC source; optionally publishes only battle party rule owners"]
    fn native_grand_fall_publishes_its_callback_recipe() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("RESONANCE_TEST_EXTRACTED").context("set RESONANCE_TEST_EXTRACTED")?,
        );
        let usual = std::fs::read(root.join("disc1/files/BTL/BTLusual.dat"))?;
        let module = Rel::read(std::fs::read(root.join("disc1/files/US_r_Top2Btl.rel"))?)?;
        let mut catalogue = super::super::read(&usual, &module)?;
        attach(&root, &usual, &module, &mut catalogue)?;
        let document = super::super::document(&catalogue)?;
        let restored = document.prepare()?;
        let binding = restored.bindings[&51][&2];
        assert_eq!((binding.member, binding.variant), (51, 0));
        let attack = restored.variant(binding)?;
        let (at, definition, hit) = attack
            .events
            .iter()
            .find_map(|(at, event)| match event {
                Event::EmitProjectile {
                    definition, hit, ..
                } => Some((*at, definition, hit)),
                _ => None,
            })
            .context("missing Grand Fall launch")?;
        assert_eq!(at, 33);
        assert_eq!(definition.motion.origin, ProjectileOrigin::TargetFloor);
        assert_eq!(definition.spawn_offset[1], 450.);
        assert_eq!(
            (definition.birth_effect.bank, definition.birth_effect.member),
            (2, 2)
        );
        assert_eq!(hit.recoil, 8);
        assert_eq!(hit.kind, DamageKind::Magic);
        if let Some(library) = std::env::var_os("RESONANCE_TEST_LIBRARY") {
            crate::cook_battle_party_rules(&root, &std::path::PathBuf::from(library))?;
        }
        Ok(())
    }
}
