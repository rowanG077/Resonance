use crate::{
    ActionId, ActorId, ContactSource, Cue, HitRule, HitShape, MeleeDefinition, Side, damage,
    geometry, projectile::Projectile,
};
use anyhow::{Context, Result, ensure};

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

#[derive(Clone, Copy)]
enum Origin {
    World([f32; 3]),
    /// Resolved from the owner after movement and body separation.
    Melee([f32; 3]),
    Bone(crate::MeleeAnchor),
    FrozenBone([f32; 3], crate::MeleeAnchor),
    BodyCenter,
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
        for origin in projectile.contact_origins(definition.offset) {
            self.push(Contact {
                source: ContactSource::Projectile(projectile.frame.id),
                owner: projectile.frame.owner,
                origin: Origin::World(origin),
                radius: projectile.radius,
                height: projectile.height,
                shape: definition.shape,
                rule: definition.hit,
                clashes: definition.clashes,
                struck: Vec::new(),
            })?;
        }
        Ok(())
    }

    pub fn melee(
        &mut self,
        actor: ActorId,
        action: ActionId,
        definition: &MeleeDefinition,
        struck: &[ActorId],
    ) -> Result<()> {
        self.melee_at(actor, action, definition, struck, None)
    }

    pub(crate) fn melee_at(
        &mut self,
        actor: ActorId,
        action: ActionId,
        definition: &MeleeDefinition,
        struck: &[ActorId],
        positions: Option<&[[f32; 3]]>,
    ) -> Result<()> {
        let (origins, radius, height, shape) = match &definition.volume {
            crate::MeleeVolume::Fixed {
                offset,
                radius,
                half_height,
            } => (
                vec![Origin::Melee(*offset)],
                *radius,
                *half_height,
                HitShape::Cylinder,
            ),
            crate::MeleeVolume::BodyCenter {
                radius,
                half_height,
                shape,
                ..
            } => (vec![Origin::BodyCenter], *radius, *half_height, *shape),
            crate::MeleeVolume::Attached {
                anchors,
                radius,
                half_height,
                shape,
                ..
            } => (
                anchors.iter().copied().map(Origin::Bone).collect(),
                *radius,
                *half_height,
                *shape,
            ),
        };
        let origins = if let Some(points) = positions {
            points
                .iter()
                .enumerate()
                .map(|(index, &point)| match origins.get(index) {
                    Some(Origin::Bone(anchor)) => Origin::FrozenBone(point, *anchor),
                    _ => Origin::World(point),
                })
                .collect()
        } else {
            origins
        };
        for origin in origins {
            self.push(Contact {
                source: ContactSource::Melee { actor, action },
                owner: actor,
                origin,
                radius,
                height,
                shape,
                rule: definition.hit,
                clashes: false,
                struck: struck.to_vec(),
            })?;
        }
        Ok(())
    }

    fn push(&mut self, contact: Contact) -> Result<()> {
        ensure!(
            !contact.rule.reaction.capture
                || matches!(contact.origin, Origin::Bone(_) | Origin::FrozenBone(_, _)),
            "capture requires a melee body attachment"
        );
        if let Origin::World(point) | Origin::Melee(point) | Origin::FrozenBone(point, _) =
            contact.origin
        {
            ensure!(
                point.iter().all(|v| v.is_finite()),
                "contact position overflow"
            );
        }
        self.0.push(contact);
        Ok(())
    }

    pub fn resolve(
        &mut self,
        battle: &mut crate::Battle,
        cues: &mut Vec<Cue>,
        placements: &[Option<resonance_content::animation::Matrix>],
    ) -> Result<()> {
        self.resolve_clashes(battle, cues)?;
        // Each attack owns repeat-hit admission; simultaneous attacks all resolve.
        let mut accepted = Vec::new();
        for mut contact in self.0.drain(..) {
            if let ContactSource::Melee { actor, action } = contact.source
                && !matches!(contact.origin, Origin::World(_) | Origin::FrozenBone(_, _))
            {
                let position = contact.position(battle)?;
                if let Some((id, sequence)) = battle.runtime[actor.index()].action_mut()
                    && *id == action
                    && let Some(window) = &mut sequence.melee
                {
                    window.positions.push(position);
                }
                contact.origin = match contact.origin {
                    Origin::Bone(anchor) => Origin::FrozenBone(position, anchor),
                    _ => Origin::World(position),
                };
            }
            for actor in 0..battle.actors.len() {
                if accepted.contains(&(contact.source, actor)) {
                    continue;
                }
                let Some(point) =
                    contact.hit_point(battle, actor, placements.get(actor).copied().flatten())?
                else {
                    continue;
                };
                contact.apply(battle, actor, point, cues)?;
                accepted.push((contact.source, actor));
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
            let origin = contact.position(battle)?;
            let mut found = None;
            for other in &self.0 {
                let other_position = other.position(battle)?;
                if battle.actors[other.owner.index()].side
                    != battle.actors[contact.owner.index()].side
                    && other.armed(battle)
                    && crate::distance::length(std::array::from_fn(|i| {
                        origin[i] - other_position[i]
                    })) < contact.radius + other.radius
                {
                    found = Some(other);
                    break;
                }
            }
            let Some(other) = found else { continue };
            let other_position = other.position(battle)?;
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
    fn position(&self, battle: &crate::Battle) -> Result<[f32; 3]> {
        let point = match self.origin {
            Origin::World(point) | Origin::FrozenBone(point, _) => point,
            Origin::Melee(offset) => battle.actors[self.owner.index()].local_point(offset),
            Origin::BodyCenter => battle.actors[self.owner.index()].effect_origin(),
            Origin::Bone(anchor) => battle
                .models
                .as_ref()
                .context("missing melee playback")?
                .anchor(self.owner, anchor)?,
        };
        ensure!(
            point.iter().all(|v| v.is_finite()),
            "contact position overflow"
        );
        Ok(point)
    }

    fn armed(&self, battle: &crate::Battle) -> bool {
        match self.source {
            ContactSource::Projectile(id) => !battle.projectiles[&id].frame.disarmed,
            ContactSource::Melee { .. } | ContactSource::Weapon { .. } => true,
        }
    }

    fn hit_point(
        &self,
        battle: &crate::Battle,
        index: usize,
        placement: Option<resonance_content::animation::Matrix>,
    ) -> Result<Option<geometry::HurtPoint>> {
        if !self.armed(battle) {
            return Ok(None);
        }
        let owner = &battle.actors[self.owner.index()];
        let actor = &battle.actors[index];
        if actor.side == owner.side
            || !actor.available()
            || !battle.unison_contact_allowed(self.owner, ActorId(index as u8))
        {
            return Ok(None);
        }
        let id = ActorId(index as u8);
        let can_hit = match self.source {
            ContactSource::Projectile(projectile) => battle.projectiles[&projectile].can_hit(id),
            ContactSource::Melee { .. } => !self.struck.contains(&id),
            ContactSource::Weapon { actor, slot, .. } => {
                battle.weapon_flights[&(actor, slot)].can_hit(id)
            }
        };
        if !can_hit {
            return Ok(None);
        }
        let position = self.position(battle)?;
        for point in battle.hurt_points(id, position[1], placement) {
            if geometry::overlaps(
                self.shape,
                [self.radius, self.height],
                position,
                owner.body.scale,
                point,
                actor.body.scale,
                actor.position[1],
            )? {
                return Ok(Some(point));
            }
        }
        Ok(None)
    }

    fn apply(
        &self,
        battle: &mut crate::Battle,
        actor_index: usize,
        point: geometry::HurtPoint,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let combat = battle.phase() == crate::BattlePhase::Combat;
        let position = self.position(battle)?;
        let actor = ActorId(actor_index as u8);
        let owner = &battle.actors[self.owner.index()];
        let projectile = match self.source {
            ContactSource::Projectile(id) => Some(&battle.projectiles[&id]),
            ContactSource::Melee { .. } | ContactSource::Weapon { .. } => None,
        };
        let feedback = projectile.and_then(|projectile| projectile.definition.effects.contact);
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
        let unison_defeat_pending = battle.unison_defeat_pending(actor);
        let source_action = match self.source {
            ContactSource::Projectile(id) => battle.projectiles[&id].action,
            ContactSource::Melee { action, .. } | ContactSource::Weapon { action, .. } => action,
        };
        let captured = battle.is_captured(actor);
        let mode = if captured {
            damage::ContactMode::Captured
        } else {
            battle.unison_contact_mode(self.owner, source_action)
        };
        let [owner, target] = battle
            .actors
            .get_disjoint_mut([self.owner.index(), actor_index])
            .unwrap();
        let (result, condition_label) = damage::resolve_contact(
            owner,
            target,
            activity,
            self.rule,
            power,
            incoming,
            &mut |_| battle.random.next_u16(),
            battle.items.all_divide,
            available_counts,
            mode,
        );
        if let crate::Power::AdjustAfterContact(adjustment) = self.rule.power {
            owner.attack_power = u16::try_from(
                (i32::from(owner.attack_power) + i32::from(adjustment))
                    .max(i32::from(damage::MIN_ATTACK_POWER)),
            )
            .context("contact power adjustment exceeds the actor power range")?;
        }
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
        let effective_hit = result.amount > 0
            && !matches!(
                result.affinity,
                crate::Affinity::Absorb | crate::Affinity::Immune
            )
            && result.protection == crate::HitProtection::None
            && !result.reaction_suppressed
            && result.guard == crate::GuardResult::None;
        if self.rule.hp_drain && effective_hit {
            let owner = &mut battle.actors[self.owner.index()];
            let before = owner.hp;
            owner.recover_flat_hp(result.amount);
            cues.push(Cue::Recovered {
                actor: self.owner,
                kind: crate::RecoveryKind::Hp,
                nominal: result.amount,
                applied: owner.hp - before,
            });
        }
        let captured = battle.is_captured(actor);
        let [owner, target] = battle
            .actors
            .get_disjoint_mut([self.owner.index(), actor_index])
            .unwrap();
        if !self.rule.arte && result.is_unblocked_damage() && !result.reaction_suppressed {
            owner.tp = owner.tp.saturating_add(1).min(owner.equipment.max_tp);
        }
        let previous_combo = target.reaction.combo_hits;
        let held_motion = captured.then_some((target.movement, target.reaction.recoil));
        let reaction = crate::reaction::respond(
            owner,
            target,
            self.rule.reaction,
            result,
            direction,
            suppressed,
        );
        let reaction = if let Some((movement, recoil)) = held_motion {
            if !result.suppresses_reaction() && target.time_stop == 0 {
                battle.runtime[actor_index]
                    .capture
                    .retain_recoil(target.reaction.recoil.kind);
            }
            // Contacts retain their authored release reaction without changing the
            // held pose or integrating an impulse before the capture ends.
            target.movement = movement;
            target.reaction.recoil = recoil;
            None
        } else {
            reaction
        };
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
        battle.unison_opener_contact(
            self.owner,
            source_action,
            actor,
            battle.actors[actor_index].hp == 0
                || (result.is_unblocked_damage() && !result.suppresses_reaction()),
        );
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
                if self.rule.reaction.capture
                    && result.guard == crate::GuardResult::None
                    && let Origin::Bone(anchor) | Origin::FrozenBone(_, anchor) = self.origin
                {
                    battle.capture_contact(self.owner, actor, source_action, anchor)?;
                }
            }
        }
        // Unblocked party damage gains Unison, including armored and lethal hits.
        if result.is_unblocked_damage() && battle.actors[self.owner.index()].side == Side::Party {
            battle.add_unison_gauge(16, cues);
        }
        battle.pause_overlimit_contact(actor, self.rule.overlimit_pause);
        let captured = battle.is_captured(actor);
        let target = &mut battle.actors[actor_index];
        if captured {
            // Damage continues while held; the existing hurt controller owns the victim.
        } else if !result.reaction_suppressed && crate::knockdown::threshold_reached(target) {
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
        let effect = feedback
            .filter(|feedback| feedback.on_guard || result.is_unblocked_damage())
            .map(|feedback| feedback.appearance)
            .or_else(|| {
                self.rule
                    .effect
                    .filter(|_| self.rule.effect_on_guard || result.is_unblocked_damage())
            });
        if let Some(appearance) = effect {
            cues.push(Cue::Effect(crate::EffectRequest {
                owner: self.owner,
                target: actor,
                appearance,
                origin: impact_position,
                heading: battle.actors[actor_index].heading,
                follow: None,
                scale: 1.,
                tint: Default::default(),
            }));
        }
        battle.actors[actor_index]
            .reaction
            .contact_feedback(result.guard);
        cues.push(Cue::Hit {
            source: self.source,
            owner: self.owner,
            actor,
            position: impact_position,
            element,
            contact_sound: feedback
                .and_then(|feedback| feedback.sound)
                .or(self.rule.sound.filter(|_| {
                    result.is_unblocked_damage()
                        || matches!(result.guard, crate::GuardResult::Blocked { .. })
                })),
            suppress_guard_sound: self.rule.guard.suppress_sound,
            was_casting,
            overlimit: battle.actors[actor_index].overlimit.is_active(),
            stunned: battle.activity(actor) == crate::Activity::Stunned,
            result,
        });
        if battle.actors[actor_index].hp == 0
            && (!unison_defeat_pending || result.is_damage())
            && (unison_defeat_pending
                || (!battle.restore_event_boss_hp(actor)
                    && !battle.try_lethal_rescue(actor, rescue_conditions, cues)?))
            && !battle.defer_unison_defeat(actor, self.rule.finisher_grade)
        {
            // Only a confirmed defeat earns kill credit and recovery.
            battle.enter_death(actor, cues);
            if self.rule.finisher_grade && battle.actors[actor_index].side == Side::Enemy {
                battle.ledger.finisher();
            }
            if battle.actors[self.owner.index()].side == Side::Party {
                battle.ledger.kill(self.owner);
            }
            if battle.actors[self.owner.index()].available() {
                battle.recover_after_kill(self.owner, cues);
            }
            battle.retarget_after_defeat(actor)?;
        }
        battle.record_enemy_contact(self.owner, result, battle.actors[actor_index].hp == 0);
        if let Some((action, sequence)) = battle.runtime[self.owner.index()].action_mut()
            && *action == source_action
        {
            sequence.contact_made = true;
        }
        if result.is_damage() {
            battle.contact_overlimit(actor, result.is_unblocked_damage());
        }

        // A capture that cannot acquire an already held victim cancels its
        // effective result; ordinary hits do not require a capture link.
        let capture_failed = self.rule.reaction.capture
            && battle.actors[actor_index].hp > 0
            && battle.is_captured(actor)
            && battle.held_victim(self.owner).is_none();
        // Only an effective hit can supply a steal victim or release an effective-hit
        // wait. Guarded, absorbed and protected contacts latch ordinary chaining.
        if (battle.actors[actor_index].hp == 0 || effective_hit)
            && let Some((action, sequence)) = battle.runtime[self.owner.index()].action_mut()
            && *action == source_action
        {
            sequence.struck_target = (!capture_failed).then_some(actor);
            sequence.struck_damage = if capture_failed {
                0
            } else {
                result.amount.max(0)
            };
        }
        match self.source {
            ContactSource::Projectile(id) => {
                let effective = battle.actors[actor_index].hp == 0 || effective_hit;
                let projectile = battle.projectiles.get_mut(&id).unwrap();
                projectile.seal_contact(actor, effective);
                projectile.hit(actor, result.guard != crate::GuardResult::None);
            }
            ContactSource::Melee {
                actor: owner,
                action,
            } => {
                if let Some((id, sequence)) = battle.runtime[owner.index()].action_mut()
                    && *id == action
                {
                    sequence.contact_target = Some(actor);
                    if let Some(window) = &mut sequence.melee {
                        window
                            .struck
                            .push((actor, window.definition.volume.cooldown()));
                    }
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
