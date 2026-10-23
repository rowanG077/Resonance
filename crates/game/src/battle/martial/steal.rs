//! Stealing keeps authored contacts and exchanges its tail on acquisition.
use super::{PreparedAttack, Sound, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result, ensure};
use resonance_battle::{AttackEvent, StealDefinition};
use std::sync::Arc;

impl Technique {
    pub(super) fn prepare_steal(
        self,
        resources: &mut Resources<'_>,
        model: Option<&resonance_battle::ModelDefinition>,
        tp_cost: u16,
    ) -> Result<PreparedAttack> {
        let mut timeline = self.authored_timeline(resources.voices.party_table())?;
        let recipe = timeline.steal.take().context("missing Item Thief recipe")?;
        ensure!(
            timeline.flight.is_none() && timeline.hammer.is_none(),
            "invalid Item Thief action variant"
        );
        let mut success = timeline.clone();
        success.duration = recipe.success_duration;
        success.chain_at = None;
        success.events = recipe.success_events;
        let mut success = self.prepare_authored_attack(resources, model, success)?;
        success.events.retain(|(at, _)| *at >= recipe.attempt_at);
        let recoil = resources.recoil;
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
            crate::battle::hit::authored(&recipe.bonus_hit, recoil)?,
            &[(1, &resources.techniques)],
        )?;
        Arc::make_mut(&mut projectile)
            .contact
            .as_mut()
            .context("Item Thief bonus has no contact")?
            .cooldown = recipe.bonus_hit.cooldown;
        let source = crate::battle::model::ModelSource::Party(self.character);
        let success_voice =
            resources
                .voices
                .absolute(source, Some(Sound::Stream(179)), &mut resources.sound)?;
        let failure_voice =
            resources
                .voices
                .absolute(source, Some(Sound::Stream(180)), &mut resources.sound)?;
        let mut definition = self.prepare_authored_attack(resources, model, timeline)?;
        definition.events.extend([
            (recipe.bonus_at, AttackEvent::StealBonus(projectile)),
            (
                recipe.attempt_at,
                AttackEvent::Steal(Arc::new(StealDefinition {
                    success: Arc::new(super::attack_definition(success, tp_cost)),
                    success_voice,
                    failure_voice,
                    gald: self.catalogue == 224,
                })),
            ),
        ]);
        definition.events.sort_by_key(|(at, _)| *at);
        Ok(definition)
    }
}
