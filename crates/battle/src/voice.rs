use crate::{ActorId, Cue, Sound};
use anyhow::{Result, ensure};

/// The audio owner arbitrates these requests independently of gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VoicePriority {
    Reaction,
    Action,
    Announcement,
}

impl crate::Battle {
    pub(crate) fn request_voice(&mut self, actor: ActorId, sound: Sound, priority: VoicePriority) {
        self.pending_cues.push(Cue::Voice {
            actor,
            sound,
            priority,
            position: self.actors[actor.index()].audio_position(),
            centered: false,
        });
    }

    pub(crate) fn stop_voice(&mut self, actor: ActorId, cues: &mut Vec<Cue>) {
        self.pending_cues
            .retain(|cue| !matches!(cue, Cue::Voice { actor: owner, .. } | Cue::TechniqueQueued { actor: owner } if *owner == actor));
        cues.push(Cue::StopVoice { actor });
    }

    pub fn stop_result_voices(&mut self) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == crate::BattlePhase::Results,
            "result voice stop outside results"
        );
        let mut cues = Vec::new();
        for index in 0..self.actors.len() {
            if self.actors[index].side == crate::Side::Party {
                self.stop_voice(ActorId(index as u8), &mut cues);
            }
        }
        Ok(cues)
    }

    pub fn request_result_voice(&mut self, actor: ActorId, line: crate::Sound) -> Result<()> {
        ensure!(
            self.phase() == crate::BattlePhase::Results,
            "result voice outside results"
        );
        self.actor(actor)?;
        self.request_voice(actor, line, VoicePriority::Announcement);
        Ok(())
    }
}
