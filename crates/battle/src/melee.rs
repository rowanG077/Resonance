//! Actor hit streams and per-target admission (2D564, 2503C, 3BDF8).
use crate::{Actor, ActorId, HitRule, HitShape};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// One prepared hit row. The loader resolves model/weapon groups to pose anchors.
/// Window timing and sequencing stay in the authored task.
#[derive(Debug, Clone)]
pub struct MeleeDefinition {
    pub hit: HitRule,
    pub cooldown: u8,
    pub radius: f32,
    pub height: f32,
    pub shape: HitShape,
    pub anchors: Vec<u16>,
    /// First original attachment slot, when 2D564 also extends its blade ribbon.
    pub trail: Option<u8>,
}

impl MeleeDefinition {
    pub(crate) fn validate(&self) -> Result<()> {
        self.hit.reaction.validate()?;
        ensure!(
            self.trail.is_none_or(|slot| slot < 4),
            "invalid melee trail slot"
        );
        ensure!(
            self.radius.is_finite()
                && self.radius >= 0.
                && self.height.is_finite()
                && self.height >= 0.
                && match self.shape {
                    HitShape::Ring { width } => width.is_finite(),
                    _ => true,
                }
                && !self.anchors.is_empty()
                && self.anchors.len() <= 40,
            "invalid melee contact"
        );
        Ok(())
    }
}

pub(crate) struct Window {
    pub task: i32,
    pub definition: Arc<MeleeDefinition>,
    pub start: i16,
    pub end: i16,
}

/// Shared by every origin of an actor's melee hit stream. Projectiles own their
/// separate caches. Closing/cancelling a task does not retract queued contacts.
#[derive(Default, Clone)]
pub(crate) struct HitCache {
    struck: [bool; 12],
    cooldowns: [u8; 12],
}

impl HitCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn step(&mut self) {
        for value in &mut self.cooldowns {
            *value = value.saturating_sub(1);
        }
    }
    pub fn can_hit(&self, actor: ActorId) -> bool {
        !self.struck[actor.index()] && self.cooldowns[actor.index()] == 0
    }
    pub fn hit(&mut self, actor: ActorId, cooldown: u8) {
        self.struck[actor.index()] = true;
        self.cooldowns[actor.index()] = cooldown;
    }
}

impl Window {
    pub fn validate_anchors(&self, actor: &Actor) -> Result<()> {
        ensure!(
            self.definition
                .anchors
                .iter()
                .all(|&i| usize::from(i) < actor.body.anchors.len()),
            "unprepared melee pose anchor"
        );
        Ok(())
    }
}
