//! Tiger Blade's two strikes share one launch and finish after returning to the floor.
use super::{PreparedAttack, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, ModelDefinition};

impl Technique {
    pub(super) fn prepare_tiger_blade(
        self,
        resources: &mut Resources<'_>,
        model: Option<&ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let action = self.prepare_authored(resources, model)?;
        finish_on_landing(action)
    }
}

pub(super) fn finish_on_landing(mut action: PreparedAttack) -> Result<PreparedAttack> {
    let finish = super::recovery_pose(&action).context("missing Tiger Blade recovery pose")?;
    // The final idle pose waits for the floor; the strikes and launch keep
    // their authored times, independently of animation blending.
    action
        .events
        .retain(|(at, event)| *at != action.end_at || !matches!(event, AttackEvent::Pose(_)));
    action.events.push((
        action.end_at,
        AttackEvent::Land {
            pose: Some(finish),
            effect: None,
            settle: None,
            recovery_pose: None,
        },
    ));
    action.events.sort_by_key(|(at, _)| *at);
    Ok(action)
}
