//! Candidate session mutations for one live encounter. Authored encounter tasks
//! select and order operations; the suspended field commits Completed once.
use super::{
    lifecycle::{Acknowledgement, Observations, Request, RequestKind, SelectionQuery, Services},
    rewards::{Advancement, EnemyReward, Rewards},
    victory::Performance,
};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    Actor, ActorAvailability, ActorId, Battle, BattleOutcome, BattlePhase, BattleResult, Cue,
    VoiceLine,
};
use resonance_content::{
    arte::Catalogue, battle_victory::Group, menu_data::MenuData, session::SessionData,
};
use resonance_events::party::Party;
use std::{collections::BTreeMap, sync::Arc};
mod titles;

pub struct Setup {
    pub enemies: Vec<EnemyReward>,
    pub enemy_levels: Vec<u8>,
    pub enemy_grades: Vec<i16>,
    pub actors: Vec<(ActorId, u8)>,
    pub formation: u16,
    pub formation_flags: u8,
    pub story: u32,
    pub intrinsic_conditions: Vec<u64>,
    pub groups: Vec<Group>,
    pub performances: Vec<Performance>,
    pub postures: Vec<super::victory::PostureBinding>,
    pub notice_action: u16,
    /// Source relative command offsets 28..37, resolved before activation.
    pub ordinary_voices: BTreeMap<(u8, u16), VoiceLine>,
    /// Source absolute descriptor commands, resolved before activation.
    pub group_voices: BTreeMap<u8, VoiceLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultNotice {
    Technique { character: u8, technique: u16 },
    CompoundEx { character: u8 },
    Title { character: u8, title: u8 },
}
#[derive(Debug, Clone)]
pub struct Results {
    pub rewards: Rewards,
    pub advancement: Vec<Advancement>,
    pub grade: i32,
    pub maximum_combo: u16,
    pub combat_ticks: u32,
    pub new_ex_skills: Vec<(u8, u8)>,
    pub character_names: [String; 9],
    pub notices: Vec<ResultNotice>,
    /// Parallel to rewards.items; computed before inventory is changed.
    pub overflow: Vec<bool>,
    /// Nominal source TP numbers, populated only by the age-150 operation.
    pub tp_recovery: Vec<(ActorId, u16)>,
}

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub actor: ActorId,
    pub character: u8,
    pub pose: Option<u8>,
    pub group: Option<u8>,
    pub voice: Option<VoiceLine>,
}

/// Presentation acknowledges actual audio/resources/card readiness. The game
/// already selected and started prepared actor controllers and voice requests.
pub trait Presentation {
    fn after_world(&mut self, _frame: &resonance_battle::BattleFrame) -> Result<()> {
        Ok(())
    }
    fn observations(&self) -> Observations;
    fn request(
        &mut self,
        kind: RequestKind,
        selection: Option<&Selection>,
        results: Option<&Results>,
        battle: &mut Battle,
    ) -> Result<Vec<Cue>>;
}

pub struct Candidate {
    setup: Setup,
    party: Party,
    libc_seed: u32,
    session: Arc<SessionData>,
    menus: Arc<MenuData>,
    catalogue: Arc<Catalogue>,
    level_difference: i8,
    new_ex_skills: Vec<(u8, u8)>,
    selection: Option<Selection>,
    results: Option<Results>,
    recovered_tp: bool,
    accepted: bool,
    recorded_escape: bool,
    performed: bool,
}

pub struct Completed {
    pub party: Party,
    pub libc_seed: u32,
    pub result: BattleResult,
    pub battle_random_state: u32,
}

impl Completed {
    /// Publish the candidate and complete the exact suspended native operation.
    /// Fatal defeat retires its field owner instead of calling this transaction.
    pub fn commit(
        self,
        world: &mut resonance_events::GameWorld,
        request: &resonance_events::battle::Request,
    ) -> Result<()> {
        use resonance_events::battle::{DefeatPolicy, Outcome};
        ensure!(
            request.is_pending(),
            "battle caller was cancelled before return"
        );
        let outcome = match self.result {
            BattleResult::Victory => Outcome::Victory,
            BattleResult::Defeat => Outcome::Defeat,
            BattleResult::Escaped => Outcome::Escaped,
        };
        ensure!(
            outcome != Outcome::Defeat || request.setup.defeat == DefeatPolicy::ResumeEvent,
            "fatal defeat cannot resume the field"
        );
        let previous_party = world.party.replace(self.party);
        let previous_random = std::mem::replace(&mut world.random_state, self.libc_seed);
        if let Err(error) = request.complete(outcome) {
            world.party = previous_party;
            world.random_state = previous_random;
            anyhow::bail!(error);
        }
        Ok(())
    }
}

