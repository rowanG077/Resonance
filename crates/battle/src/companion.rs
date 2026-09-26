//! Prepared companion policy inputs. Selection remains in maintained source.
use crate::{ActionPhase, ActorId, Battle, PreparedBattle};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone)]
pub struct CompanionTechnique {
    pub action: u16,
    pub enabled: bool,
    pub flags: u32,
    pub cost: u16,
    pub learning_route: u8,
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Debug, Clone)]
pub struct CompanionDefinition {
    pub actor: ActorId,
    pub strategy: [u8; 3],
    pub saved_position: u8,
    pub level: u8,
    pub level_difference: i8,
    pub tp_limit: u8,
    pub healing_limit: u8,
    pub support_level_limit: i8,
    pub techniques: Vec<CompanionTechnique>,
}

impl PreparedBattle {
    pub fn with_companions(mut self, definitions: Vec<CompanionDefinition>) -> Result<Self> {
        for definition in definitions {
            let index = definition.actor.index();
            ensure!(
                index < self.actors.len() && self.actors[index].side == crate::Side::Party,
                "companion policy needs a party actor"
            );
            ensure!(
                self.companions[index].is_none() && self.controls[index].is_some(),
                "companion policy needs unique normal bindings"
            );
            ensure!(
                (1..=9).contains(&definition.strategy[0])
                    && (1..=8).contains(&definition.strategy[1])
                    && (1..=6).contains(&definition.strategy[2])
                    && definition.level != 0
                    && (-8..=8).contains(&definition.level_difference)
                    && definition.tp_limit <= 100
                    && definition.healing_limit <= 100,
                "invalid companion policy parameters"
            );
            for technique in &definition.techniques {
                ensure!(
                    technique.minimum.is_finite()
                        && technique.maximum.is_finite()
                        && technique.minimum >= 0.
                        && technique.maximum > technique.minimum,
                    "invalid companion technique range"
                );
                ensure!(
                    self.actions
                        .iter()
                        .any(|action| action.id == technique.action
                            && matches!(action.phase, ActionPhase::Actor | ActionPhase::Casting)
                            && action.tp_cost == technique.cost),
                    "unprepared companion technique"
                );
            }
            self.companions[index] = Some(definition);
        }
        Ok(self)
    }
}

impl Battle {
    fn companion(&self, owner: ActorId) -> Result<&CompanionDefinition> {
        self.prepared.companions[owner.index()]
            .as_ref()
            .context("actor has no companion policy")
    }

    pub(crate) fn companion_parameters(&self, owner: ActorId) -> Result<Vec<i32>> {
        let p = self.companion(owner)?;
        let motion = self.prepared.controls[owner.index()]
            .as_ref()
            .context("companion has no movement parameters")?;
        Ok(vec![
            p.strategy[0].into(),
            p.strategy[1].into(),
            p.strategy[2].into(),
            p.saved_position.into(),
            p.level.into(),
            p.level_difference.into(),
            p.tp_limit.into(),
            p.healing_limit.into(),
            p.support_level_limit.into(),
            p.techniques.len() as i32,
            motion.run_speed.to_bits() as i32,
            motion.turn_ticks.into(),
            (motion.walk_speed * self.actors[owner.index()].body.scale).to_bits() as i32,
        ])
    }

    pub(crate) fn companion_technique(&self, owner: ActorId, index: usize) -> Result<Vec<i32>> {
        let technique = self
            .companion(owner)?
            .techniques
            .get(index)
            .context("invalid companion technique index")?;
        Ok(vec![
            technique.action.into(),
            technique.flags as i32,
            technique.cost.into(),
            technique.learning_route.into(),
            technique.minimum.to_bits() as i32,
            technique.maximum.to_bits() as i32,
            technique.enabled.into(),
        ])
    }

    pub(crate) fn companion_normal(&self, owner: ActorId, index: usize) -> Result<Vec<i32>> {
        let normal = self.prepared.controls[owner.index()]
            .as_ref()
            .context("actor has no normal bindings")?
            .normals
            .get(index)
            .context("invalid normal selector")?;
        Ok(vec![
            normal.action.into(),
            normal.allowed_directions.into(),
            normal.fallback.map_or(-1, i32::from),
            normal.minimum_reach.to_bits() as i32,
            normal.reach.to_bits() as i32,
        ])
    }

    pub(crate) fn companion_actor(&self, actor: ActorId) -> Result<Vec<i32>> {
        let actor = self.actor(actor)?;
        Ok(vec![
            actor.available().into(),
            (actor.availability == crate::ActorAvailability::Dead).into(),
            actor.hp_percent().into(),
            actor.tp_percent().into(),
            actor.tp.into(),
            actor.movement.flying.into(),
            actor.position[1].to_bits() as i32,
            matches!(actor.activity, crate::Activity::Guarding).into(),
            (matches!(actor.activity, crate::Activity::Casting { .. }) && !actor.hud.cast_released)
                .into(),
            actor.petrified.into(),
        ])
    }

    pub(crate) fn refresh_companion_retry(&mut self, owner: ActorId) -> Result<u8> {
        let actor = self.actor(owner)?;
        let target = self.actor(self.target(owner).context("companion has no target")?)?;
        // 1C40 fills1084 with 4DBA0 flat root distances before any actor callback.
        let position = self.target_positions[owner.index()];
        let nearby = self
            .actors
            .iter()
            .enumerate()
            .filter(|(i, other)| {
                other.side != actor.side
                    && crate::distance::length([
                        self.target_positions[*i][0] - position[0],
                        0.,
                        self.target_positions[*i][2] - position[2],
                    ]) <= 150.
            })
            .count();
        let flags = u8::from(nearby >= 2)
            | (u8::from(actor.hp_percent() <= 15) << 1)
            | (u8::from(
                matches!(target.activity, crate::Activity::Casting { .. })
                    && !target.hud.cast_released,
            ) << 2)
            | (u8::from(target.activity == crate::Activity::Guarding) << 3);
        self.controls[owner.index()]
            .as_mut()
            .context("companion has no control state")?
            .companion_retry = flags;
        Ok(flags)
    }
}
