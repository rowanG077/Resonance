//! Corrine lands on the selected enemy, returns to the caster, then departs.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{PreparedSpell, SpellOrigin, SpellProjectile};
use resonance_content::{battle_effect::ModelAnimation, battle_scene::Scene, prepared::Files};
use std::sync::Arc;

pub const CORRINE: u16 = 139;
pub const HEART: u16 = 245;
pub const SCENE: u16 = 282;
const APPEAR_TICK: u16 = 11;
// Corrine and Heart initialize the same independent scene timer. Projectile
// appearance, flight and departure do not determine when that scene retires.
const ACTIVE_SCENE_TICKS: u16 = 210;

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    common: &EffectResource,
    techniques: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let source = scene
        .projectiles
        .get(1)
        .context("missing Corrine projectile")?;
    let rule = source.hit()?;
    let mut projectile = resources.scene_projectile(
        files,
        &source.projectile,
        rule,
        &recoil::Parameters::load(files)?,
        bank,
        &[(0, common), (1, techniques), (2, bank), (6, bank)],
    )?;
    let definition = Arc::make_mut(&mut projectile);
    definition.effects.departure_animation = Some(ModelAnimation {
        model: 0,
        clip: 0,
        frame: 0.,
        rate: 0.5,
        repeat: true,
        blend_ticks: 4,
    });
    Ok(PreparedSpell {
        projectiles: vec![SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + APPEAR_TICK,
            projectile,
        )],
        ..stored_spells::scene_spell(scene, SpellOrigin::SceneAboveTarget, ACTIVE_SCENE_TICKS)?
    })
}
