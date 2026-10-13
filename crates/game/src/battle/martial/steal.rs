//! Stealing keeps authored contacts and exchanges its tail on acquisition.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActionExecution, AttackEvent, StealDefinition};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_steal<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let mut timeline = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .clone();
        let recipe = timeline.steal.take().context("missing Item Thief recipe")?;
        ensure!(
            timeline.flight.is_none() && timeline.hammer.is_none(),
            "invalid Item Thief action variant"
        );
        let mut success = timeline.clone();
        success.duration = recipe.success_duration;
        success.chain_at = None;
        success.events = recipe.success_events;
        let mut success = self.prepare_authored_attack(resources, model, tp_cost, success)?;
        let ActionExecution::Attack(attack) = &mut success.execution else {
            unreachable!()
        };
        attack.events.retain(|(at, _)| *at >= recipe.attempt_at);
        let recoil = crate::battle::recoil::Parameters::load(resources.files)?;
        let mut row = resources
            .projectiles
            .records
            .get(7)
            .context("missing Item Thief bonus projectile")?
            .clone();
        const BONUS_BIRTH: u16 = 14;
        row.birth_effect.member = BONUS_BIRTH as u8;
        row.spawn_offset = recipe.bonus_offset;
        row.velocity[2] = recipe.bonus_forward_speed;
        row.motion.velocity_jitter = recipe.bonus_velocity_jitter;
        resources.effect(true, &[BONUS_BIRTH, u16::from(row.ground_effect.member)])?;
        let mut projectile = resources.prepared.projectile(
            resources.files,
            &row,
            crate::battle::hit::authored(&recipe.bonus_hit, &recoil)?,
            &[(1, &resources.techniques)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("Item Thief bonus has no contact")?
            .cooldown = recipe.bonus_hit.cooldown;
        let source = crate::battle::model::ModelSource::Party(self.character as u8);
        let success_voice =
            resources
                .voices
                .absolute(source, Some(Sound::Stream(179)), &mut resources.sound)?;
        let failure_voice =
            resources
                .voices
                .absolute(source, Some(Sound::Stream(180)), &mut resources.sound)?;
        let mut definition = self.prepare_authored_attack(resources, model, tp_cost, timeline)?;
        let ActionExecution::Attack(attack) = &mut definition.execution else {
            unreachable!()
        };
        attack.events.extend([
            (recipe.bonus_at, AttackEvent::StealBonus(projectile)),
            (
                recipe.attempt_at,
                AttackEvent::Steal(Arc::new(StealDefinition {
                    success: Arc::new(success),
                    success_voice,
                    failure_voice,
                    gald: self.catalogue == 224,
                })),
            ),
        ]);
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
