//! Health-triggered speech retains combat while ducking other actors' voices.
use crate::{
    Actor, ActorId, ActorSet, Battle, BattlePhase, Cue, PreparedBattle, Side, Sound, VoicePriority,
};
use anyhow::{Result, ensure};
use resonance_content::battle_enemy::ConversationTiming;

const LINE_TAIL: u32 = 30;
const DUCK_PERCENT: u8 = 60;

#[derive(Debug, Clone)]
pub struct EnemyConversation {
    pub actor: ActorId,
    pub hp_percent: u8,
    pub timing: ConversationTiming,
    pub lines: Vec<EnemyConversationLine>,
}

#[derive(Debug, Clone, Copy)]
pub struct EnemyConversationLine {
    pub speaker: ActorId,
    pub sound: Option<Sound>,
    pub ticks: u16,
}

#[derive(Default)]
pub(crate) struct State {
    pending: Vec<EnemyConversation>,
    playing: Option<Playing>,
}

struct Playing {
    definition: EnemyConversation,
    next: usize,
    remaining: u32,
    auto_disabled: bool,
}

impl State {
    pub(crate) fn active(&self, actor: ActorId) -> bool {
        self.playing
            .as_ref()
            .is_some_and(|playing| playing.definition.actor == actor)
    }

    pub(crate) fn new(pending: Vec<EnemyConversation>) -> Self {
        Self {
            pending,
            playing: None,
        }
    }

    fn advance(&mut self, actors: &mut [Actor], voice_busy: Option<ActorSet>, cues: &mut Vec<Cue>) {
        if let Some(playing) = &mut self.playing {
            if playing.advance(actors, voice_busy, cues) {
                self.finish(actors, cues);
            }
        } else if let Some(index) = self.pending.iter().position(|definition| {
            let actor = &actors[definition.actor.index()];
            actor.available() && actor.hp_percent() <= i16::from(definition.hp_percent)
        }) {
            let definition = self.pending.remove(index);
            let owner = definition.actor;
            let spaced = definition.timing == ConversationTiming::Spaced;
            voice_volumes(
                actors.len(),
                if spaced { DUCK_PERCENT } else { 50 },
                (!spaced).then_some(owner),
                cues,
            );
            self.playing = Some(Playing {
                definition,
                next: 0,
                remaining: 0,
                auto_disabled: std::mem::replace(
                    &mut actors[owner.index()].guard.auto_disabled,
                    true,
                ),
            });
        }
    }

    pub(crate) fn finish(&mut self, actors: &mut [Actor], cues: &mut Vec<Cue>) {
        if let Some(playing) = self.playing.take() {
            actors[playing.definition.actor.index()].guard.auto_disabled = playing.auto_disabled;
            voice_volumes(actors.len(), 100, None, cues);
        }
    }
}

impl PreparedBattle {
    pub fn with_enemy_conversations(mut self, definitions: Vec<EnemyConversation>) -> Result<Self> {
        let mut owners = std::collections::BTreeSet::new();
        for definition in &definitions {
            ensure!(
                self.actors
                    .get(definition.actor.index())
                    .is_some_and(|a| a.side == Side::Enemy)
                    && owners.insert(definition.actor)
                    && definition.hp_percent <= 100
                    && !definition.lines.is_empty()
                    && definition
                        .lines
                        .iter()
                        .all(|line| self.actors.get(line.speaker.index()).is_some()),
                "invalid enemy conversation owner, health or speaker"
            );
        }
        self.resources.enemy_conversations = definitions;
        Ok(self)
    }
}

impl Battle {
    pub(crate) fn advance_enemy_conversations(
        &mut self,
        voice_busy: Option<ActorSet>,
        cues: &mut Vec<Cue>,
    ) {
        if self.phase() != BattlePhase::Combat || self.unison.active() {
            return;
        }
        let first_cue = cues.len();
        self.enemy_conversations
            .advance(&mut self.actors, voice_busy, cues);
        self.record_voice_requests(&cues[first_cue..]);
    }
}

fn voice_volumes(actors: usize, percent: u8, speaker: Option<ActorId>, cues: &mut Vec<Cue>) {
    for index in 0..actors {
        let actor = ActorId(index as u8);
        cues.push(Cue::VoiceVolume {
            actor,
            percent: if Some(actor) == speaker { 100 } else { percent },
        });
    }
}

