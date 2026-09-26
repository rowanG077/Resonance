//! Shared 2503C timers run during entry as well as ordinary actor callbacks.
use crate::{ActorId, Battle, Cue, PreparedBattle};
use anyhow::{Result, ensure};

impl PreparedBattle {
    /// 2B7E4 is a no-op when the profile has no texture-expression channels.
    pub fn with_idle_expressions(mut self, expressions: Vec<Option<[u8; 4]>>) -> Result<Self> {
        ensure!(
            expressions.len() == self.actors.len(),
            "idle expression/actor count differs"
        );
        ensure!(
            expressions
                .iter()
                .zip(&self.models)
                .all(|(value, model)| value.is_none() || model.is_some()),
            "idle expression needs a prepared actor model"
        );
        self.idle_expressions = expressions;
        Ok(self)
    }
    pub fn with_blinking(mut self, enabled: Vec<bool>) -> Result<Self> {
        ensure!(
            enabled.len() == self.actors.len(),
            "blink/actor count differs"
        );
        ensure!(
            enabled
                .iter()
                .zip(&self.models)
                .all(|(&enabled, model)| !enabled || model.is_some()),
            "blinking needs a prepared actor model"
        );
        self.blinking = enabled;
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn restore_idle_expression(&mut self, index: usize) {
        if let Some(layers) = self.prepared.idle_expressions[index] {
            self.eye_expressions[index] = layers[0];
            self.models[index].as_mut().unwrap().shown.texture_layers = layers;
        }
    }

    /// Original 2B404: enemies always use normal idle; low-health party members
    /// keep an already active injured idle, or bind it with four more blend ticks.
    pub(crate) fn play_idle_pose(&mut self, actor: ActorId, blend: u8) -> Result<()> {
        if blend == 0 {
            return Ok(());
        }
        let index = actor.index();
        let owner = &self.actors[index];
        let fallback = self.decisions[index]
            .as_ref()
            .and_then(|d| d.definition.idle_motion)
            .or_else(|| {
                self.prepared.controls[index]
                    .as_ref()
                    .and_then(|d| d.motions)
                    .map(|m| m.idle)
            });
        let Some(model) = &mut self.models[index] else {
            return Ok(());
        };
        let Some(normal) = model.definition.idle_motions[0].or(fallback.map(|m| m.clip)) else {
            return Ok(());
        };
        let resource = model.definition.resource;
        let binding = |clip| crate::MotionBinding {
            model: resource,
            clip,
        };
        if owner.side == crate::Side::Party
            && i64::from(owner.hp) * 100 / i64::from(owner.max_hp) < 25
            && let Some(injured) = model.definition.idle_motions[1]
        {
            let injured = binding(injured);
            if model.is_playing(injured)? {
                return Ok(());
            }
            if matches!(owner.control, crate::Control::Auto | crate::Control::Enemy)
                || self.idle_timers[index] == 0
            {
                return model.play(injured, 0., 0.5, true, blend.wrapping_add(4));
            }
        }
        let normal = binding(normal);
        // 2C05C(enabled=0) returns before rebinding an identical body clip.
        if !model.is_playing(normal)? {
            model.play(normal, 0., 0.5, true, blend)?;
        }
        Ok(())
    }
    pub(crate) fn advance_actor_common(&mut self, index: usize, cues: &mut Vec<Cue>) -> Result<()> {
        self.melee[index].step();
        for timer in &mut self.trail_timers[index] {
            *timer = timer.saturating_sub(1);
        }
        let actor = &mut self.actors[index];
        actor.hit_stop = actor.hit_stop.saturating_sub(1);
        actor.reaction.stagger.window = actor.reaction.stagger.window.saturating_sub(1);
        actor.reaction.protection.step();
        actor.hud.common();
        actor.movement.advance_hover_phase();
        self.contact_feedback[index].step();
        if actor.hp > 0 {
            for (value, target) in actor.body.tint[..3]
                .iter_mut()
                .zip(self.prepared.ambient_color)
            {
                if *value > target {
                    *value -= 1;
                }
                if *value < target {
                    *value += 1;
                }
            }
        }
        actor.body.jitter.advance(&mut self.random);
        if self.prepared.blinking[index] && actor.available() {
            self.fidget_timers[index] = self.fidget_timers[index].wrapping_add(1);
            if self.fidget_timers[index] > 180 {
                self.fidget_timers[index] = 0;
            }
            let baseline = self.eye_expressions[index];
            if !matches!(baseline, 2 | 3 | 10) {
                let amount = match self.fidget_timers[index] {
                    176 => Some(1),
                    178 => Some(2),
                    180 => Some(baseline),
                    _ => None,
                };
                if let Some(amount) = amount {
                    self.models[index].as_mut().unwrap().shown.texture_layers[0] = amount;
                }
            }
        }
        self.voices[index].step(
            ActorId(index as u8),
            actor.body.audio_position,
            self.prepared.voices_enabled,
            &mut self.next_voice,
            cues,
        )?;
        self.contact_audio_actors[index].step();
        Ok(())
    }
}
