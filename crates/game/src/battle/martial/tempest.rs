//! Tempest spins through its authored strikes, then settles on the floor.
use super::{ActionDefinition, Sound, Technique};
use anyhow::{Context, Result};
use resonance_battle::{AttackEvent, AttackPose, ModelDefinition};

impl Technique {
    pub(super) fn prepare_tempest<S: FnMut(Sound) -> Result<Option<resonance_battle::Sound>>>(
        self,
        resources: &mut crate::battle::encounter::resources::Resources<'_, S>,
        model: Option<&ModelDefinition>,
        tp_cost: u16,
    ) -> Result<ActionDefinition> {
        let mut action = self.prepare_authored(resources, model, tp_cost)?;
        let crate::battle::ActionExecution::Attack(attack) = &mut action.execution else {
            unreachable!("martial preparation produces an attack")
        };
        let recovery_pose = attack
            .events
            .iter()
            .find_map(|(at, event)| match event {
                AttackEvent::Pose(pose) if *at == attack.end_at => Some(*pose),
                _ => None,
            })
            .context("missing Tempest recovery pose")?;
        attack
            .events
            .retain(|(at, event)| *at != attack.end_at || !matches!(event, AttackEvent::Pose(_)));
        let landing_at = attack
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
        attack.events.push((
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
