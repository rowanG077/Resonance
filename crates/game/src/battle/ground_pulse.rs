//! One authored contact volume shares the spell's retained point.
use super::{
    ActionResources, EffectResource, recoil,
    stored_spells::{self, ACTIVE_START_TICKS, CONTACT_INITIALIZATION_TICKS, TRANSITION_TICKS},
};
use anyhow::{Context, Result, bail};
use resonance_battle::{PreparedSpell, SpellOrigin, SpellStartup};
use resonance_content::{battle_scene::Scene, prepared::Files};

pub const SPREAD: u16 = 63;
pub const AIR_THRUST: u16 = 71;
pub const CYCLONE: u16 = 72;
pub const AIR_BLADE: u16 = 73;
pub const GROUND_DASHER: u16 = 76;
pub const RAGING_MIST: u16 = 85;
pub const GRAVITY_WELL: u16 = 90;
pub const KNIGHT_AIR_THRUST: u16 = 216;

pub(super) fn prepare(
    files: &Files,
    catalogue: u16,
    scene: &Scene,
    bank: &EffectResource,
    resources: &mut ActionResources,
) -> Result<PreparedSpell> {
    let (lifetime, pulse_tick) = match catalogue {
        SPREAD => (170, 45),
        GRAVITY_WELL => (225, 45),
        AIR_THRUST | KNIGHT_AIR_THRUST => (150, 30),
        CYCLONE => (220, 50),
        AIR_BLADE => (160, 30),
        GROUND_DASHER => (190, 40),
        RAGING_MIST => (220, 20),
        _ => bail!("unknown ground pulse spell {catalogue}"),
    };
    let source = scene
        .projectiles
        .get(1)
        .context("missing ground pulse contact")?;
    let rule = source.hit()?;
    let projectile = resources.scene_projectile(
        files,
        &source.projectile,
        rule,
        &recoil::Parameters::load(files)?,
        bank,
        &[(0, bank), (6, bank)],
    )?;
    let startup = resources.effect_member(bank, 1)?;
    Ok(PreparedSpell {
        projectiles: vec![resonance_battle::SpellProjectile::new(
            TRANSITION_TICKS + ACTIVE_START_TICKS + pulse_tick + CONTACT_INITIALIZATION_TICKS,
            projectile,
        )],
        startup: Some((
            TRANSITION_TICKS + 1,
            if matches!(
                catalogue,
                SPREAD | GROUND_DASHER | RAGING_MIST | GRAVITY_WELL
            ) {
                SpellStartup::DeferredEffect(startup)
            } else {
                SpellStartup::Effect(startup)
            },
        )),
        ..stored_spells::scene_spell(
            scene,
            match catalogue {
                AIR_THRUST | KNIGHT_AIR_THRUST => SpellOrigin::SceneTarget {
                    minimum_height: 176.,
                },
                AIR_BLADE => SpellOrigin::SceneCasterAhead {
                    distance: 80.,
                    height: 100.,
                },
                CYCLONE | GROUND_DASHER | RAGING_MIST => {
                    SpellOrigin::SceneGround { toward_caster: 1. }
                }
                _ => SpellOrigin::GroundAtRelease { toward_caster: 1. },
            },
            lifetime,
        )?
    })
}
