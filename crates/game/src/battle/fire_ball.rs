//! Resolve the Fire Ball release and its projectile resources.
use super::EffectResource;
use anyhow::{Context, Result};
use resonance_content::{battle_projectile, prepared::Files};

pub const CATALOGUE: u16 = 66;

pub fn prepare(
    files: &Files,
    projectiles: &battle_projectile::Table,
    bank: &EffectResource,
    resources: &mut super::ActionResources,
    sound: Option<resonance_battle::Sound>,
) -> Result<resonance_battle::PreparedVolley> {
    let row = projectiles
        .records
        .get(3)
        .context("missing Fire Ball projectile artwork")?;
    let hit = resonance_battle::HitRule {
        kind: resonance_battle::DamageKind::Magic,
        arte: true,
        element: resonance_battle::HitElement::Element(resonance_battle::Element::Fire),
        ..super::hit::physical(75)
    };
    let projectile = resources.projectile(files, row, hit, &[(1, bank)])?;
    let startup = resonance_battle::EffectAppearance {
        resource: bank.resource,
        member: 23,
    };
    let mut effects = bank.clone();
    effects.members = vec![startup.member];
    super::require_effect(&mut resources.effects, effects)?;
    Ok(resonance_battle::PreparedVolley {
        projectile,
        shots: 3,
        interval: 8,
        startup: Some(startup),
        sound,
    })
}
