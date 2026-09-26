//! Shared submitted contacts: fn_1_3D864, fn_1_3D6FC and fn_1_3BDF8.
use crate::{
    ActionId, Actor, ActorId, ContactSource, Cue, EffectAppearance, HitRule, HitShape,
    MeleeDefinition, Side, damage, geometry, projectile::Projectile,
};
use anyhow::{Result, ensure};

#[cfg(test)]
mod tests;

pub(crate) struct Contact {
    pub source: ContactSource,
    pub owner: ActorId,
    pub position: [f32; 3],
    radius: f32,
    height: f32,
    shape: HitShape,
    rule: HitRule,
    cooldown: u8,
    clash: Option<EffectAppearance>,
    used: bool,
}

#[derive(Default)]
pub(crate) struct Contacts(pub [Vec<Contact>; 2]);

impl Contacts {
    pub fn weapon(
        &mut self,
        owner: ActorId,
        flight: &crate::weapon_flight::Flight,
        side: Side,
    ) -> Result<()> {
        let definition = &flight.definition;
        self.push(
            side,
            Contact {
                source: ContactSource::Weapon {
                    actor: owner,
                    slot: definition.slot,
                    action: flight.action,
                },
                owner,
                position: flight.position,
                radius: definition.radius,
                height: definition.height,
                shape: definition.shape,
                rule: definition.hit,
                cooldown: definition.cooldown,
                clash: None,
                used: false,
            },
        )
    }

    pub fn submit(&mut self, projectile: &Projectile, side: Side) -> Result<()> {
        let Some(definition) = &projectile.definition.contact else {
            return Ok(());
        };
        if !projectile.frame.contact_active {
            return Ok(());
        }
        self.push(
            side,
            Contact {
                source: ContactSource::Projectile(projectile.frame.id),
                owner: projectile.frame.owner,
                position: std::array::from_fn(|i| {
                    projectile.frame.position[i] + definition.offset[i]
                }),
                radius: projectile.radius,
                height: projectile.height,
                shape: definition.shape,
                rule: definition.hit,
                cooldown: definition.cooldown,
                clash: definition.clash_effect,
                used: false,
            },
        )
    }

    pub fn melee(
        &mut self,
        actor: ActorId,
        action: ActionId,
        body: &Actor,
        model: Option<&crate::model::Model>,
        definition: &MeleeDefinition,
    ) -> Result<()> {
        for &anchor in &definition.anchors {
            // 2D564 does not also submit a hand hit for a detached weapon.
            if model.is_some_and(|model| !model.anchor_attached(anchor)) {
                continue;
            }
            self.push(
                body.side,
                Contact {
                    source: ContactSource::Melee { actor, action },
                    owner: actor,
                    position: body.body.anchors[usize::from(anchor)],
                    radius: definition.radius,
                    height: definition.height,
                    shape: definition.shape,
                    rule: definition.hit,
                    cooldown: definition.cooldown,
                    clash: None,
                    used: false,
                },
            )?;
        }
        Ok(())
    }

    fn push(&mut self, side: Side, contact: Contact) -> Result<()> {
        let list = &mut self.0[usize::from(side == Side::Enemy)];
        if list.len() == 40 {
            return Ok(());
        }
        ensure!(
            contact.position.iter().all(|v| v.is_finite()),
            "contact position overflow"
        );
        list.push(contact);
        Ok(())
    }