impl Candidate {
    /// All external assets are prepared before this activation transaction.
    pub fn new(
        setup: Setup,
        mut party: Party,
        libc_seed: u32,
        session: Arc<SessionData>,
        menus: Arc<MenuData>,
        catalogue: Arc<Catalogue>,
    ) -> Result<Self> {
        ensure!(
            setup.formation_flags & 0x20 == 0,
            "alternate result camera is not prepared"
        );
        ensure!(
            matches!(setup.formation, 1 | 2),
            "victory selection is not prepared for this formation"
        );
        ensure!(
            (1..=4).contains(&setup.actors.len())
                && setup.actors.len() == setup.intrinsic_conditions.len()
                && !setup.enemies.is_empty()
                && setup.enemies.len() == setup.enemy_levels.len()
                && setup.enemies.len() == setup.enemy_grades.len()
                && setup.enemy_levels.iter().all(|&level| level != 0),
            "invalid result roster"
        );
        ensure!(
            party.members.len() == 9
                && party
                    .formation
                    .iter()
                    .take(4)
                    .copied()
                    .eq(setup.actors.iter().map(|&(_, id)| id)),
            "result party differs from active roster"
        );
        for (slot, &(actor, character)) in setup.actors.iter().enumerate() {
            ensure!(
                (1..=3).contains(&character)
                    && !setup.actors[..slot]
                        .iter()
                        .any(|&(other, id)| other == actor || id == character),
                "victory character is not prepared"
            );
            for selector in 0..5 {
                ensure!(
                    setup
                        .performances
                        .iter()
                        .filter(|row| row.character == character && row.selector == selector)
                        .count()
                        == 1,
                    "victory performance is not prepared"
                );
            }
            for offset in 28..38 {
                ensure!(
                    setup.ordinary_voices.contains_key(&(character, offset)),
                    "victory voice is not prepared"
                );
            }
        }
        for &group in &resonance_content::battle_victory::OPENING_GROUPS {
            ensure!(
                setup.groups.iter().filter(|row| row.id == group).count() == 1
                    && setup.group_voices.contains_key(&group),
                "victory group is not prepared"
            );
        }
        let party_average = setup
            .actors
            .iter()
            .map(|&(_, id)| i32::from(party.members[usize::from(id - 1)].level))
            .sum::<i32>()
            / setup.actors.len() as i32;
        let enemy_average = setup
            .enemy_levels
            .iter()
            .map(|&level| i32::from(level))
            .sum::<i32>()
            / setup.enemy_levels.len() as i32;
        let level_difference = (party_average - enemy_average).clamp(-8, 8) as i8;
        party.begin_battle(
            &menus,
            &setup.actors.iter().map(|&(_, id)| id).collect::<Vec<_>>(),
        )?;
        // 40C8 stores the current formation only after 10A8 reads the previous
        // value. Preparation has completed that selection; this candidate owns
        // the mutation until victory, escape or a resumable defeat commits it.
        party.battles.previous_formation = Some(setup.formation);
        // CEA8 -> 1C8DC learns matched compounds during activation; the
        // per-actor "new compound" flag survives until the result notices.
        let mut new_ex_skills = Vec::new();
        for &(_, character) in &setup.actors {
            let index = usize::from(character - 1);
            let member = &mut party.members[index];
            for (row, compound) in menus.ex_skills.characters[index]
                .compounds
                .iter()
                .enumerate()
            {
                if compound.skill != 0
                    && compound
                        .required
                        .iter()
                        .all(|id| member.ex_skills.contains(id))
                    && member.compound_ex_skills.insert(row as u8)
                {
                    member.recent_compound_ex_skills.insert(row as u8);
                    new_ex_skills.push((character, row as u8));
                }
            }
        }
        Ok(Self {
            setup,
            party,
            libc_seed,
            session,
            menus,
            catalogue,
            level_difference,
            new_ex_skills,
            selection: None,
            results: None,
            recovered_tp: false,
            accepted: false,
            recorded_escape: false,
            performed: false,
        })
    }

