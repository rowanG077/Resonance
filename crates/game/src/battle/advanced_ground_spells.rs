//! Water and earth scenes retain one point while their contacts run independently.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, CONTACT_INITIALIZATION_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result, bail};
use resonance_battle::{PreparedSpell, SpellOrigin, SpellProjectile, SpellStartup};
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
    let mut projectiles = Vec::with_capacity(pulses.len());
    for &(at, member, height) in pulses {
        let source = scene
            .projectiles
            .get(member)
            .context("missing ground scene projectile")?;
        let rule = source.hit()?;
        let mut projectile = resources.scene_projectile(
            files,
            &source.projectile,
            rule,
            &recoil,
            bank,
            &[(0, bank), (6, bank)],
        )?;
        Arc::make_mut(&mut projectile).offset[1] += height;
        projectiles.push(SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + at + CONTACT_INITIALIZATION_TICKS,
            projectile,
        ));
    }
    let startup = resources.effect_member(bank, 1)?;
    Ok(PreparedSpell {
        projectiles,
        startup: Some((
            TRANSITION_TICKS + 1,
            if matches!(catalogue, ATLAS | TIDAL_WAVE | DREADED_WAVE) {
                SpellStartup::DeferredEffect(startup)
            } else {
                SpellStartup::Effect(startup)
            },
        )),
        ..stored_spells::scene_spell(scene, origin, lifetime)?
    })
}
