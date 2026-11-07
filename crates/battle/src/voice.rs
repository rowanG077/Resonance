use crate::{ActorId, Cue, Sound};
use anyhow::{Result, ensure};

/// The audio owner arbitrates these requests independently of gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VoicePriority {
    Reaction,
    Action,
    Announcement,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_choices_leave_gameplay_randomness_unchanged() -> Result<()> {
        use crate::{ActionKey, ActionRequest, ActorSetup, AttackEvent, BattleInput, Side};
        for chance in [0, 50, 100] {
            let mut action = crate::tests::action(4);
            crate::tests::attack_mut(&mut action).events.push((
                0,
                AttackEvent::VoiceChoice {
                    choice: resonance_content::battle_voice::Choice {
                        sounds: [Some(Sound::Cue(10)), Some(Sound::Cue(20))],
                        first_chance: chance,
                    },
                    priority: VoicePriority::Action,
                },
            ));
            let mut prepared = crate::PreparedBattle::new(
                [Side::Party, Side::Enemy]
                    .map(|side| (crate::tests::actor(side), ActorSetup::default()))
                    .into(),
                vec![action].into(),
                1234,
            )?;
            crate::tests::assign_action(&mut prepared, 0, ActionKey(0));
            let mut battle = prepared.finish()?;
            let before = battle.random_state();
            let frame = battle.step(BattleInput {
                actions: vec![ActionRequest {
                    actor: ActorId(0),
                    action: ActionKey(0),
                    target: ActorId(1),
                }],
                ..Default::default()
            })?;
            assert_eq!(battle.random_state(), before);
            assert!(frame.cues.iter().any(
                |cue| matches!(cue, Cue::Voice { action: Some(_), sound: Sound::Cue(line), .. }
                if (*line == 10 && chance > 0) || (*line == 20 && chance < 100))
            ));
        }
        Ok(())
    }

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
        self.request_action_voice(actor, sound, priority, None);
    }

    pub(crate) fn request_action_voice(
        &mut self,
        actor: ActorId,
        sound: Sound,
        priority: VoicePriority,
        action: Option<crate::ActionId>,
    ) {
        let cue = Cue::Voice {
            actor,
            action,
            sound,
            priority,
            position: self.actors[actor.index()].audio_position(),
            centered: false,
        };
        self.record_voice_requests(std::slice::from_ref(&cue));
        self.pending_cues.push(cue);
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
        self.pending_cues.retain(|cue| match cue {
            Cue::Voice {
                actor: owner,
                priority,
                ..
            } => *owner != actor || *priority > max_priority,
            Cue::TechniqueQueued { actor: owner } => *owner != actor,
            _ => true,
        });
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
