use crate::{
    ActionId, Actor, ActorId, ContactSource, Cue, HitRule, HitShape, MeleeDefinition, Side, damage,
    geometry, projectile::Projectile,
};
use anyhow::{Result, ensure};

#[cfg(test)]
#[path = "contact/alone_tests.rs"]
mod alone_tests;
#[cfg(test)]
#[path = "contact/lethal_rescue_tests.rs"]
mod lethal_rescue_tests;
#[cfg(test)]
mod tests;

pub(crate) struct Contact {
    pub source: ContactSource,
    pub owner: ActorId,
    origin: Origin,
    radius: f32,
    height: f32,
    shape: HitShape,
    rule: HitRule,
    clashes: bool,
    /// Melee eligibility captured when submitted, even if the action is interrupted later.
    struck: Vec<ActorId>,
}

enum Origin {
    World([f32; 3]),
    /// Resolved from the owner after movement and body separation.
    Melee([f32; 3]),
}

#[derive(Default)]
pub(crate) struct Contacts(pub Vec<Contact>);

impl Contacts {
    pub fn weapon(&mut self, owner: ActorId, flight: &crate::weapon_flight::Flight) -> Result<()> {
        let definition = &flight.definition;
        self.push(Contact {
            source: ContactSource::Weapon {
                actor: owner,
                slot: definition.slot,
                action: flight.action,
            },
            owner,
            origin: Origin::World(flight.position),
            radius: definition.radius,
            height: 0.,
            shape: HitShape::Sphere,
            rule: definition.hit,
            clashes: false,
            struck: Vec::new(),
        })
    }

    pub fn submit(&mut self, projectile: &Projectile) -> Result<()> {
        let Some(definition) = &projectile.definition.contact else {
            return Ok(());
        };
        if !projectile.frame.contact_active {
            return Ok(());
        }
        self.push(Contact {
            source: ContactSource::Projectile(projectile.frame.id),
            owner: projectile.frame.owner,
            origin: Origin::World(std::array::from_fn(|i| {
                projectile.frame.position[i] + definition.offset[i]
            })),
            radius: projectile.radius,
            height: projectile.height,
            shape: definition.shape,
            rule: definition.hit,
            clashes: definition.clashes,
            struck: Vec::new(),
        })
    }

    pub fn melee(
        &mut self,
        actor: ActorId,
        action: ActionId,
        definition: &MeleeDefinition,
        struck: &[ActorId],
    ) -> Result<()> {
        self.push(Contact {
            source: ContactSource::Melee { actor, action },
            owner: actor,
            origin: Origin::Melee(definition.volume.offset),
            radius: definition.volume.radius,
            height: definition.volume.half_height,
            shape: HitShape::Cylinder,
            rule: definition.hit,
            clashes: false,
            struck: struck.to_vec(),
        })
    }

