//! Tiger Blade's two strikes share one launch and finish after returning to the floor.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, ModelDefinition};

impl Technique {
    pub(super) fn prepare_tiger_blade<
        S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
    >(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let action = self.prepare_authored(resources, model, tp_cost)?;
        finish_on_landing(action)
    }
}

pub(super) fn finish_on_landing(mut action: ActionDefinition) -> Result<ActionDefinition> {
    let crate::battle::ActionExecution::Attack(attack) = &mut action.execution else {
        unreachable!("martial preparation produces an attack")
    };
    let finish = attack
        .events
        .iter()
        .find_map(|(at, event)| match event {
            AttackEvent::Pose(pose) if *at == attack.end_at => Some(*pose),
            _ => None,
        })
        .context("missing Tiger Blade recovery pose")?;
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
