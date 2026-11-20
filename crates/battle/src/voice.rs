use crate::{ActorId, Cue, Sound};
use anyhow::{Result, ensure};

/// The audio owner arbitrates these requests independently of gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VoicePriority {
    /// Wait for this actor’s earlier speech, then play with announcement priority.
    /// Accepting a newer ordinary line cancels the waiting request.
    AfterSpeech,
    Reaction,
    Action,
    Announcement,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_requests_count_once_and_result_voices_do_not_count() {
        let mut battle = crate::PreparedBattle::new(
            [crate::Side::Party, crate::Side::Enemy]
                .map(|side| (crate::tests::actor(side), Default::default()))
                .into(),
            Default::default(),
            1,
        )
        .unwrap()
        .finish()
        .unwrap();
        let actor = ActorId(0);
        battle.request_voice(actor, Sound::Stream(1), VoicePriority::Action);
        let frame = battle.publish(Vec::new());
        assert_eq!(battle.ledger().achievements[0].voices, 1);
        assert_eq!(
            frame
                .cues
                .iter()
                .filter(|cue| matches!(cue, Cue::Voice { .. }))
                .count(),
            1
        );
        battle.actors[1].hp = 0;
        battle.actors[1].availability = crate::ActorAvailability::Dead;
        assert_eq!(
            battle.recognize_result(),
            Some(crate::BattleResult::Victory)
        );
        battle.retire_combat().unwrap();
        battle
            .request_result_voice(actor, Sound::Stream(2))
            .unwrap();
        assert_eq!(battle.ledger().achievements[0].voices, 1);
    }
}

impl crate::Battle {
    pub(crate) fn request_voice(&mut self, actor: ActorId, sound: Sound, priority: VoicePriority) {
        let cue = self.voice_cue(actor, sound, priority, None);
        self.pending_cues.push(cue);
    }

    /// Record the request when its gameplay event creates the cue.
    pub(crate) fn voice_cue(
        &mut self,
        actor: ActorId,
        sound: Sound,
        priority: VoicePriority,
        action: Option<crate::ActionId>,
    ) -> Cue {
        let cue = Cue::Voice {
            actor,
            action,
            sound,
            priority,
            position: self.actors[actor.index()].audio_position(),
            centered: false,
        };
        self.record_voice_requests(std::slice::from_ref(&cue));
        cue
    }

    /// Count newly resolved voice requests once, independently of audio playback.
    /// Scene feedback supplies only the cues it added, excluding core requests.
    pub fn record_voice_requests(&mut self, cues: &[Cue]) {
        if self.phase() != crate::BattlePhase::Combat {
            return;
        }
        for cue in cues {
            if let Cue::Voice { actor, .. } = cue {
                let count = &mut self.ledger.achievements[actor.index()].voices;
                *count = count.saturating_add(1);
            }
        }
    }

    pub(crate) fn stop_voice(
        &mut self,
        actor: ActorId,
        max_priority: VoicePriority,
        cues: &mut Vec<Cue>,
    ) {
        let keep = |cue: &Cue| match cue {
            Cue::Voice {
                actor: owner,
                priority,
                ..
            } => *owner != actor || *priority > max_priority,
            Cue::TechniqueQueued { actor: owner } => *owner != actor,
            _ => true,
        };
        cues.retain(keep);
        self.pending_cues.retain(keep);
        cues.push(Cue::StopVoice {
            actor,
            max_priority,
        });
    }

    pub fn stop_result_voices(&mut self) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == crate::BattlePhase::Results,
            "result voice stop outside results"
        );
        let mut cues = Vec::new();
        for index in 0..self.actors.len() {
            if self.actors[index].side == crate::Side::Party {
                self.stop_voice(ActorId(index as u8), VoicePriority::Announcement, &mut cues);
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
