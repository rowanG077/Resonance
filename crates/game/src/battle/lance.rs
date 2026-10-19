//! Four elevated lances converge on one retained ground point.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, PreparedSpell, SpellEffect, SpellOrigin, SpellProjectile, SpellStartup,
};
use resonance_content::{battle_scene::Scene, prepared::Files};
use std::sync::Arc;

pub const CATALOGUE: u16 = 250;
pub const HOLY_LANCE: u16 = 115;
const LIFETIME: u16 = 210;
const RING_RADIUS: f32 = 325.;
const RING_HEIGHT: f32 = 300.;
const AIM_HEIGHT: f32 = 80.;

pub(super) fn prepare(
    files: &Files,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let mut required = bank.clone();
    required.members = vec![1, 2, 3];
    resources.effect(required)?;
    let recoil = recoil::Parameters::load(files)?;
    let mut definitions = Vec::new();
    for index in [1, 2] {
        let source = scene
            .projectiles
            .get(index)
            .context("missing lance projectile")?;
        let rule = source.hit()?;
        let mut hit = super::hit::authored(rule, &recoil)?;
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
            &[(0, bank), (2, bank), (6, bank)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("lance projectile has no contact")?
            .cooldown = rule.cooldown;
        definitions.push(projectile);
    }
    let at = |tick| TRANSITION_TICKS + ACTIVE_START_TICKS + tick;
    let appearance = |member| EffectAppearance {
        resource: bank.resource,
        member,
    };
    let effect = |tick, member, offset| SpellEffect {
        at: at(tick),
        appearance: appearance(member),
        placement: resonance_battle::SpellEffectPlacement::Scene(offset),
        deferred: true,
        captured_heading_offset: None,
    };
    let mut effects = Vec::new();
    let mut projectiles = Vec::new();
    for (marker, launch, degrees) in [
        (0, 35, 45_f32),
        (10, 45, 225.),
        (20, 55, 135.),
        (30, 65, 315.),
    ] {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let offset = [cos * RING_RADIUS, 0., sin * RING_RADIUS];
        effects.push(effect(marker, 2, offset));
        let raised = [offset[0], RING_HEIGHT, offset[2]];
        effects.push(effect(launch, 3, raised));
        projectiles.push(SpellProjectile {
            at: at(launch) + 1,
            definition: Arc::clone(&definitions[0]),
            origin_offset: raised,
            launch: resonance_battle::SpellLaunch::AtHeight(AIM_HEIGHT),
        });
    }
    effects.push(effect(85, 3, [0., 350., 0.]));
    projectiles.push(SpellProjectile {
        at: at(85) + 1,
        definition: Arc::clone(&definitions[1]),
        origin_offset: [0., 400., 0.],
        launch: resonance_battle::SpellLaunch::AtHeight(AIM_HEIGHT),
    });
    effects.push(effect(95, 3, [0., 120., 0.]));
    effects.sort_by_key(|effect| effect.at);
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        projectiles,
        effects,
        recovery: vec![],
        origin: SpellOrigin::SceneGround { toward_caster: 1. },
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, LIFETIME)?,
        camera: stored_spells::scene_camera(scene.camera, LIFETIME)?,
        duration: TRANSITION_TICKS + LIFETIME + SCENE_BOUNDARY_TICKS,
        startup: Some((
            TRANSITION_TICKS + 1,
            SpellStartup::DeferredEffect(appearance(1)),
        )),
        sound: None,
    })
}
