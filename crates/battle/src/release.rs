//! Released volleys retain their origin; projectiles own their flight.
use crate::{
    ActionId, ActorId, Battle, Cue, EffectAppearance, ProjectileDefinition, Sound, SpellSlot,
};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PreparedVolley {
    pub projectile: Arc<ProjectileDefinition>,
    pub shots: u8,
    /// Gameplay ticks between shots.
    pub interval: u16,
    pub startup: Option<EffectAppearance>,
    pub sound: Option<Sound>,
}

impl PreparedVolley {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.shots > 0 && self.interval > 0,
            "empty or unpaced projectile volley"
        );
        self.projectile.validate()
    }
}

/// A released spell owns its targeting and clock independently of the caster.
pub(crate) struct Volley {
    pub definition: Arc<PreparedVolley>,
    pub actor: ActorId,
    pub target: ActorId,
    pub slot: SpellSlot,
    pub age: u32,
    pub origin: Option<[f32; 3]>,
    pub emitted: u8,
}

impl Volley {
    pub(crate) fn step(
        &mut self,
        battle: &mut Battle,
        id: ActionId,
        cues: &mut Vec<Cue>,
    ) -> Result<bool> {
        let origin = if let Some(origin) = self.origin {
            origin
        } else {
            let target = &battle.actors[self.target.index()];
            if !target.available()
                && let Some(index) = battle
                    .actors
                    .iter()
                    .position(|a| a.side == target.side && a.available())
            {
                self.target = ActorId::from_index(index)?;
            }
            let actor = &battle.actors[self.actor.index()];
            let origin = actor.effect_origin();
            if let Some(appearance) = self.definition.startup {
                cues.push(Cue::Effect(crate::EffectRequest {
                    owner: self.actor,
                    target: self.actor,
                    appearance,
                    origin,
                    heading: actor.heading,
                    follow: None,
                    scale: actor.effect_scale,
                    tint: Default::default(),
                }));
            }
            self.origin = Some(origin);
            origin
        };
        if self.age >= u32::from(self.emitted) * u32::from(self.definition.interval) {
            battle.emit(
                Arc::clone(&self.definition.projectile),
                id,
                self.actor,
                self.target,
                origin,
            )?;
            if let Some(sound) = self.definition.sound {
                cues.push(Cue::Sound {
                    actor: self.actor,
                    sound,
                    position: origin,
                    priority: 2,
                });
            }
            self.emitted += 1;
        }
        if self.emitted == self.definition.shots {
            return Ok(true);
        }
        self.age += 1;
        Ok(false)
    }
}

impl Battle {
    pub(crate) fn spell_active(&self, actor: ActorId, slot: SpellSlot) -> bool {
        self.volleys
            .values()
            .any(|volley| volley.actor == actor && volley.slot == slot)
    }

    /// Release work is independent of the caster's current actor action.
    pub(crate) fn release_volley(
        &mut self,
        definition: Arc<PreparedVolley>,
        actor: ActorId,
        target: ActorId,
        slot: SpellSlot,
        parent: Option<ActionId>,
        cues: &mut Vec<Cue>,
    ) -> Result<Option<ActionId>> {
        if self.spell_active(actor, slot) {
            return Ok(None);
        }
        let volley = Volley {
            definition,
            actor,
            target,
            slot,
            age: 0,
            origin: None,
            emitted: 0,
        };
        let id = self.allocate_action_id()?;
        self.volleys.insert(id, volley);
        cues.push(Cue::Released {
            action: id,
            parent,
            actor,
            slot,
        });
        Ok(Some(id))
    }
}