impl Playing {
    fn advance(
        &mut self,
        actors: &[Actor],
        voice_busy: Option<ActorSet>,
        cues: &mut Vec<Cue>,
    ) -> bool {
        let owner = self.definition.actor;
        let spaced = self.definition.timing == ConversationTiming::Spaced;
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining != 0 {
            return false;
        }
        // Continuous speech releases combat on the update after the final request,
        // while its audio continues independently.
        let Some(line) = self.definition.lines.get(self.next) else {
            return true;
        };
        let previous = self
            .next
            .checked_sub(1)
            .map(|index| self.definition.lines[index].speaker);
        let voice_playing =
            previous.is_some_and(|speaker| voice_busy.is_some_and(|busy| busy.contains(speaker)));
        if !spaced && voice_playing {
            return false;
        }
        if spaced || previous.unwrap_or(owner) != line.speaker {
            voice_volumes(
                actors.len(),
                if spaced { DUCK_PERCENT } else { 50 },
                Some(line.speaker),
                cues,
            );
        }
        let speaker = &actors[line.speaker.index()];
        if speaker.available()
            && let Some(sound) = line.sound
        {
            cues.push(Cue::Voice {
                actor: line.speaker,
                sound,
                priority: VoicePriority::Announcement,
                position: speaker.audio_position(),
                centered: true,
            });
        }
        self.next += 1;
        self.remaining = if spaced {
            u32::from(line.ticks) + LINE_TAIL
        } else if voice_busy.is_none() && self.next < self.definition.lines.len() {
            u32::from(line.ticks)
        } else {
            0
        };
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorAvailability, BattleInput};

    fn conversation(actor: ActorId, speakers: &[ActorId]) -> EnemyConversation {
        EnemyConversation {
            actor,
            hp_percent: 100,
            timing: ConversationTiming::Continuous,
            lines: speakers
                .iter()
                .enumerate()
                .map(|(index, &speaker)| EnemyConversationLine {
                    speaker,
                    sound: Some(Sound::Stream(7 + index as u16)),
                    ticks: 2,
                })
                .collect(),
        }
    }

    fn battle(sides: &[Side], conversations: Vec<EnemyConversation>) -> Result<Battle> {
        PreparedBattle::new(
            sides
                .iter()
                .map(|&side| (crate::tests::actor(side), Default::default()))
                .collect(),
            Default::default(),
            1,
        )?
        .with_enemy_conversations(conversations)?
        .finish()
    }

