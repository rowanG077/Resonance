//! A forward water jet starts ahead of the caster and carries its own artwork.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, CONTACT_INITIALIZATION_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{PreparedSpell, SpellOrigin, SpellProjectile};
use resonance_content::{battle_scene::Scene, prepared::Files};

pub const CATALOGUE: u16 = 65;

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    const LIFETIME: u16 = 160;
    const RELEASE_TICK: u16 = 30;
    let source = scene
        .projectiles
        .get(1)
        .context("missing Aqua Laser projectile")?;
    let rule = source.hit()?;
    let projectile = resources.scene_projectile(
        files,
        &source.projectile,
        rule,
        &recoil::Parameters::load(files)?,
        bank,
        &[(0, bank)],
    )?;
    Ok(PreparedSpell {
        projectiles: vec![SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + RELEASE_TICK + CONTACT_INITIALIZATION_TICKS,
            projectile,
        )],
        ..stored_spells::scene_spell(
            scene,
            SpellOrigin::SceneCasterAhead {
                distance: 80.,
                height: 100.,
            },
            LIFETIME,
        )?
    })
}
