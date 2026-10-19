//! Light and dark orbs release two contacts at the target's current center.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{EffectAppearance, PreparedSpell, SpellOrigin, SpellStartup};
use resonance_content::{battle_scene::Scene, prepared::Files};
use std::sync::Arc;

pub const CATALOGUE: u16 = 113;

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    const LIFETIME: u16 = 165;
    let recoil = recoil::Parameters::load(files)?;
    let mut effects = bank.clone();
    effects.members = vec![1];
    let mut projectiles = Vec::with_capacity(2);
    for (at, row, variant) in [(10, 1, 0), (60, 2, 1)] {
        let source = scene.projectiles.get(row).context("missing orb contact")?;
        let rule = source.hit_variant(variant)?;
        let mut hit = super::hit::authored(rule, &recoil)?;
        if let Some(member) = rule.effect {
            effects.members.push(member);
            hit.effect = Some(EffectAppearance {
                resource: bank.resource,
                member,
            });
        }
        let mut projectile = resources.projectile(files, &source.projectile, hit, &[(0, bank)])?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("orb projectile has no contact")?
            .cooldown = rule.cooldown;
        projectiles.push(resonance_battle::SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + at + 1,
            projectile,
        ));
    }
    resources.effect(effects)?;
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles,
        recovery: vec![],
        origin: SpellOrigin::FollowingTarget,
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, LIFETIME)?,
        camera: stored_spells::scene_camera(scene.camera, LIFETIME)?,
        duration: TRANSITION_TICKS + LIFETIME + SCENE_BOUNDARY_TICKS,
        startup: Some((
            TRANSITION_TICKS + 1,
            SpellStartup::DeferredEffect(EffectAppearance {
                resource: bank.resource,
                member: 1,
            }),
        )),
        sound: None,
    })
}