    fn push(&mut self, contact: Contact) -> Result<()> {
        let (Origin::World(point) | Origin::Melee(point)) = contact.origin;
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "contact position overflow"
        );
        self.0.push(contact);
        Ok(())
    }

    pub fn resolve(&mut self, battle: &mut crate::Battle, cues: &mut Vec<Cue>) -> Result<()> {
        self.resolve_clashes(battle, cues)?;
        // Each attack owns repeat-hit admission; simultaneous attacks all resolve.
        for contact in self.0.drain(..) {
            for actor in 0..battle.actors.len() {
                if !contact.hits(battle, actor)? {
                    continue;
                }
                contact.apply(battle, actor, cues)?;
                if !matches!(contact.source, ContactSource::Melee { .. }) {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Clash-capable projectiles cancel opposing projectiles within their radii.
    /// A melee or thrown weapon remains armed.
    /// Resolve every clash before searching for actor hits, without side priority.
    fn resolve_clashes(&self, battle: &mut crate::Battle, cues: &mut Vec<Cue>) -> Result<()> {
        for contact in &self.0 {
            let ContactSource::Projectile(id) = contact.source else {
                continue;
            };
            if !contact.clashes || !contact.armed(battle) {
                continue;
            }
            let origin = contact.position(&battle.actors);
            let Some(other) = self.0.iter().find(|other| {
                let other_position = other.position(&battle.actors);
                battle.actors[other.owner.index()].side != battle.actors[contact.owner.index()].side
                    && other.armed(battle)
                    && crate::distance::length(std::array::from_fn(|i| {
                        origin[i] - other_position[i]
                    })) < contact.radius + other.radius
            }) else {
                continue;
            };
            let other_position = other.position(&battle.actors);
            let position = std::array::from_fn(|i| (origin[i] + other_position[i]) * 0.5);
            let projectile = battle.projectiles.get_mut(&id).unwrap();
            let effect = projectile.definition.effects.clash;
            projectile.clash();
            if let ContactSource::Projectile(other) = other.source {
                battle.projectiles.get_mut(&other).unwrap().clash();
            }
            cues.push(Cue::ProjectileClashed {
                projectile: id,
                other: other.source,
                position,
            });
            let Some(effect) = effect else { continue };
            cues.push(Cue::Effect(crate::EffectRequest {
                owner: contact.owner,
                target: contact.owner,
                appearance: effect,
                origin: position,
                heading: 0.,
                follow: None,
                scale: 1.,

                tint: Default::default(),
            }));
        }
        Ok(())
    }
}

impl Contact {
    fn position(&self, actors: &[Actor]) -> [f32; 3] {
        match self.origin {
            Origin::World(point) => point,
            Origin::Melee(offset) => actors[self.owner.index()].local_point(offset),
        }
    }

    fn armed(&self, battle: &crate::Battle) -> bool {
        match self.source {
            ContactSource::Projectile(id) => !battle.projectiles[&id].frame.disarmed,
            ContactSource::Melee { .. } | ContactSource::Weapon { .. } => true,
        }
    }

    fn hits(&self, battle: &crate::Battle, index: usize) -> Result<bool> {
        if !self.armed(battle) {
            return Ok(false);
        }
        let owner = &battle.actors[self.owner.index()];
        let actor = &battle.actors[index];
        if actor.side == owner.side || !actor.available() {
            return Ok(false);
        }
        let id = ActorId(index as u8);
        let can_hit = match self.source {
            ContactSource::Projectile(projectile) => battle.projectiles[&projectile].can_hit(id),
            ContactSource::Melee { .. } => !self.struck.contains(&id),
            ContactSource::Weapon { actor, slot, .. } => {
                battle.weapon_flights[&(actor, slot)].can_hit(id)
            }
        };
        let position = self.position(&battle.actors);
        let Some(point) = actor.hurt_point(position[1]).filter(|_| can_hit) else {
            return Ok(false);
        };
        geometry::overlaps(
            self.shape,
            [self.radius, self.height],
            position,
            owner.body.scale,
            point,
            actor.body.scale,
            actor.position[1],
        )
    }

    fn apply(
        &self,
        battle: &mut crate::Battle,
        actor_index: usize,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let combat = battle.phase() == crate::BattlePhase::Combat;
        let position = self.position(&battle.actors);
        let actor = ActorId(actor_index as u8);
        let owner = &battle.actors[self.owner.index()];
        let projectile = match self.source {
            ContactSource::Projectile(id) => Some(&battle.projectiles[&id]),
            ContactSource::Melee { .. } | ContactSource::Weapon { .. } => None,
        };
        let element = damage::resolve_element(self.rule.element, owner);
        if battle.actors[actor_index].side == Side::Party {
            battle.ledger.party_was_hit = true;
        }
        // Start feedback at the collider surface in the planar contact direction.
        let target = &battle.actors[actor_index];
        // Capture state before damage and condition changes.
        let rescue_conditions = target.conditions.effective();
        let activity = battle.activity(actor);
        let was_casting = matches!(activity, crate::Activity::Casting { .. });
        let point = target.hurt_point(position[1]).unwrap();
        let normal = crate::distance::planar_direction(position, point.center, [0.; 3]);
        let radius = point.radius * target.body.scale;
        let impact_position = std::array::from_fn(|i| point.center[i] + normal[i] * radius);
        let power = match self.source {
            ContactSource::Melee { .. } => owner.attack_power,
            ContactSource::Projectile(id) => battle.projectiles[&id].attack_power,
            ContactSource::Weapon { actor, slot, .. } => {
                battle.weapon_flights[&(actor, slot)].attack_power
            }
        };
        let incoming = projectile.map_or(owner.facing_direction, |p| p.velocity);
        let direction = self.rule.reaction.direction(
            owner.position,
            battle.actors[actor_index].position,
            position,
            incoming,
        );
        let suppressed = battle.suppress_recoil(
            self.owner,
            actor,
            self.rule.reaction.recoil.suppression_distance,
        );
        let mut available_counts = [0u8; 2];
        for actor in &battle.actors {
            if actor.available() {
                available_counts[actor.side as usize] += 1;
            }
        }
        let [owner, target] = battle
            .actors
            .get_disjoint_mut([self.owner.index(), actor_index])
            .unwrap();
        let (result, condition_label) = damage::resolve_with_condition(
            owner,
            target,
            activity,
            self.rule,
            power,
            incoming,
            &mut |_| battle.random.next_u16(),
            battle.items.all_divide,
            available_counts,
        );
        let condition_position =
            std::array::from_fn(|i| target.body.center_offset[i] * 2. + target.position[i]);
        if let Some(kind) = condition_label {
            cues.push(Cue::ConditionLabel {
                actor,
                kind,
                position: condition_position,
            });
        }
        if combat && result.is_damage() {
            // Retain half of nominal damage for recovery. Absorption,
            // avoidance, immunity, and contacts after combat retain the previous value.
            let nominal = result.amount.max(0);
            battle.runtime[actor_index].recovery.last_hit_damage = nominal;
            battle.runtime[actor_index].recovery.last_hit_recovery = nominal >> 1;
        }
        battle.contact_retaliation(self.owner, actor, result, cues)?;
        let [owner, target] = battle
            .actors
            .get_disjoint_mut([self.owner.index(), actor_index])
            .unwrap();
        if !self.rule.arte && result.is_unblocked_damage() {
            owner.tp = owner.tp.saturating_add(1).min(owner.equipment.max_tp);
        }
        let previous_combo = target.reaction.combo_hits;
        let reaction = crate::reaction::respond(
            owner,
            target,
            self.rule.reaction,
            result,
            direction,
            suppressed,
        );
        let combo_hits = target.reaction.combo_hits;
        if owner.side == Side::Party
            && target.reaction.combo_hits > i32::from(battle.ledger.maximum_combo)
        {
            battle.ledger.maximum_combo =
                target.reaction.combo_hits.min(i32::from(u16::MAX)) as u16;
        }
        if combat && combo_hits != previous_combo && combo_hits > 1 {
            cues.push(Cue::Combo {
                actor,
                hits: combo_hits,
                damage: battle.actors[actor_index].reaction.combo_damage,
            });
        }
        if combo_hits != previous_combo {
            battle
                .ledger
                .combo_contact(battle.actors[actor_index].side, combo_hits);
        }
        // Accepted geometric contacts enable automatic chains, including guarded and
        // absorbed hits.
        battle.confirm_actor_contact(self.owner);
        if let Some(reaction) = reaction {
            match reaction {
                crate::reaction::ContactReaction::Hurt { remaining, .. } => {
                    battle.begin_hurt(actor, remaining, cues)
                }
                crate::reaction::ContactReaction::Guard => {
                    battle.interrupt_actor(actor, cues);
                }
            }
            if let crate::reaction::ContactReaction::Hurt { alternate, .. } = reaction {
                battle
                    .model_requests
                    .push(crate::ModelRequest::Hurt { actor, alternate });
            }
        }
        // Unblocked party damage gains Unison, including armored and lethal hits.
        if result.is_unblocked_damage() && battle.actors[self.owner.index()].side == Side::Party {
            battle.add_unison_gauge(16, cues);
        }
        battle.pause_overlimit_contact(actor, self.rule.overlimit_pause);
        let target = &mut battle.actors[actor_index];
        if crate::knockdown::threshold_reached(target) {
            if target.reaction.profile.can_knock_down {
                target.reaction.stagger.received = 0;
                battle.enter_knockdown(actor, cues);
            }
        } else {
            let stunned = crate::stun::roll(
                &battle.actors[self.owner.index()],
                &battle.actors[actor_index],
                self.rule.reaction.stun_chance,
                result,
                &mut battle.random,
            );
            if stunned {
                battle.enter_stun(actor, cues);
            }
        }
        cues.push(Cue::Hit {
            source: self.source,
            owner: self.owner,
            actor,
            position: impact_position,
            element,
            was_casting,
            overlimit: battle.actors[actor_index].overlimit.is_active(),
            stunned: battle.activity(actor) == crate::Activity::Stunned,
            result,
        });
        if battle.actors[actor_index].hp == 0
            && !battle.try_lethal_rescue(actor, rescue_conditions, cues)?
        {
            // Only a confirmed defeat earns kill credit and recovery.
            battle.enter_death(actor, cues);
            if battle.actors[self.owner.index()].side == Side::Party {
                battle.ledger.kill(self.owner);
            }
            if battle.actors[self.owner.index()].available() {
                battle.recover_after_kill(self.owner, cues);
            }
            battle.retarget_after_defeat(actor)?;
        }
        if result.is_damage() {
            battle.contact_overlimit(actor, result.is_unblocked_damage());
        }

        match self.source {
            ContactSource::Projectile(id) => battle.projectiles.get_mut(&id).unwrap().hit(actor),
            ContactSource::Melee {
                actor: owner,
                action,
            } => {
                if let Some((id, sequence)) = battle.runtime[owner.index()].action_mut()
                    && *id == action
                    && let Some(window) = &mut sequence.melee
                {
                    window.struck.push(actor);
                }
            }
            ContactSource::Weapon {
                actor: owner, slot, ..
            } => battle
                .weapon_flights
                .get_mut(&(owner, slot))
                .unwrap()
                .hit(actor),
        }
        Ok(())
    }
}