    pub fn results(&self) -> Option<&Results> {
        self.results.as_ref()
    }
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }
    pub fn services<'a, P: Presentation>(
        &'a mut self,
        presentation: &'a mut P,
    ) -> CandidateServices<'a, P> {
        CandidateServices {
            candidate: self,
            presentation,
        }
    }

    fn actor<'a>(&self, battle: &'a Battle, character: u8) -> Option<&'a Actor> {
        self.setup
            .actors
            .iter()
            .find(|&&(_, id)| id == character)
            .and_then(|&(id, _)| battle.actors().get(id.index()))
    }
    fn leader(&self, battle: &Battle) -> Result<(ActorId, u8)> {
        battle
            .ledger()
            .last_party_killer
            .and_then(|killer| {
                self.setup.actors.iter().copied().find(|&(actor, _)| {
                    actor == killer && battle.actors()[actor.index()].available()
                })
            })
            .or_else(|| {
                self.setup
                    .actors
                    .iter()
                    .copied()
                    .find(|&(actor, _)| battle.actors()[actor.index()].available())
            })
            .context("victory has no available leader")
    }
    fn conditions(&self, battle: &Battle) -> Vec<u64> {
        self.setup
            .actors
            .iter()
            .zip(&self.setup.intrinsic_conditions)
            .map(|(&(id, character), &intrinsic)| {
                let member = &self.party.members[usize::from(character - 1)];
                let actor = &battle.actors()[id.index()];
                let mut conditions = intrinsic;
                for (persistent, runtime) in [(0x20, 1), (0x40, 2), (0x80, 8), (0x200, 0x80)] {
                    if member.conditions & persistent != 0 {
                        conditions |= runtime;
                    }
                }
                if actor.petrified {
                    conditions |= 0x20;
                }
                conditions
            })
            .collect()
    }
    fn sync_party(&mut self, battle: &Battle) -> Result<()> {
        for &(id, character) in &self.setup.actors {
            let actor = battle
                .actors()
                .get(id.index())
                .context("missing result actor")?;
            let member = &mut self.party.members[usize::from(character - 1)];
            member.hp = u16::try_from(actor.hp).context("invalid persistent HP")?;
            member.tp = actor.tp;
            member.overlimit =
                u8::try_from(actor.overlimit / 10).context("invalid persistent Over Limit")?;
            member.conditions &= !0x8000_0100;
            if actor.hp == 0 {
                member.conditions |= 0x8000_0000;
            }
            if actor.petrified {
                member.conditions |= 0x100;
            }
        }
        Ok(())
    }
    fn construct_rewards(&mut self, battle: &mut Battle) -> Result<()> {
        ensure!(
            self.results.is_none() && self.selection.is_some(),
            "repeated or unselected victory rewards"
        );
        let conditions = self.conditions(battle);
        let grade =
            battle.finalize_grade(&conditions, &self.setup.enemy_grades, self.level_difference)?;
        let maximum_combo = battle.ledger().maximum_combo;
        let rewards = Rewards::roll(
            &self.setup.enemies,
            maximum_combo,
            self.level_difference,
            || u32::from(battle.draw_random()),
        )?;
        let overflow = rewards
            .items
            .iter()
            .map(|award| {
                let cap = self
                    .session
                    .items
                    .get(usize::from(award.item))
                    .context("missing drop item")?
                    .stack_limit;
                Ok(
                    u16::from(*self.party.items.get(&award.item).unwrap_or(&0)) + award.count
                        > u16::from(cap),
                )
            })
            .collect::<Result<_>>()?;
        self.sync_party(battle)?;
        let advancement = rewards.apply(
            &mut self.party,
            &self.session,
            &self.catalogue,
            &self.menus.titles,
            || resonance_events::libc_random(&mut self.libc_seed),
        )?;
        for &(id, character) in &self.setup.actors {
            let member = &self.party.members[usize::from(character - 1)];
            let stats = member.stats_for(&self.menus, usize::from(character - 1));
            battle.set_actor_vitals(
                id,
                i32::from(member.hp),
                i32::from(stats.hp),
                member.tp,
                stats.tp,
            )?;
        }
        let mut notices: Vec<_> = advancement
            .iter()
            .flat_map(|row| {
                row.notices
                    .iter()
                    .map(move |&technique| ResultNotice::Technique {
                        character: row.character,
                        technique,
                    })
            })
            .collect();
        notices.extend(titles::award(
            &mut self.party,
            &self.setup.actors,
            battle.actors(),
            &battle.ledger().title_events,
        ));
        for &(_, character) in &self.setup.actors {
            if self.new_ex_skills.iter().any(|&(id, _)| id == character) {
                notices.push(ResultNotice::CompoundEx { character });
            }
        }
        self.party.battles.add_grade(grade);
        self.party.battles.maximum_combo = self.party.battles.maximum_combo.max(maximum_combo);
        if self.party.settings.preferences.battle_rank >= 1 {
            self.party.battles.hard_victories = self
                .party
                .battles
                .hard_victories
                .saturating_add(1)
                .min(9_999);
        }
        self.results = Some(Results {
            rewards,
            advancement,
            grade: i32::from(grade),
            maximum_combo,
            combat_ticks: battle.ledger().combat_ticks,
            new_ex_skills: self.new_ex_skills.clone(),
            character_names: std::array::from_fn(|index| {
                self.party.members[index]
                    .name
                    .clone()
                    .unwrap_or_else(|| self.menus.rename.initial_names[index].clone())
            }),
            notices,
            overflow,
            tp_recovery: Vec::new(),
        });
        Ok(())
    }

    fn perform(&mut self, battle: &mut Battle) -> Result<Vec<Cue>> {
        ensure!(!self.performed, "victory performance already started");
        let selection = self
            .selection
            .as_mut()
            .context("victory performance has no selection")?;
        let mut cues = Vec::new();
        let voice = if let Some(group) = selection.group {
            *self
                .setup
                .group_voices
                .get(&group)
                .context("missing group voice")?
        } else {
            let pose = selection.pose.context("missing ordinary pose")?;
            let performance = self
                .setup
                .performances
                .iter()
                .find(|row| row.character == selection.character && row.selector == pose)
                .context("missing selected performance")?;
            if selection.character != 2 || self.setup.story != 1000 {
                cues.extend(battle.start_result_action(selection.actor, performance.action)?);
            }
            // Even Colette's fixed voice sides consume this ordinary source draw.
            let mut side = battle.draw_random() & 1;
            let mut voice_pose = u16::from(pose);
            if selection.character == 2 && matches!(pose, 2 | 3) {
                voice_pose = 3;
                side = u16::from(pose - 2);
            }
            *self
                .setup
                .ordinary_voices
                .get(&(selection.character, 28 + voice_pose * 2 + side))
                .context("missing selected voice")?
        };
        battle.request_result_voice(selection.actor, voice)?;
        selection.voice = Some(voice);
        self.performed = true;
        Ok(cues)
    }

    /// Consuming the candidate is the one persistent commit boundary. The
    /// lifecycle must already have returned the exact live core outcome.
    pub fn finish(mut self, battle: &Battle, outcome: &BattleOutcome) -> Result<Completed> {
        ensure!(
            battle.phase() == BattlePhase::Finished
                && battle.snapshot().recognized_result == Some(outcome.result)
                && outcome.actors == battle.actors()
                && outcome.random_state == battle.random_state(),
            "result candidate does not match completed encounter"
        );
        match outcome.result {
            BattleResult::Victory => ensure!(
                self.results.is_some() && self.recovered_tp && self.accepted,
                "victory lifecycle is incomplete"
            ),
            BattleResult::Escaped => {
                ensure!(self.recorded_escape, "escape lifecycle is incomplete")
            }
            BattleResult::Defeat => {}
        }
        self.sync_party(battle)?;
        let ledger = battle.ledger();
        self.party.battles.combat_ticks = ledger.combat_ticks;
        self.party.battles.maximum_combo_damage = self
            .party
            .battles
            .maximum_combo_damage
            .max(ledger.maximum_combo_damage);
        for (slot, &(id, character)) in self.setup.actors.iter().enumerate() {
            let index = usize::from(character - 1);
            self.party.members[index].conditions &= 0xf000_0fe3;
            self.party.settings.battle_controls[slot] = match battle.actors()[id.index()].control {
                resonance_battle::Control::Manual => 0,
                resonance_battle::Control::SemiAuto => 1,
                resonance_battle::Control::Auto => 2,
                resonance_battle::Control::Enemy => anyhow::bail!("party actor has enemy control"),
            };
            self.party.battles.kills[index] = self.party.battles.kills[index]
                .saturating_add(ledger.kills[id.index()])
                .min(5_000);
            self.party.battles.deaths[index] = self.party.battles.deaths[index]
                .saturating_add(ledger.deaths[id.index()])
                .min(250);
            self.party.battles.items[index] = self.party.battles.items[index]
                .saturating_add(ledger.items[id.index()])
                .min(250);
        }
        Ok(Completed {
            party: self.party,
            libc_seed: self.libc_seed,
            result: outcome.result,
            battle_random_state: outcome.random_state,
        })
    }
}

