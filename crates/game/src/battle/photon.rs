//! Light and dark orbs release two contacts at the target's current center.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{PreparedSpell, SpellOrigin, SpellStartup};
use resonance_content::{battle_scene::Scene, prepared::Files};

pub const CATALOGUE: u16 = 113;

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    const LIFETIME: u16 = 165;
    let recoil = recoil::Parameters::load(files)?;
    let mut projectiles = Vec::with_capacity(2);
    for (at, row, variant) in [(10, 1, 0), (60, 2, 1)] {
        let source = scene.projectiles.get(row).context("missing orb contact")?;
        let rule = source.hit_variant(variant)?;
        let projectile = resources.scene_projectile(
            files,
            &source.projectile,
            rule,
            &recoil,
            bank,
            &[(0, bank)],
        )?;
        projectiles.push(resonance_battle::SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + at + 1,
            projectile,
        ));
    }
    let startup = resources.effect_member(bank, 1)?;
    Ok(PreparedSpell {
        projectiles,
        startup: Some((TRANSITION_TICKS + 1, SpellStartup::DeferredEffect(startup))),
        ..stored_spells::scene_spell(scene, SpellOrigin::FollowingTarget, LIFETIME)?
    })
}