    #[test]
    fn continuous_exchange_waits_between_lines_and_releases_combat_during_the_final_line()
    -> Result<()> {
        let mut battle = battle(
            &[Side::Party, Side::Enemy],
            vec![conversation(ActorId(1), &[ActorId(1), ActorId(0)])],
        )?;
        let step = |battle: &mut Battle, speaker: Option<ActorId>| -> Result<Vec<Cue>> {
            let mut busy = crate::ActorSet::default();
            if let Some(speaker) = speaker {
                busy.insert(speaker);
            }
            Ok(battle
                .step(BattleInput {
                    voice_busy: Some(busy),
                    ..Default::default()
                })?
                .cues)
        };
        step(&mut battle, None)?;
        assert!(step(&mut battle, None)?.iter().any(|cue| matches!(
            cue,
            Cue::Voice {
                actor: ActorId(1),
                ..
            }
        )));
        for _ in 0..5 {
            assert!(step(&mut battle, Some(ActorId(1)))?.is_empty());
        }
        let cues = step(&mut battle, None)?;
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::Voice {
                actor: ActorId(0),
                ..
            }
        )));
        assert!(cues.iter().any(|cue| matches!(
            cue,
            Cue::VoiceVolume {
                actor: ActorId(0),
                percent: 100
            }
        )));
        assert!(battle.enemy_conversations.active(ActorId(1)));
        assert!(battle.actors[1].guard.auto_disabled);
        assert_eq!(
            step(&mut battle, Some(ActorId(0)))?,
            vec![
                Cue::VoiceVolume {
                    actor: ActorId(0),
                    percent: 100
                },
                Cue::VoiceVolume {
                    actor: ActorId(1),
                    percent: 100
                },
            ]
        );
        assert!(!battle.enemy_conversations.active(ActorId(1)));
        assert!(!battle.actors[1].guard.auto_disabled);
        for _ in 0..5 {
            assert!(step(&mut battle, Some(ActorId(0)))?.is_empty());
        }
        Ok(())
    }

    #[test]
    fn simultaneous_thresholds_serialize_and_preserve_existing_guard_settings() -> Result<()> {
        let mut battle = battle(
            &[Side::Party, Side::Enemy, Side::Enemy],
            vec![
                conversation(ActorId(1), &[ActorId(1)]),
                conversation(ActorId(2), &[ActorId(2)]),
            ],
        )?;
        battle.actors[1].guard.auto_disabled = true;
        let mut heard = Vec::new();
        for _ in 0..12 {
            let frame = battle.step(BattleInput::default())?;
            for cue in frame.cues {
                if let Cue::Voice { actor, .. } = cue {
                    heard.push(actor);
                }
            }
        }
        assert_eq!(heard, [ActorId(1), ActorId(2)]);
        assert!(battle.actors[1].guard.auto_disabled);
        assert!(!battle.actors[2].guard.auto_disabled);
        Ok(())
    }

    #[test]
    fn ending_combat_restores_conversation_state_before_the_next_world_update() -> Result<()> {
        use crate::BattleResult;
        for result in [
            BattleResult::Victory,
            BattleResult::Defeat,
            BattleResult::Escaped,
        ] {
            let mut battle = battle(
                &[Side::Party, Side::Enemy],
                vec![conversation(ActorId(1), &[ActorId(1), ActorId(0)])],
            )?;
            battle.step(BattleInput::default())?;
            battle.step(BattleInput::default())?;
            assert!(battle.enemy_conversations.active(ActorId(1)));
            assert!(battle.actors[1].guard.auto_disabled);

            match result {
                BattleResult::Victory => battle.actors[1].availability = ActorAvailability::Dead,
                BattleResult::Defeat => battle.actors[0].availability = ActorAvailability::Dead,
                BattleResult::Escaped => battle.recognize_escape(true)?,
            }
            assert_eq!(battle.recognize_result(), Some(result));
            assert!(!battle.enemy_conversations.active(ActorId(1)));
            assert!(!battle.actors[1].guard.auto_disabled);
            let frame = battle.step(BattleInput {
                paused: true,
                ..Default::default()
            })?;
            assert_eq!(
                frame.cues,
                vec![
                    Cue::VoiceVolume {
                        actor: ActorId(0),
                        percent: 100
                    },
                    Cue::VoiceVolume {
                        actor: ActorId(1),
                        percent: 100
                    },
                ]
            );
            for _ in 0..5 {
                assert!(battle.step(BattleInput::default())?.cues.is_empty());
            }
        }
        Ok(())
    }

    #[test]
    fn health_exchange_keeps_order_waits_through_pause_and_runs_once() -> Result<()> {
        for timing in [ConversationTiming::Spaced, ConversationTiming::Continuous] {
            let mut definition = conversation(ActorId(2), &[ActorId(2), ActorId(0), ActorId(1)]);
            definition.hp_percent = 50;
            definition.timing = timing;
            definition.lines[1].ticks = 1;
            definition.lines[2].ticks = 0;
            let mut battle = battle(&[Side::Party, Side::Party, Side::Enemy], vec![definition])?;
            battle.actors[2].hp = 100;
            assert!(battle.step(BattleInput::default())?.cues.is_empty());
            battle.actors[2].hp = 50;
            let mut volumes = [100; 3];
            for cue in battle.step(BattleInput::default())?.cues {
                if let Cue::VoiceVolume { actor, percent } = cue {
                    volumes[actor.index()] = percent;
                }
            }
            assert!(battle.actors[2].guard.auto_disabled);
            for _ in 0..3 {
                assert!(
                    battle
                        .step(BattleInput {
                            paused: true,
                            ..Default::default()
                        })?
                        .cues
                        .is_empty()
                );
            }
            battle.actors[1].hp = 0;
            battle.actors[1].availability = ActorAvailability::Dead;
            let mut heard = Vec::new();
            for tick in 0..150 {
                let frame = battle.step(BattleInput::default())?;
                for cue in frame.cues {
                    if let Cue::VoiceVolume { actor, percent } = cue {
                        volumes[actor.index()] = percent;
                    }
                    if let Cue::Voice {
                        actor,
                        sound,
                        priority,
                        centered,
                        ..
                    } = cue
                    {
                        assert_eq!(priority, VoicePriority::Announcement);
                        assert!(centered);
                        heard.push((tick, actor, sound));
                    }
                }
                if tick == 0 {
                    assert_eq!(
                        volumes,
                        if timing == ConversationTiming::Spaced {
                            [60, 60, 100]
                        } else {
                            [50, 50, 100]
                        }
                    );
                    // Healing and crossing the threshold again cannot restart speech.
                    battle.actors[2].hp = 100;
                }
                if tick == 110 {
                    battle.actors[2].hp = 50;
                }
                if timing == ConversationTiming::Continuous && tick == 4 {
                    assert!(!battle.enemy_conversations.active(ActorId(2)));
                    assert!(!battle.actors[2].guard.auto_disabled);
                    assert_eq!(volumes, [100; 3]);
                }
                assert_eq!(battle.recognize_result(), None);
            }
            assert_eq!(
                heard,
                vec![
                    (0, ActorId(2), Sound::Stream(7)),
                    (
                        if timing == ConversationTiming::Spaced {
                            32
                        } else {
                            2
                        },
                        ActorId(0),
                        Sound::Stream(8)
                    ),
                ]
            );
            assert!(!battle.actors[2].guard.auto_disabled);
            assert_eq!(volumes, [100; 3]);
        }
        Ok(())
    }
}
