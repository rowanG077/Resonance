//! Tiger Blade's two strikes share one launch and finish after returning to the floor.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, AttackPose, ModelDefinition};

impl Technique {
    pub(super) fn prepare_tiger_blade<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let flight = resources
            .voices
            .party_table()
            .martial_attack(self.catalogue, self.character as u8)?
            .flight
            .as_ref()
            .context("missing Tiger Blade flight")?
            .clone();
        let mut action = self.prepare_authored(resources, model, tp_cost)?;
        let crate::battle::ActionExecution::Attack(attack) = &mut action.execution else {
            unreachable!("martial preparation produces an attack")
        };
        let finish = flight.finish_pose;
        let finish = AttackPose {
            motion: crate::battle::model::motion(model, finish.clip)
                .context("missing Tiger Blade finish pose")?,
            start: finish.start,
            end: finish.end,
            rate: finish.rate,
            blend: finish.blend,
            repeat: finish.repeat,
        };
        // The final idle pose waits for the floor; the strikes and launch keep
        // their authored times, independently of animation blending.
        attack
            .events
            .retain(|(at, event)| *at != attack.end_at || !matches!(event, AttackEvent::Pose(_)));
        attack.events.push((
            attack.end_at,
            AttackEvent::Land {
                pose: Some(finish),
                effect: None,
                settle: None,
                recovery_pose: None,
            },
        ));
        attack.events.sort_by_key(|(at, _)| *at);
        Ok(action)
    }
}
