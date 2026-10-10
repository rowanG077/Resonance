//! Optional poses for the result screen.
use crate::{ActorId, Battle, BattlePhase, MotionBinding, Side};
use anyhow::{Result, ensure};

impl Battle {
    /// Play a celebration once; the model holds its final pose independently of results.
    pub fn play_victory_pose(&mut self, actor: ActorId, motion: MotionBinding) -> Result<()> {
        self.set_result_posture(actor, None, [0; 4])?;
        self.request_pose(
            actor,
            Some(motion),
            crate::Pose {
                blend: 12,
                ..Default::default()
            },
        );
        Ok(())
    }

    /// Petrified actors can change expression without replacing their frozen pose.
    pub fn set_result_posture(
        &mut self,
        actor: ActorId,
        motion: Option<MotionBinding>,
        expression: [u8; 4],
    ) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "result posture outside results"
        );
        ensure!(
            self.actor(actor)?.side == Side::Party,
            "result posture needs a party actor"
        );
        self.model_requests.push(crate::ModelRequest::Expression {
            actor,
            layers: expression,
        });
        self.request_pose(
            actor,
            motion,
            crate::Pose {
                repeat: true,
                blend: 0,
                ..Default::default()
            },
        );
        Ok(())
    }
}
