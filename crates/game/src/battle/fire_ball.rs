//! Resolve the Fire Ball release and its projectile resources.
use super::EffectResource;
use anyhow::{Context, Result};
use resonance_content::{battle_projectile, prepared::Files};

pub const CATALOGUE: u16 = 66;
pub const KNIGHT_CATALOGUE: u16 = 213;

pub fn prepare(
    files: &Files,
    projectiles: &battle_projectile::Table,
    bank: &EffectResource,
    resources: &mut super::ActionResources,
    sound: Option<resonance_battle::Sound>,
) -> Result<resonance_battle::PreparedSpell> {
    let row = projectiles
        .records
        .get(3)
        .context("missing Fire Ball projectile artwork")?;
    let launch = projectiles
        .fire_ball
        .as_ref()
        .context("missing Fire Ball launch parameters")?;
    let rule = super::resident_spells::rule(projectiles, 204)?;
    let recoil = super::recoil::Parameters::load(files)?;
    let hit = super::hit::authored(&rule.hit, &recoil)?;
    let mut projectile = resources.projectile(files, row, hit, &[(1, bank)])?;
    if let Some(contact) = std::sync::Arc::make_mut(&mut projectile).contact.as_mut() {
        contact.cooldown = rule.hit.cooldown;
    }
    let startup = resonance_battle::EffectAppearance {
        resource: bank.resource,
        member: 23,
    };
    let mut effects = bank.clone();
    effects.members = vec![startup.member];
    super::require_effect(&mut resources.effects, effects)?;
    Ok(resonance_battle::PreparedSpell {
        policy: Default::default(),
        summon: None,
        effects: vec![],
        recovery: vec![],
        projectiles: [4, 12, 20]
            .into_iter()
            .zip(launch.velocities)
            .map(|(at, velocity)| {
                let mut definition = std::sync::Arc::clone(&projectile);
                std::sync::Arc::make_mut(&mut definition).velocity = velocity;
                let mut shot = resonance_battle::SpellProjectile::new(at, definition);
                shot.launch = resonance_battle::SpellLaunch::CasterAbove {
                    center_height_scale: launch.center_height_scale,
                    height_offset: launch.height_offset,
                };
                shot
            })
            .collect(),
        origin: resonance_battle::SpellOrigin::Caster,
        transition_ticks: 0,
        stage_tint: None,
        camera: None,
        duration: 20,
        startup: Some((0, resonance_battle::SpellStartup::Effect(startup))),
        sound,
    })
}