    pub fn resolve(&mut self, battle: &mut crate::Battle, cues: &mut Vec<Cue>) -> Result<()> {
        let show_numbers = battle.phase() == crate::BattlePhase::Combat;
        // One actor latch across both sides; side and submission order are preserved.
        let mut received = [false; 12];
        for side in 0..2 {
            for index in 0..self.0[side].len() {
                if self.clash(side, index, battle, cues)? {
                    continue;
                }
                let contact = &self.0[side][index];
                let owner = &battle.actors[contact.owner.index()];
                let projectile = match contact.source {
                    ContactSource::Projectile(id) => Some(&battle.projectiles[&id]),
                    ContactSource::Melee { .. } | ContactSource::Weapon { .. } => None,
                };
                let mut hit = None;
                for (actor_index, actor) in battle.actors.iter().enumerate() {
                    let id = ActorId(actor_index as u8);
                    let can_hit = match contact.source {
                        ContactSource::Projectile(projectile) => {
                            battle.projectiles[&projectile].can_hit(id)
                        }
                        ContactSource::Melee { actor, .. } => {
                            battle.melee[actor.index()].can_hit(id)
                        }
                        ContactSource::Weapon { actor, slot, .. } => {
                            battle.weapon_flights[&(actor, slot)].can_hit(id)
                        }
                    };
                    if actor.side == owner.side
                        || !actor.available()
                        || received[actor_index]
                        || !can_hit
                    {
                        continue;
                    }
                    for (point_index, &point) in actor.body.points.iter().enumerate() {
                        if geometry::overlaps(
                            contact.shape,
                            [contact.radius, contact.height],
                            contact.position,
                            owner.body.scale,
                            point,
                            actor.body.scale,
                            actor.position[1],
                        )? {
                            hit = Some((actor_index, point_index));
                            break;
                        }
                    }
                    if hit.is_some() {
                        break;
                    }
                }
                if let Some((actor_index, point_index)) = hit {
                    received[actor_index] = true;
                    if battle.actors[actor_index].side == Side::Party {
                        battle.ledger.party_was_hit = true;
                    }
                    // 3C3A4: feedback starts at the hurt bone's surface in the
                    // planar direction of the submitted contact.
                    let target = &battle.actors[actor_index];
                    let was_casting = matches!(target.activity, crate::Activity::Casting { .. })
                        && !target.hud.cast_released;
                    let point = target.body.points[point_index];
                    let normal =
                        crate::distance::planar_direction(contact.position, point.center, [0.; 3]);
                    let radius = point.radius * target.body.scale;
                    let impact_position =
                        std::array::from_fn(|i| point.center[i] + normal[i] * radius);
                    let impact_heading = match contact.source {
                        ContactSource::Projectile(_) | ContactSource::Weapon { .. } => {
                            target.heading
                        }
                        ContactSource::Melee { .. } => owner.heading + 180.,
                    };
                    let power = projectile.map_or(owner.attack_power, |p| p.attack_power);
                    // 3BE68/3BE7C: attached and detached weapon contacts use
                    // the current owner18E4 cache, independently of heading.
                    let incoming = projectile.map_or(owner.facing_direction, |p| p.velocity);
                    let direction = contact.rule.reaction.direction(
                        owner.position,
                        battle.actors[actor_index].position,
                        contact.position,
                        incoming,
                    );
                    let [owner, target] = battle
                        .actors
                        .get_disjoint_mut([contact.owner.index(), actor_index])
                        .unwrap();
                    let result = damage::resolve(
                        owner,
                        target,
                        contact.rule,
                        power,
                        incoming,
                        &mut battle.random,
                    );
                    let previous_combo = target.reaction.combo_hits;
                    let reaction =
                        crate::reaction::respond(target, contact.rule.reaction, result, direction);
                    let combo_hits = target.reaction.combo_hits;
                    if owner.side == Side::Party
                        && target.reaction.combo_hits > i32::from(battle.ledger.maximum_combo)
                    {
                        battle.ledger.maximum_combo = target.reaction.combo_hits as u16;
                    }
                    target.hud.contact(result.guard);
                    if show_numbers {
                        target.show_hit_number(result);
                    }
                    // Ordinary contacts do not start local hit-stop. The original
                    // 3CDA8..3CE04 pause branch requires an Over Limit target.
                    // 3C58C/3C848: guard/avoid/absorb/immune results skip TP gain.
                    // Armor and reduced down contacts still qualify; lethal damage
                    // replaces the original result mask with an ordinary hit.
                    if !contact.rule.arte
                        && (target.hp == 0
                            || (result.guard == crate::GuardResult::None
                                && result.protection != crate::HitProtection::Avoided
                                && !matches!(
                                    result.affinity,
                                    crate::Affinity::Absorb | crate::Affinity::Immune
                                )))
                    {
                        owner.tp = owner.tp.saturating_add(1).min(owner.max_tp);
                    }
                    if show_numbers && combo_hits != previous_combo {
                        // 3C904..3C954 / B564: display the leading non-auto
                        // party member's own hits received and selected target.
                        // Availability does not change the leading member.
                        let leader = battle
                            .actors
                            .iter()
                            .position(|actor| {
                                actor.side == Side::Party && actor.control != crate::Control::Auto
                            })
                            .or_else(|| {
                                battle
                                    .actors
                                    .iter()
                                    .position(|actor| actor.side == Side::Party)
                            });
                        if combo_hits > 1
                            && leader.is_some_and(|leader| {
                                leader == actor_index
                                    || battle.target(ActorId(leader as u8))
                                        == Some(ActorId(actor_index as u8))
                            })
                        {
                            cues.push(Cue::Combo {
                                actor: ActorId(actor_index as u8),
                                hits: combo_hits,
                                damage: battle.actors[actor_index].reaction.combo_damage,
                            });
                        }
                        battle.record_combo_titles(combo_hits);
                    }
                    // 3BDF8 sets owner+199C for an accepted geometric contact,
                    // including guard/absorb; automatic chains read it later.
                    battle.confirm_actor_contact(contact.owner);
                    let actor = ActorId(actor_index as u8);
                    cues.push(Cue::Hit {
                        source: contact.source,
                        actor,
                        hurt_point: point_index as u8,
                        result,
                    });
                    if let Some(reaction) = reaction {
                        battle.interrupt_actor(actor, cues);
                        if matches!(reaction, crate::reaction::ContactReaction::Hurt { .. }) {
                            battle.clear_actor_particles(actor, cues);
                        }
                        if let Some(model) = &mut battle.models[actor_index] {
                            match reaction {
                                crate::reaction::ContactReaction::Hurt { alternate } => {
                                    model.hurt(alternate)?
                                }
                                crate::reaction::ContactReaction::Guard => {
                                    let actor = &battle.actors[actor_index];
                                    model
                                        .guard(actor.position[1] > 0.1 && !actor.movement.flying)?;
                                }
                            }
                        }
                    }
                    let target = &mut battle.actors[actor_index];
                    if target.hp > 0 && crate::knockdown::threshold_reached(target) {
                        if target.reaction.profile.can_knock_down {
                            crate::knockdown::enter(target);
                            battle.interrupt_actor(actor, cues);
                        }
                    } else if crate::stun::roll(
                        &battle.actors[contact.owner.index()],
                        &battle.actors[actor_index],
                        contact.rule.reaction.stun_chance,
                        result,
                        &mut battle.random,
                    ) {
                        battle.enter_stun(actor)?;
                    } else if battle.actors[actor_index].hp == 0 {
                        battle.enter_death(actor, cues)?;
                        let action = match contact.source {
                            ContactSource::Projectile(id) => battle.projectiles[&id].action,
                            ContactSource::Melee { action, .. }
                            | ContactSource::Weapon { action, .. } => action,
                        };
                        battle.death_contact_feedback(actor, action, cues)?;
                    }
                    // 3D528 calls 1FB2C after 28DF4 has cleared the victim's
                    // combo. Only a party owner updates the party killer slot.
                    if battle.actors[actor_index].hp == 0
                        && battle.actors[contact.owner.index()].side == Side::Party
                    {
                        battle.ledger.kill(
                            contact.owner,
                            battle.actors[actor_index].reaction.combo_hits,
                        );
                    }
                    // 3D624 / 3B370 dispatch the authored effect after reaction,
                    // stun and defeat processing, before contact sound/voice.
                    if let Some(impact) = contact.rule.impact
                        && (impact.on_guard || result.guard == crate::GuardResult::None)
                    {
                        let action = match contact.source {
                            ContactSource::Projectile(id) => battle.projectiles[&id].action,
                            ContactSource::Melee { action, .. }
                            | ContactSource::Weapon { action, .. } => action,
                        };
                        battle.show_effect(
                            crate::effect::Spawn {
                                action,
                                scene: None,
                                owner: contact.owner,
                                target: actor,
                                appearance: impact.appearance,
                                origin: impact_position,
                                heading: impact_heading,
                                follow: None,
                                scale: 1.,
                                late: false,
                                tint: Default::default(),
                            },
                            cues,
                        )?;
                    }
                    let action = match contact.source {
                        ContactSource::Projectile(id) => battle.projectiles[&id].action,
                        ContactSource::Melee { action, .. }
                        | ContactSource::Weapon { action, .. } => action,
                    };
                    battle.common_contact_feedback(
                        action,
                        contact.owner,
                        actor,
                        impact_position,
                        impact_heading,
                        contact
                            .rule
                            .element
                            .resolve(&battle.actors[contact.owner.index()]),
                        result,
                        cues,
                    )?;
                    battle.contact_audio(
                        contact.owner,
                        actor,
                        was_casting,
                        contact.rule,
                        contact
                            .rule
                            .element
                            .resolve(&battle.actors[contact.owner.index()]),
                        result,
                        cues,
                    );
                    match contact.source {
                        ContactSource::Projectile(id) => {
                            battle.projectiles.get_mut(&id).unwrap().hit(actor)
                        }
                        ContactSource::Melee { .. } => {
                            battle.melee[contact.owner.index()].hit(actor, contact.cooldown)
                        }
                        ContactSource::Weapon {
                            actor: owner, slot, ..
                        } => battle
                            .weapon_flights
                            .get_mut(&(owner, slot))
                            .unwrap()
                            .hit(actor),
                    }
                }
            }
        }
        Ok(())
    }

