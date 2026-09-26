//! Detached equipped weapons (2D564/21E94). Each slot retains its hit rule and
//! cooldowns independently of the action that launched it.
use crate::{ActionId, ActorId, HitRule, HitShape};
use anyhow::{Context, Result, ensure};
use glam::{EulerRot, Mat4, Quat, Vec3};
use resonance_content::animation::Matrix;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct WeaponFlightDefinition {
    pub slot: u8,
    pub outbound_ticks: u16,
    pub speed: f32,
    pub return_speed: f32,
    pub direction_y: f32,
    pub hit: HitRule,
    pub cooldown: u8,
    pub radius: f32,
    pub height: f32,
    pub shape: HitShape,
}

impl WeaponFlightDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        self.hit.reaction.validate()?;
        ensure!(
            self.slot < 2
                && self.outbound_ticks <= i16::MAX as u16
                && self.speed.is_finite()
                && self.speed > 0.
                && self.return_speed.is_finite()
                && self.return_speed > 0.
                && self.direction_y.is_finite()
                && self.radius.is_finite()
                && self.radius >= 0.
                && self.height.is_finite()
                && self.height >= 0.
                && match self.shape {
                    HitShape::Ring { width } => width.is_finite(),
                    _ => true,
                },
            "invalid detached weapon flight"
        );
        Ok(())
    }
}

pub(crate) struct Launch {
    pub task: i32,
    pub definition: Arc<WeaponFlightDefinition>,
    pub start: i16,
}

/// A proved callback operand for this visit only. The hand displacement
/// replaces it whenever the original direction helper actually writes one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ReturnSteering {
    KeepDirection,
    Desired([f32; 3]),
}

impl ReturnSteering {
    /// The caller proves a retained-death callback, a fresh body pair and an
    /// Auto nonleader. Reduced trig and direct tiny cosine keep its direction;
    /// direct non-tiny arguments still have an unproved callback operand.
    pub(crate) fn retained_death(heading: f32) -> Option<Self> {
        if !heading.is_finite() || heading.abs() >= 1000. {
            return None;
        }
        let radians = (180. + heading) * f32::from_bits(0x3c8e_fa33);
        let high = ((f64::from(radians).to_bits() >> 32) as u32) & 0x7fff_ffff;
        (!(0x3e40_0000..=0x3fe9_21fb).contains(&high)).then_some(Self::KeepDirection)
    }

    pub(crate) fn facing(angle: f32, wrapped: bool) -> Self {
        let desired = [
            f32::from_bits(f64::from(angle).to_bits() as u32),
            angle,
            if wrapped { angle } else { 1. },
        ];
        // The source constructor preserves the incoming paired lane as one;
        // a single-precision angle wrap replaces it with the desired angle.
        // Overflowing length is unordered and takes the unchanged branch.
        if crate::distance::length(desired) > 0.1 {
            Self::Desired(desired)
        } else {
            Self::KeepDirection
        }
    }
}

pub(crate) struct Flight {
    pub definition: Arc<WeaponFlightDefinition>,
    pub action: ActionId,
    pub position: [f32; 3],
    direction: [f32; 3],
    angles: [f32; 3],
    remaining: u16,
    speed: f32,
    pub caught: bool,
    cooldowns: [u8; 12],
}

impl Flight {
    fn new(
        definition: Arc<WeaponFlightDefinition>,
        action: ActionId,
        position: [f32; 3],
        heading: f32,
        facing_direction: [f32; 3],
    ) -> Self {
        // 2D614..2D658 copies cached owner18E4, replaces Y, then calls the
        // SDK normalizer. Heading can differ from this cache after a turn.
        let [x, _, z] = facing_direction;
        Self {
            direction: crate::distance::normalize([x, definition.direction_y, z]),
            angles: [-60., heading + 15., 0.],
            remaining: definition.outbound_ticks,
            speed: definition.speed,
            definition,
            action,
            position,
            caught: false,
            cooldowns: [0; 12],
        }
    }

