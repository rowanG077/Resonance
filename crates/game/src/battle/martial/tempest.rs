//! Tempest spins through its authored strikes, then settles on the floor.
use super::{PreparedAttack, Technique};
use crate::battle::encounter::resources::Resources;
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, AttackPose, ModelDefinition};

impl Technique {
    pub(super) fn prepare_tempest(
        self,
        resources: &mut Resources<'_>,
        model: Option<&ModelDefinition>,
    ) -> Result<PreparedAttack> {
        let mut action = self.prepare_authored(resources, model)?;
        let recovery_pose =
            super::recovery_pose(&action).context("missing Tempest recovery pose")?;
        action
            .events
            .retain(|(at, event)| *at != action.end_at || !matches!(event, AttackEvent::Pose(_)));
        let landing_at = action
            .events
            .iter()
            .map(|(at, event)| {
                *at + match event {
                    AttackEvent::Contact { duration, .. } => *duration,
                    _ => 0,
                }
            })
            .max()
            .unwrap_or(0);
        // The landing clip blends for four updates and settles for fourteen.
        // The idle pose begins with recovery, independently of clip completion.
        let landing_clip = if self.catalogue == 17 { 18 } else { 16 };
        action.events.push((
            landing_at,
            AttackEvent::Land {
                pose: Some(AttackPose {
                    motion: crate::battle::model::motion(model, landing_clip)
                        .context("missing Tempest landing pose")?,
                    start: 0.,
                    end: None,
                    rate: 0.5,
                    blend: 4,
                    repeat: false,
                }),
                effect: None,
                settle: Some(14),
                recovery_pose: Some(recovery_pose),
            },
        ));
        Ok(action)
    }
}
