//! Relative projectile placement is resolved from the actor at release.
use crate::{ActionId, ActorId, Battle, ProjectileDefinition};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ProjectileShot {
    pub definition: Arc<ProjectileDefinition>,
    /// Actor-local offset of the emission origin, before the template's offset.
    pub offset: [f32; 3],
    pub heading_offset: f32,
    pub scatter: Option<ProjectileScatter>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProjectileScatter {
    /// Independent signed grid positions along world X and Z.
    pub position_range: u16,
    pub position_step: f32,
    /// Signed actor-local X velocity added to the projectile template.
    pub lateral_range: u16,
    pub lateral_step: f32,
}

impl ProjectileShot {
    pub(super) fn validate(&self) -> Result<()> {
        self.definition.validate()?;
        ensure!(
            self.offset.iter().all(|value| value.is_finite()) && self.heading_offset.is_finite(),
            "invalid projectile placement"
        );
        if let Some(scatter) = self.scatter {
            ensure!(
                scatter.position_range > 0
                    && scatter.lateral_range > 0
                    && scatter.position_step.is_finite()
                    && scatter.lateral_step.is_finite()
                    && self.definition.motion.velocity_jitter == [0.; 3],
                "invalid projectile scatter"
            );
        }
        Ok(())
    }

    pub(super) fn emit(
        &self,
        battle: &mut Battle,
        action: ActionId,
        actor: ActorId,
        target: ActorId,
    ) -> Result<()> {
        let owner = battle.actor(actor)?;
        let heading = owner.heading + self.heading_offset;
        let offset = crate::geometry::rotate(self.offset, owner.heading);
        let mut position = std::array::from_fn(|axis| owner.position[axis] + offset[axis]);
        let mut definition = Arc::clone(&self.definition);
        if let Some(scatter) = self.scatter {
            #[cfg(any(test, feature = "test-support"))]
            let observed = battle
                .random_inputs
                .as_mut()
                .map(|inputs| {
                    inputs.scatter(
                        crate::random_inputs::Key {
                            clock: battle.ledger.elapsed_ticks,
                            owner: actor.index(),
                            target: target.index(),
                            action: action.0,
                            contact: crate::random_inputs::Contact::Projectile(
                                battle.next_projectile,
                            ),
                        },
                        scatter,
                    )
                })
                .transpose()?;
            #[cfg(any(test, feature = "test-support"))]
            let [x, z, lateral] =
                observed.unwrap_or_else(|| std::array::from_fn(|_| battle.random.next_u16()));
            #[cfg(not(any(test, feature = "test-support")))]
            let [x, z, lateral] = std::array::from_fn(|_| battle.random.next_u16());
            let sample =
                |value: u16, range: u16| (i32::from(value as i16) % i32::from(range)) as f32;
            position[0] += scatter.position_step * sample(x, scatter.position_range);
            position[2] += scatter.position_step * sample(z, scatter.position_range);
            Arc::make_mut(&mut definition).velocity[0] +=
                scatter.lateral_step * sample(lateral, scatter.lateral_range);
        }
        battle.emit_at(definition, action, actor, target, position, heading)?;
        Ok(())
    }
}
