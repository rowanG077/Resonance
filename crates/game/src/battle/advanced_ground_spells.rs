//! Water and earth scenes retain one point while their contacts run independently.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, SCENE_BOUNDARY_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result, bail};
use resonance_battle::{
    EffectAppearance, PreparedSpell, SpellOrigin, SpellProjectile, SpellStartup,
};
use resonance_content::{battle_scene::Scene, prepared::Files};
use std::sync::Arc;

pub const TIDAL_WAVE: u16 = 64;
pub const DREADED_WAVE: u16 = 86;
pub const ATLAS: u16 = 91;
pub const EARTH_BITE: u16 = 93;
pub const SCENES: [(u16, u16); 4] = [
    (TIDAL_WAVE, 202),
    (DREADED_WAVE, 224),
    (ATLAS, 229),
    (EARTH_BITE, 231),
];

pub(super) fn prepare(
    files: &Files,
    catalogue: u16,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    // Pulse callback age, projectile member, and height relative to the retained point.
    let (lifetime, origin, pulses): (_, _, &[(u16, usize, f32)]) = match catalogue {
        TIDAL_WAVE => (
            210,
            SpellOrigin::SceneFixed { position: [0.; 3] },
            &[(25, 1, 0.)],
        ),
        DREADED_WAVE => (
            180,
            SpellOrigin::SceneGround { toward_caster: 1. },
            &[(35, 1, 0.)],
        ),
        ATLAS => (
            170,
            SpellOrigin::SceneTargetPlane {
                height: 192.,
                toward_caster: 0.,
                body_center: true,
            },
            &[(30, 1, 0.)],
        ),
        EARTH_BITE => (
            265,
            SpellOrigin::SceneTargetPlane {
                height: 125.,
                toward_caster: 1.,
                body_center: false,
            },
            &[(30, 1, 0.), (100, 2, -125.), (110, 2, -125.)],
        ),
        _ => bail!("unknown ground scene spell {catalogue}"),
    };
    let recoil = recoil::Parameters::load(files)?;
    let mut effects = bank.clone();
    effects.members = vec![1];
    // Contacts initialize on the update after allocation.
    const INITIALIZATION_TICKS: u16 = 1;
    let mut projectiles = Vec::with_capacity(pulses.len());
    for &(at, member, height) in pulses {
        let source = scene
            .projectiles
            .get(member)
            .context("missing ground scene projectile")?;
        let rule = source.hit()?;
        let mut hit = super::hit::authored(rule, &recoil)?;
        if let Some(member) = rule.effect {
            effects.members.push(member);
            hit.effect = Some(EffectAppearance {
                resource: bank.resource,
                member,
            });
        }
        let mut projectile =
            resources.projectile(files, &source.projectile, hit, &[(0, bank), (6, bank)])?;
        let definition = Arc::make_mut(&mut projectile);
        definition.offset[1] += height;
        definition
            .contact
            .as_mut()
            .context("ground scene projectile has no contact")?
            .cooldown = rule.cooldown;
        projectiles.push(SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + at + INITIALIZATION_TICKS,
            projectile,
        ));
    }
    resources.effect(effects)?;
    let startup = EffectAppearance {
        resource: bank.resource,
        member: 1,
    };
    Ok(PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        projectiles,
        recovery: vec![],
        origin,
        transition_ticks: TRANSITION_TICKS,
        stage_tint: stored_spells::stage_tint(scene.stage_color, lifetime)?,
        camera: stored_spells::scene_camera(scene.camera, lifetime)?,
        duration: TRANSITION_TICKS + lifetime + SCENE_BOUNDARY_TICKS,
        startup: Some((
            TRANSITION_TICKS + 1,
            if matches!(catalogue, ATLAS | TIDAL_WAVE | DREADED_WAVE) {
                SpellStartup::DeferredEffect(startup)
            } else {
                SpellStartup::Effect(startup)
            },
        )),
        sound: None,
    })
}
