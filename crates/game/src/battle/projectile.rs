//! Resolve a projectile and its effect banks in one preparation step.
use super::{ActionResources, EffectResource};
use anyhow::{Context, Result};
use resonance_battle::{
    EffectAppearance, ProjectileContact, ProjectileDefinition, ProjectileEffects,
};
use resonance_content::{battle_projectile, prepared::Files};
use std::sync::Arc;

impl ActionResources {
    /// Bank keys come from the projectile catalogue; values belong to this encounter.
    pub fn projectile(
        &mut self,
        files: &Files,
        row: &battle_projectile::Projectile,
        mut hit: resonance_battle::HitRule,
        banks: &[(u8, &EffectResource)],
    ) -> Result<Arc<ProjectileDefinition>> {
        if let Some(reason) = &row.unsupported_reason {
            anyhow::bail!("{reason}");
        }
        let select = |bank, member| {
            if member == 0 {
                return None;
            }
            let binding = banks
                .iter()
                .find_map(|&(key, binding)| (key == bank).then_some(binding))
                .filter(|binding| binding.bank.is_some())?;
            let mut binding = binding.clone();
            binding.members = vec![u16::from(member)];
            Some(binding)
        };
        let contact = &row.contact;
        hit.overlimit_pause = false;
        let mut effect = |source: battle_projectile::Effect| -> Result<Option<EffectAppearance>> {
            let Some(binding) = select(source.bank, source.member) else {
                return Ok(None);
            };
            let appearance = EffectAppearance {
                resource: binding.resource,
                member: u16::from(source.member),
            };
            self.effect(binding)?;
            Ok(Some(appearance))
        };
        let trail = effect(row.trail_effect)?;
        let definition = ProjectileDefinition {
            lifetime: row.lifetime,
            velocity: row.velocity,
            acceleration: row.acceleration,
            offset: row.spawn_offset,
            clamp_ground: row.clamp_ground,
            active: row.active,
            birth: effect(row.birth_effect)?,
            motion: row.motion.clone(),
            effects: ProjectileEffects {
                trail: trail
                    .map(|effect| -> Result<_> {
                        Ok((
                            effect,
                            row.trail_interval
                                .context("missing projectile trail interval")?,
                        ))
                    })
                    .transpose()?,
                ground: effect(row.ground_effect)?,
                shadow: row.shadow,
                clash: if contact.clashes {
                    files
                        .diagnostics()
                        .attempt(
                            "projectile clash artwork",
                            effect(battle_projectile::Effect {
                                bank: 0,
                                member: 11,
                            }),
                        )?
                        .flatten()
                } else {
                    None
                },
            },
            contact: Some(ProjectileContact {
                hit,
                cooldown: 12,
                repeat_limit: contact.repeat_limit,
                radius: contact.radius,
                height: contact.height,
                shape: contact.shape,
                offset: contact.offset,
                radius_growth: contact.growth[0],
                height_growth: contact.growth[1],
                survives_contact: contact.survives_contact,
                clashes: contact.clashes,
            }),
        };
        definition.validate()?;
        Ok(Arc::new(definition))
    }
}
