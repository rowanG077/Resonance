//! Repeated hammers choose their contact variant independently at each release.
use crate::{ActionId, ActorId, Battle, Element, HitElement, ProjectileDefinition, Random};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct HammerProjectile {
    /// Plain, Ice and Toss Hammer, with their own contact and appearance.
    pub choices: [Arc<ProjectileDefinition>; 3],
    /// Hammer Rain uses Pow Pow Hammer's experience.
    pub uses_catalogue: u16,
    pub ring: Option<HammerRing>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(any(test, feature = "test-support"), derive(serde::Deserialize))]
#[cfg_attr(any(test, feature = "test-support"), serde(deny_unknown_fields))]
pub struct HammerRing {
    pub count: u8,
    pub angle_step: f32,
    pub speed: f32,
    pub speed_jitter: f32,
    pub lift: f32,
    pub lift_jitter: f32,
    pub jitter_range: u16,
}

impl HammerProjectile {
    pub(super) fn validate(&self) -> Result<()> {
        for projectile in &self.choices {
            projectile.validate()?;
        }
        if let Some(ring) = self.ring {
            ensure!(
                ring.count > 0
                    && ring.jitter_range > 0
                    && [
                        ring.angle_step,
                        ring.speed,
                        ring.speed_jitter,
                        ring.lift,
                        ring.lift_jitter
                    ]
                    .iter()
                    .all(|value| value.is_finite())
                    && self.choices.iter().all(|projectile| {
                        projectile.motion.speed.is_none()
                            && projectile.motion.velocity_jitter == [0.; 3]
                    }),
                "invalid hammer ring"
            );
        }
        Ok(())
    }

    pub(super) fn emit(
        &self,
        battle: &mut Battle,
        action: ActionId,
        owner: ActorId,
        target: ActorId,
    ) -> Result<()> {
        let uses = battle
            .technique_uses(owner, self.uses_catalogue)
            .unwrap_or(0);
        let actor = battle.actor(owner)?;
        let position = actor.position;
        let luck = actor.equipment.luck;
        let element = crate::damage::resolve_element(HitElement::Inherited, actor);
        for index in 0..self.ring.map_or(1, |ring| ring.count) {
            let choice = choose(uses, luck, element, &mut battle.random);
            let mut projectile = Arc::clone(&self.choices[choice]);
            if let Some(ring) = self.ring {
                #[cfg(any(test, feature = "test-support"))]
                let observed = battle
                    .random_inputs
                    .as_mut()
                    .map(|inputs| {
                        inputs.hammer_ring(
                            crate::random_inputs::Key {
                                clock: battle.ledger.elapsed_ticks,
                                owner: owner.index(),
                                target: target.index(),
                                action: action.0,
                                contact: crate::random_inputs::Contact::Projectile(
                                    battle.next_projectile,
                                ),
                            },
                            index,
                            ring,
                        )
                    })
                    .transpose()?;
                #[cfg(any(test, feature = "test-support"))]
                let values =
                    observed.unwrap_or_else(|| std::array::from_fn(|_| battle.random.next_u16()));
                #[cfg(not(any(test, feature = "test-support")))]
                let values = std::array::from_fn(|_| battle.random.next_u16());
                Arc::make_mut(&mut projectile).velocity = ring.velocity(index, values);
            }
            battle.emit(projectile, action, owner, target, position)?;
        }
        Ok(())
    }
}

fn choose(uses: u16, luck: u16, element: Option<Element>, random: &mut Random) -> usize {
    const EXPERIENCED_USES: u16 = 200;
    if uses < EXPERIENCED_USES {
        0
    } else if random.next_u16() % 100 < luck / 16 {
        2
    } else {
        usize::from(element == Some(Element::Ice) && random.next_u16() & 3 == 0)
    }
}

impl HammerRing {
    fn velocity(self, index: u8, values: [u16; 3]) -> [f32; 3] {
        let angle = self.angle_step * f32::from(index);
        let [x, z, lift] =
            values.map(|value| (i32::from(value as i16) % i32::from(self.jitter_range)) as f32);
        // Start on the actor's right; the shared projectile transform rotates
        // this local ring and its spawn offset together.
        let x = (self.speed + self.speed_jitter * x) * angle.cos();
        let z = (self.speed + self.speed_jitter * z) * angle.sin();
        let y = self.lift + self.lift_jitter * lift;
        [x, y, z]
    }
}
