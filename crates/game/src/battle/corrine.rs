//! Corrine lands on the selected enemy, returns to the caster, then departs.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{EffectAppearance, PreparedSpell, SpellOrigin, SpellProjectile};
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
    let mut hit = super::hit::authored(rule, &recoil::Parameters::load(files)?)?;
    if let Some(member) = rule.effect {
        let mut required = bank.clone();
        required.members = vec![member];
        resources.effect(required)?;
        hit.effect = Some(EffectAppearance {
            resource: bank.resource,
            member,
        });
    }
    let mut projectile = resources.projectile(
        files,
        &source.projectile,
        hit,
        &[(0, common), (1, techniques), (2, bank), (6, bank)],
    )?;
    let definition = Arc::make_mut(&mut projectile);
    definition
        .contact
        .as_mut()
        .context("Corrine projectile has no contact")?
        .cooldown = rule.cooldown;
    definition.effects.departure_animation = Some(ModelAnimation {
        model: 0,
        clip: 0,
        frame: 0.,
        rate: 0.5,
        repeat: true,
        blend_ticks: 4,
    });
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        projectiles: vec![SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + APPEAR_TICK,
            projectile,
        )],
        effects: vec![],
        recovery: vec![],
        origin: SpellOrigin::SceneAboveTarget,
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, ACTIVE_SCENE_TICKS)?,
        camera: stored_spells::scene_camera(scene.camera, ACTIVE_SCENE_TICKS)?,
        duration: TRANSITION_TICKS + ACTIVE_SCENE_TICKS + SCENE_BOUNDARY_TICKS,
        startup: None,
        sound: None,
    })
}