    /// The caller supplies the preceding callback's proved catch operand.
    /// Return visits replace it with the current sampled hand.
    fn step(
        &mut self,
        hand: [f32; 3],
        outbound_point: Option<[f32; 3]>,
        tiny_return: Option<ReturnSteering>,
    ) -> Result<()> {
        let returning = self.remaining == 0;
        let point = if returning {
            [hand[0] + 0.1, hand[1], hand[2] + 0.1]
        } else {
            outbound_point
                .context("detached weapon catch operand is unproved for this actor callback")?
        };
        let return_motion = if returning {
            let delta = std::array::from_fn(|i| point[i] - self.position[i]);
            let distance = crate::distance::length(delta);
            let steering = if distance >= 0.1 {
                // 4DAF4 writes a normalized hand displacement. 4D920 will
                // normalize that result again below.
                ReturnSteering::Desired(crate::distance::normalize(delta))
            } else {
                ensure!(
                    distance < 0.1,
                    "detached weapon return distance is unordered"
                );
                tiny_return.context("detached weapon return steering below 0.1 is unproved")?
            };
            Some((steering, distance))
        } else {
            None
        };
        for value in &mut self.cooldowns {
            *value = value.saturating_sub(1);
        }
        if let Some((steering, distance)) = return_motion {
            let blend = if distance <= 200. {
                0.001_f32.mul_add(200. - distance, 0.35)
            } else {
                0.25
            };
            // 4D920's strict comparison includes the unordered/no-change
            // branch. A tiny hand gap uses the callback operand directly,
            // with just this one normalization before the blend.
            if let ReturnSteering::Desired(desired) = steering
                && crate::distance::length(desired) > 0.1
            {
                let desired = crate::distance::normalize(desired);
                self.direction = crate::distance::normalize(std::array::from_fn(|i| {
                    self.direction[i] * (1. - blend) + desired[i] * blend
                }));
            }
            self.speed = 0.1_f32.mul_add(self.definition.return_speed, self.speed);
            if self.speed.abs() > self.definition.return_speed {
                self.speed = self.definition.return_speed;
            }
        } else {
            self.remaining -= 1;
        }
        for (position, direction) in self.position.iter_mut().zip(self.direction) {
            *position += direction * self.speed;
        }
        if self.position[1] <= 5.1 {
            self.position[1] = 5.1;
        }
        let degrees = f32::from_bits(0x4265_2ee4); // Original 57.29579162597656.
        // 22090 calls the SDK acos wrapper, then rounds before the fused scale.
        self.angles[0] = degrees.mul_add(f64::from(self.direction[1]).acos() as f32, 180.);
        self.angles[1] = degrees * self.direction[0].atan2(self.direction[2]);
        self.caught = crate::distance::length(std::array::from_fn(|i| point[i] - self.position[i]))
            <= self.speed.abs();
        ensure!(
            self.position
                .iter()
                .chain(&self.angles)
                .all(|v| v.is_finite()),
            "detached weapon transform overflow"
        );
        Ok(())
    }

    fn world(&self) -> Matrix {
        let [x, y, z] = self.angles.map(f32::to_radians);
        Mat4::from_rotation_translation(
            Quat::from_euler(EulerRot::ZYX, z, y, x),
            Vec3::from_array(self.position),
        )
        .to_cols_array_2d()
    }

    pub fn can_hit(&self, target: ActorId) -> bool {
        self.cooldowns[target.index()] == 0
    }
    pub fn hit(&mut self, target: ActorId) {
        self.cooldowns[target.index()] = self.definition.cooldown;
    }
}

/// 24314/244D0 under the ordinary martial/death frame chain leave the high
/// word of the widened source-rounded radians at the catch point's Z operand.
/// The other two words are tiny pointer values with proved origin equivalence.
pub(crate) fn sampled_heading_point(heading: f32) -> [f32; 3] {
    let radians = (180. + heading) * f32::from_bits(0x3c8e_fa33);
    [
        0.,
        0.,
        f32::from_bits((f64::from(radians).to_bits() >> 32) as u32),
    ]
}

/// 24D24's non-tiny input is retained by the martial hit-stop command chain.
/// Opening normal admission excludes the early branch that skips this copy.
pub(crate) fn facing_point(direction: [f32; 3]) -> Option<[f32; 3]> {
    (crate::distance::length(direction) >= 0.01).then_some([0., 0., direction[0]])
}

impl crate::Battle {
    pub(crate) fn retire_weapon_flight(&mut self, owner: ActorId, slot: u8) {
        self.weapon_flights.remove(&(owner, slot));
        self.trail_timers[owner.index()][usize::from(slot)] = 0;
        if let Some(model) = &mut self.models[owner.index()] {
            model.attach_weapon(slot, &mut self.actors[owner.index()]);
        }
    }

    pub(crate) fn retire_weapon_flights(&mut self) {
        self.weapon_flights.clear();
        for (model, actor) in self.models.iter_mut().zip(&mut self.actors) {
            if let Some(model) = model {
                model.attach_weapons(actor);
            }
        }
    }

    pub(crate) fn throw_weapon(
        &mut self,
        owner: ActorId,
        action: ActionId,
        definition: Arc<WeaponFlightDefinition>,
    ) -> Result<()> {
        let key = (owner, definition.slot);
        // A consumed launch row never replaces an already detached slot.
        if self
            .weapon_flights
            .get(&key)
            .is_some_and(|flight| !flight.caught)
        {
            return Ok(());
        }
        let hand = self.models[owner.index()]
            .as_ref()
            .context("weapon flight requires actor model")?
            .weapon_attachment(definition.slot)?;
        let actor = &self.actors[owner.index()];
        let flight = Flight::new(
            definition,
            action,
            hand,
            actor.heading,
            actor.facing_direction,
        );
        self.weapon_flights.insert(key, flight);
        Ok(())
    }

    pub(crate) fn advance_weapon_flights(
        &mut self,
        owner: ActorId,
        outbound_point: Option<[f32; 3]>,
        tiny_return: Option<ReturnSteering>,
        contacts: &mut crate::contact::Contacts,
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
                let model = self.models[owner.index()]
                    .as_mut()
                    .context("weapon flight requires actor model")?;
                flight.step(model.weapon_attachment(slot)?, outbound_point, tiny_return)?;
                self.trail_timers[owner.index()][usize::from(slot)] = 30;
                model.detach_weapon(
                    slot,
                    (!flight.caught).then(|| flight.world()),
                    &mut self.actors[owner.index()],
                )?;
                contacts.weapon(owner, flight, self.actors[owner.index()].side)
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