    fn clash(
        &mut self,
        side: usize,
        index: usize,
        battle: &mut crate::Battle,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let contact = &self.0[side][index];
        let Some(effect) = contact.clash else {
            return Ok(false);
        };
        if contact.used {
            return Ok(false);
        }
        let Some(other) = self.0[side ^ 1].iter().find(|other| {
            !other.used
                && crate::distance::length(std::array::from_fn(|i| {
                    contact.position[i] - other.position[i]
                })) < contact.radius + other.radius
        }) else {
            return Ok(false);
        };
        // Only projectile entries initiate clashes; melee entries remain available.
        let ContactSource::Projectile(id) = contact.source else {
            unreachable!()
        };
        let projectile = &battle.projectiles[&id];
        let position = std::array::from_fn(|i| {
            other.position[i] + (contact.position[i] - other.position[i]) * 0.5
        });
        cues.push(Cue::ProjectileClashed {
            projectile: id,
            other: other.source,
            position,
        });
        battle.show_effect(
            crate::effect::Spawn {
                scene: None,
                action: projectile.action,
                owner: projectile.frame.owner,
                target: projectile.frame.owner,
                appearance: effect,
                origin: position,
                heading: 0.,
                follow: None,
                scale: 1.,
                late: false,
                tint: Default::default(),
            },
            cues,
        )?;
        battle.projectiles.get_mut(&id).unwrap().clash();
        self.0[side][index].used = true;
        Ok(true)
    }
}
