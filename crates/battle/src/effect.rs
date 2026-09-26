//! Effect programs share the action VM; their object-group lifetime is independent
//! of the action which emitted them (40210, 420C4, 40188 and 11E8C).
use crate::{
    ActionId, ActorId, Battle, Cue, EffectAppearance,
    script::{Sequence, step_sequence},
};
use anyhow::{Context as _, Result, ensure};
pub use resonance_content::battle_effect::EffectTint;

#[derive(Clone, Copy)]
pub(crate) enum Follow {
    Actor(ActorId),
    Center(ActorId),
    Projectile(crate::ProjectileId),
}

pub(crate) struct Spawn {
    pub action: ActionId,
    pub scene: Option<ActionId>,
    pub owner: ActorId,
    pub target: ActorId,
    pub appearance: EffectAppearance,
    pub origin: [f32; 3],
    pub heading: f32,
    pub follow: Option<Follow>,
    pub scale: f32,
    pub late: bool,
    pub tint: EffectTint,
}

pub(crate) struct Context {
    pub scene: Option<ActionId>,
    pub resource: u32,
    pub origin: [f32; 3],
    pub heading: f32,
    pub retiring: bool,
    pub values: [f32; 4],
    pub integers: [i16; 4],
    pub follow: Option<Follow>,
    pub scale: f32,
    pub late: bool,
    pub tint: EffectTint,
}

/// The actor's embedded effect timeline (15B4), not an allocated object-group
/// effect. Dropping its action cancels pending emissions; particles live on.
pub(crate) struct Attached {
    sequence: Box<Sequence>,
    initializing: bool,
}

impl Battle {
    pub(crate) fn attach_effect(
        &mut self,
        id: ActionId,
        action: &mut Sequence,
        appearance: EffectAppearance,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let definition = self
            .prepared
            .effects
            .get(&appearance.resource)
            .and_then(|bank| bank.members.get(&appearance.member))
            .context("unprepared attached effect member")?;
        let actor = &self.actors[action.actor.index()];
        let mut sequence = Sequence::new(definition, action.actor, action.actor)?;
        sequence.effect = Some(Context {
            scene: None,
            resource: appearance.resource,
            origin: actor.position,
            heading: actor.heading,
            retiring: false,
            values: [0.; 4],
            integers: [0; 4],
            follow: Some(Follow::Actor(action.actor)),
            scale: 1.,
            late: false,
            tint: Default::default(),
        });
        cues.push(Cue::Effect {
            action: id,
            resource: appearance.resource,
            member: appearance.member,
            position: actor.position,
            heading: actor.heading,
        });
        action.attached = Some(Attached {
            sequence: Box::new(sequence),
            initializing: true,
        });
        Ok(())
    }

    pub(crate) fn advance_attached(
        &mut self,
        id: ActionId,
        action: &mut Sequence,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let Some(mut attached) = action.attached.take() else {
            return Ok(());
        };
        // 3823C selects the program; the first 420C4 visit is the following
        // actor callback. 2C5B4 holds this clock only for actor-local hit-stop.
        if attached.initializing {
            attached.initializing = false;
        } else if self.actors[action.actor.index()].hit_stop == 0 {
            let sequence = &mut attached.sequence;
            sequence.effect.as_mut().unwrap().origin = self.actors[action.actor.index()].position;
            step_sequence(self, id, sequence, cues)?;
            if sequence.finished {
                return Ok(());
            }
            sequence.age += 1;
        }
        action.attached = Some(attached);
        Ok(())
    }

    pub(crate) fn follow_position(&self, follow: Follow) -> Option<[f32; 3]> {
        match follow {
            Follow::Actor(actor) => Some(self.actors[actor.index()].position),
            Follow::Center(actor) => Some(self.actors[actor.index()].body.center),
            Follow::Projectile(id) => self.projectiles.get(&id).map(|p| p.frame.position),
        }
    }

    pub(crate) fn object_available(&self) -> bool {
        // 11FCC initializes 416 shared object slots. Actor callbacks have slots;
        // released spell contexts live inside actors and do not allocate here.
        self.actors.len()
            + self.projectile_count()
            + self
                .projectiles
                .values()
                .filter(|p| p.frame.shadow.is_some())
                .count()
            + self.particles.len()
            + self.effects_in_flight
            + self
                .sequences
                .values()
                .filter(|s| s.effect.is_some())
                .count()
            < 416
    }