pub struct CandidateServices<'a, P> {
    candidate: &'a mut Candidate,
    presentation: &'a mut P,
}
impl<P: Presentation> Services for CandidateServices<'_, P> {
    fn after_world(&mut self, frame: &resonance_battle::BattleFrame) -> Result<()> {
        self.presentation.after_world(frame)
    }
    fn observations(&self, battle: &Battle) -> Observations {
        let mut state = self.presentation.observations();
        state.suppress_music = self.candidate.setup.formation_flags & 0x10 != 0;
        state.suppress_performance = self.candidate.setup.formation_flags & 0x20 != 0;
        state.has_ex_notice = self
            .candidate
            .results
            .as_ref()
            .is_some_and(|results| !results.new_ex_skills.is_empty());
        state.performance_finished = self.candidate.selection.as_ref().is_some_and(|selection| {
            selection.group.is_none()
                || selection.group.is_some_and(|group| {
                    self.candidate.party.battles.victory_groups & (1 << group) != 0
                })
                || battle
                    .actor_voice_finished(selection.actor)
                    .unwrap_or(false)
        });
        state
    }
    fn selection_query(&self, query: SelectionQuery, battle: &Battle) -> Result<i32> {
        let candidate = &self.candidate;
        Ok(match query {
            SelectionQuery::Leader => i32::from(candidate.leader(battle)?.1),
            SelectionQuery::Available(id) => {
                i32::from(candidate.actor(battle, id).is_some_and(Actor::available))
            }
            SelectionQuery::Dead(id) => i32::from(
                candidate
                    .actor(battle, id)
                    .is_some_and(|actor| actor.availability == ActorAvailability::Dead),
            ),
            SelectionQuery::HpPercent(id) => candidate
                .actor(battle, id)
                .map_or(0, |actor| actor.hp * 100 / actor.max_hp),
            SelectionQuery::AffinityRank(id) => i32::from(affinity_rank(&candidate.party, id)),
            SelectionQuery::Participation(id) => i32::from(
                *candidate
                    .party
                    .battles
                    .participation
                    .get(usize::from(id.checked_sub(1).context("zero character")?))
                    .context("unknown character")?,
            ),
            SelectionQuery::Poisoned(id) => candidate
                .setup
                .actors
                .iter()
                .position(|&(_, character)| character == id)
                .map_or(0, |index| {
                    i32::from(candidate.conditions(battle)[index] & 3 != 0)
                }),
            SelectionQuery::Story => {
                i32::try_from(candidate.setup.story).context("story value exceeds script range")?
            }
            SelectionQuery::PartyWasHit => i32::from(battle.ledger().party_was_hit),
            SelectionQuery::AllHealthy => {
                i32::from(candidate.setup.actors.iter().all(|&(id, _)| {
                    let actor = &battle.actors()[id.index()];
                    actor.available() && actor.hp * 100 / actor.max_hp >= 75
                }))
            }
        })
    }
    fn request(&mut self, request: Request, battle: &mut Battle) -> Result<Acknowledgement> {
        let candidate = &mut self.candidate;
        let mut cues = Vec::new();
        match request.kind {
            RequestKind::SelectVictory { group, pose } => {
                ensure!(
                    candidate.selection.is_none() && pose < 5,
                    "invalid repeated victory selection"
                );
                let (actor, character) = if group == 0 {
                    candidate.leader(battle)?
                } else {
                    let selected = candidate
                        .setup
                        .groups
                        .iter()
                        .find(|row| row.id == group)
                        .context("unprepared victory group")?;
                    *candidate
                        .setup
                        .actors
                        .iter()
                        .find(|&&(actor, character)| {
                            character == selected.character
                                && battle.actors()[actor.index()].available()
                        })
                        .context("unavailable group leader")?
                };
                candidate.selection = Some(Selection {
                    actor,
                    character,
                    pose: (group == 0).then_some(pose),
                    group: (group != 0).then_some(group),
                    voice: None,
                });
            }
            RequestKind::ConstructRewards => {
                candidate.construct_rewards(battle)?;
                // 57718: title awards precede enemy hiding, then result layout.
                battle.hide_result_enemies()?;
                let selection = candidate
                    .selection
                    .context("unselected result construction")?;
                let results = candidate.results.as_ref().unwrap();
                let participants =
                    if let Some(group) = selection.group {
                        let descriptor = &candidate
                            .setup
                            .groups
                            .iter()
                            .find(|row| row.id == group)
                            .context("missing group layout")?
                            .descriptor;
                        descriptor
                            .get(4..4 + usize::from(descriptor[3]))
                            .context("invalid group participant count")?
                            .iter()
                            .map(|&character| {
                                candidate
                                    .setup
                                    .actors
                                    .iter()
                                    .find(|&&(_, id)| id == character)
                                    .map(|&(actor, _)| actor)
                                    .context("group participant is absent from active party")
                            })
                            .collect::<Result<Vec<_>>>()?
                    } else {
                        std::iter::once(selection.actor)
                            .chain(candidate.setup.actors.iter().filter_map(
                                |&(actor, character)| {
                                    (actor != selection.actor
                                        && results.advancement.iter().any(|row| {
                                            row.character == character && row.levels != 0
                                        }))
                                    .then_some(actor)
                                },
                            ))
                            .collect()
                    };
                let party: Vec<_> = candidate
                    .setup
                    .actors
                    .iter()
                    .map(|&(actor, _)| actor)
                    .collect();
                battle.begin_result_camera(participants.len().try_into()?, false)?;
                battle.place_result_actors(&participants, &party)?;
                cues.extend(super::victory::construct_result_actors(
                    battle,
                    selection.actor,
                    &candidate.setup.actors,
                    &candidate.setup.postures,
                    candidate.setup.story,
                )?);
            }
            RequestKind::ResultCamera { age } => battle.advance_result_camera(age)?,
            RequestKind::LevelNotices | RequestKind::ExNotices => {
                let results = candidate
                    .results
                    .as_ref()
                    .context("result notices precede rewards")?;
                for &(actor, character) in &candidate.setup.actors {
                    let selected = if request.kind == RequestKind::LevelNotices {
                        results
                            .advancement
                            .iter()
                            .any(|row| row.character == character && row.levels != 0)
                    } else {
                        results.new_ex_skills.iter().any(|&(id, _)| id == character)
                    };
                    if selected {
                        cues.extend(
                            battle.start_result_resident(actor, candidate.setup.notice_action)?,
                        );
                    }
                }
            }
            RequestKind::RecoverTp => {
                ensure!(!candidate.recovered_tp, "TP recovery already applied");
                let results = candidate
                    .results
                    .as_mut()
                    .context("TP recovery before rewards")?;
                for &(id, character) in &candidate.setup.actors {
                    let actor = &battle.actors()[id.index()];
                    if actor.available() && actor.max_tp != 0 {
                        let (tp, nominal) = super::rewards::recover_tp(actor.tp, actor.max_tp, 0);
                        battle.set_actor_vitals(id, actor.hp, actor.max_hp, tp, actor.max_tp)?;
                        battle.show_recovery(
                            id,
                            resonance_battle::RecoveryKind::Tp,
                            i16::try_from(nominal)
                                .context("TP recovery display exceeds source range")?,
                        )?;
                        candidate.party.members[usize::from(character - 1)].tp = tp;
                        results.tp_recovery.push((id, nominal));
                    }
                }
                candidate.recovered_tp = true;
            }
            RequestKind::PerformVictory => cues.extend(candidate.perform(battle)?),
            RequestKind::AcceptVictory => {
                ensure!(!candidate.accepted, "victory confirmation already accepted");
                if let Some(group) = candidate
                    .selection
                    .context("unselected victory confirmation")?
                    .group
                {
                    candidate.party.battles.victory_groups |= 1 << group;
                }
                candidate.accepted = true;
            }
            RequestKind::RecordEscape => {
                ensure!(!candidate.recorded_escape, "escape already recorded");
                candidate.party.battles.record_escape();
                candidate.recorded_escape = true;
            }
            _ => {}
        }
        cues.extend(self.presentation.request(
            request.kind,
            candidate.selection.as_ref(),
            candidate.results.as_ref(),
            battle,
        )?);
        Ok(request.acknowledge().with_cues(cues))
    }
}

/// 19DC4 swaps at each strict larger comparison; ties are not re-sorted.
fn affinity_rank(party: &Party, character: u8) -> u8 {
    let mut order = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
    for i in 0..9 {
        for j in i + 1..9 {
            if party.members[usize::from(order[i] - 1)].affinity
                < party.members[usize::from(order[j] - 1)].affinity
            {
                order.swap(i, j);
            }
        }
    }
    order
        .into_iter()
        .filter(|&id| id != 1)
        .position(|id| id == character)
        .map_or(8, |index| index as u8)
}
