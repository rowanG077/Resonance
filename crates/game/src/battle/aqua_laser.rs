//! A forward water jet starts ahead of the caster and carries its own artwork.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{EffectAppearance, PreparedSpell, SpellOrigin, SpellProjectile};
use resonance_content::{battle_scene::Scene, prepared::Files};
use std::sync::Arc;

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
    let mut hit = super::hit::authored(rule, &recoil::Parameters::load(files)?)?;
    if let Some(member) = rule.effect {
        let mut effects = bank.clone();
        effects.members = vec![member];
        resources.effect(effects)?;
        hit.effect = Some(EffectAppearance {
            resource: bank.resource,
            member,
        });
    }
    let mut projectile = resources.projectile(files, &source.projectile, hit, &[(0, bank)])?;
    Arc::make_mut(&mut projectile)
        .contact
        .as_mut()
        .context("Aqua Laser projectile has no contact")?
        .cooldown = rule.cooldown;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles: vec![SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + RELEASE_TICK,
            projectile,
        )],
        recovery: vec![],
        origin: SpellOrigin::SceneCasterAhead {
            distance: 80.,
            height: 100.,
        },
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, LIFETIME)?,
        camera: stored_spells::scene_camera(scene.camera, LIFETIME)?,
        duration: TRANSITION_TICKS + LIFETIME + SCENE_BOUNDARY_TICKS,
        startup: None,
        sound: None,
    })
}