    pub(crate) fn show_effect(&mut self, spawn: Spawn, cues: &mut Vec<Cue>) -> Result<()> {
        let scope = format!(
            "battle effect {}:{}",
            spawn.appearance.resource, spawn.appearance.member
        );
        let result = self.show_effect_inner(spawn, cues);
        if let Err(error) = result {
            self.diagnostics.report(&scope, error)?;
            self.diagnostic = true;
        }
        Ok(())
    }

    fn show_effect_inner(&mut self, spawn: Spawn, cues: &mut Vec<Cue>) -> Result<()> {
        let definition = self
            .prepared
            .effects
            .get(&spawn.appearance.resource)
            .and_then(|bank| bank.members.get(&spawn.appearance.member))
            .context("unprepared battle effect member")?
            .clone();
        if !self.object_available() {
            return Ok(());
        }
        ensure!(
            self.effects_in_flight < symphonia_script::authored::CALL_FRAME_LIMIT,
            "battle effect activation limit exceeded (64)"
        );
        let id = ActionId(self.next_action);
        self.next_action = self
            .next_action
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("battle action handle exhausted"))?;
        let scene = spawn.scene;
        let mut sequence = Sequence::new(&definition, spawn.owner, spawn.target)?;
        sequence.effect = Some(Context {
            scene,
            resource: spawn.appearance.resource,
            origin: spawn.origin,
            heading: spawn.heading,
            retiring: false,
            values: [0.; 4],
            integers: [0; 4],
            follow: spawn.follow,
            scale: spawn.scale,
            late: spawn.late,
            tint: spawn.tint,
        });
        cues.push(Cue::Effect {
            action: spawn.action,
            resource: spawn.appearance.resource,
            member: spawn.appearance.member,
            position: spawn.origin,
            heading: spawn.heading,
        });
        self.effects_in_flight += 1;
        let result = step_sequence(self, id, &mut sequence, cues);
        self.effects_in_flight -= 1;
        if let Err(error) = result {
            // Any already spawned particles belong to this failed constructor.
            // Keep them out of the following object pass in tolerant mode.
            if !self.diagnostics.paranoid() {
                self.discard_action(id, sequence.actor, sequence.definition.phase, cues);
            }
            return Err(error);
        }
        if !sequence.finished {
            sequence.age += 1;
        }
        // The constructor installs the active callback even if its initial visit
        // reached End. Cleanup follows the later retirement callback.
        self.sequences.insert(id, sequence);
        Ok(())
    }

    pub(crate) fn advance_effects(&mut self, late: bool, cues: &mut Vec<Cue>) -> Result<()> {
        let ids: Vec<_> = self
            .sequences
            .iter()
            .rev()
            .filter(|(_, s)| s.effect.as_ref().is_some_and(|effect| effect.late == late))
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            let Some(mut sequence) = self.sequences.remove(&id) else {
                // A failed scene owner may already have retired its children.
                continue;
            };
            if sequence.effect.as_ref().unwrap().retiring {
                continue;
            }
            if !sequence.finished {
                let effect = sequence.effect.as_mut().unwrap();
                // 420C4 snapshots the followed origin at each timeline visit.
                // Retirement leaves the last position; never follow a recycled
                // object slot. Particle attachments have their own visit below.
                let origin = effect
                    .follow
                    .and_then(|follow| self.follow_position(follow));
                if let Some(origin) = origin {
                    effect.origin = origin;
                }
                self.effects_in_flight += 1;
                let result = step_sequence(self, id, &mut sequence, cues);
                self.effects_in_flight -= 1;
                if let Err(error) = result {
                    self.diagnostics.report("battle effect action", error)?;
                    self.diagnostic = true;
                    self.discard_action(id, sequence.actor, sequence.definition.phase, cues);
                    continue;
                }
            }
            if sequence.finished {
                sequence.effect.as_mut().unwrap().retiring = true;
            } else {
                sequence.age += 1;
            }
            self.sequences.insert(id, sequence);
        }
        Ok(())
    }
}

#[cfg(test)]
mod attached_tests;
