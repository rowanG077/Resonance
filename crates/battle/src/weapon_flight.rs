//! Each thrown weapon travels out, returns to its owner, and hits each target once.
use crate::{ActionId, ActorId, HitRule};
use anyhow::{Result, ensure};
use glam::Vec3;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Debug, Clone)]
pub struct WeaponFlightDefinition {
    pub slot: u8,
    /// Launch and return point in actor-local units, independent of animation.
    pub origin: [f32; 3],
    pub outbound_ticks: u32,
    pub speed: f32,
    pub return_speed: f32,
    pub direction_y: f32,
    pub hit: HitRule,
    pub radius: f32,
}

impl WeaponFlightDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        self.hit.reaction.validate()?;
        ensure!(
            self.slot < 2
                && self.origin.iter().all(|value| value.is_finite())
                && self.speed.is_finite()
                && self.speed > 0.
                && self.return_speed.is_finite()
                && self.return_speed > 0.
                && self.direction_y.is_finite()
                && self.radius.is_finite()
                && self.radius > 0.,
            "invalid detached weapon flight"
        );
        Ok(())
    }
}

pub(crate) struct Flight {
    pub definition: Arc<WeaponFlightDefinition>,
    pub action: ActionId,
    pub position: [f32; 3],
    pub attack_power: u16,
    pub direction: [f32; 3],
    remaining: u32,
    pub caught: bool,
    struck: BTreeSet<ActorId>,
}

impl Flight {
    fn new(
        definition: Arc<WeaponFlightDefinition>,
        action: ActionId,
        actor: &crate::Actor,
    ) -> Self {
        let [x, _, z] = actor.facing_direction;
        Self {
            direction: crate::distance::normalize([x, definition.direction_y, z]),
            remaining: definition.outbound_ticks,
            position: actor.local_point(definition.origin),
            definition,
            action,
            attack_power: actor.attack_power,
            caught: false,
            struck: BTreeSet::new(),
        }
    }

    fn step(&mut self, anchor: [f32; 3]) -> Result<()> {
        ensure!(
            anchor.iter().all(|value| value.is_finite()),
            "invalid weapon return point"
        );
        let speed = if self.remaining == 0 {
            let target = Vec3::from_array(anchor);
            let position = Vec3::from_array(self.position);
            let offset = target - position;
            let distance = offset.length();
            if distance <= self.definition.return_speed {
                self.position = anchor;
                self.caught = true;
                return Ok(());
            }
            self.direction = (offset / distance).to_array();
            self.definition.return_speed
        } else {
            self.remaining -= 1;
            self.definition.speed
        };
        for (position, direction) in self.position.iter_mut().zip(self.direction) {
            *position += direction * speed;
        }
        self.position[1] = self.position[1].max(0.);
        ensure!(
            self.position.iter().all(|value| value.is_finite()),
            "detached weapon transform overflow"
        );
        Ok(())
    }
    pub fn can_hit(&self, target: ActorId) -> bool {
        !self.struck.contains(&target)
    }
    pub fn hit(&mut self, target: ActorId) {
        self.struck.insert(target);
    }
}

impl crate::Battle {
    pub(crate) fn retire_weapon_flight(&mut self, owner: ActorId, slot: u8) {
        self.weapon_flights.remove(&(owner, slot));
    }

    pub(crate) fn retire_weapon_flights(&mut self) {
        self.weapon_flights.clear();
    }

    pub(crate) fn throw_weapon(
        &mut self,
        owner: ActorId,
        action: ActionId,
        definition: Arc<WeaponFlightDefinition>,
    ) {
        let key = (owner, definition.slot);
        // A consumed launch row never replaces an already detached slot.
        if self
            .weapon_flights
            .get(&key)
            .is_some_and(|flight| !flight.caught)
        {
            return;
        }
        let flight = Flight::new(definition, action, &self.actors[owner.index()]);
        self.weapon_flights.insert(key, flight);
    }

    pub(crate) fn advance_weapon_flights(
        &mut self,
        owner: ActorId,
        contacts: &mut crate::contact::Contacts,
        cues: &mut Vec<crate::Cue>,
    ) -> Result<()> {
        for slot in 0..2 {
            let key = (owner, slot);
            if self
                .weapon_flights
                .get(&key)
                .is_some_and(|flight| flight.caught)
            {
                self.weapon_flights.remove(&key);
            }
            if !self.weapon_flights.contains_key(&key) {
                continue;
            }
            let result = (|| {
                let flight = self.weapon_flights.get_mut(&key).unwrap();
                flight.step(self.actors[owner.index()].local_point(flight.definition.origin))?;
                if !flight.caught {
                    cues.push(crate::Cue::WeaponTrail {
                        actor: owner,
                        slot,
                        duration: 1,
                    });
                }
                contacts.weapon(owner, flight)
            })();
            if let Err(error) = result {
                self.diagnostics.report(
                    &format!("battle actor {} weapon {slot}", owner.index()),
                    error,
                )?;
                self.diagnostic = true;
                self.retire_weapon_flight(owner, slot);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

/// Native flight placement; the scene supplies any available weapon artwork.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponFlightFrame {
    pub owner: ActorId,
    pub slot: u8,
    pub position: [f32; 3],
    pub direction: [f32; 3],
}
