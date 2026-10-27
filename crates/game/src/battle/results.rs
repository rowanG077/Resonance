//! Candidate session mutations for one live encounter.
//! The suspended field commits Completed once.
use super::{
    lifecycle::{
        QuickOrder, Request,
        victory_selection::{self, ColetteState, VictoryGroupId},
    },
    party::Character,
    rewards::{EnemyReward, HappinessAward, MaximumVital, RewardModifiers, Rewards},
    victory::Performance,
};
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActorAvailability, ActorId, Battle, BattleOutcome, BattlePhase, BattleResult, Cue, Sound,
    TechniqueId,
    conditions::{ConditionSet, POISON},
};
use resonance_content::{arte::Catalogue, menu_data::MenuData, session::SessionData};
use resonance_events::party::Party;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[cfg(test)]
mod condition_refresh_tests;
mod cooking;
#[cfg(test)]
mod cooking_lifecycle_tests;
#[cfg(test)]
mod equipment_title_tests;
mod escape;
#[cfg(test)]
mod escape_tests;
#[cfg(test)]
pub(crate) mod item_tests;
#[cfg(test)]
mod overlimit_tests;
#[cfg(test)]
mod petrify_tests;
#[cfg(test)]
mod poison_tests;
#[cfg(test)]
mod reward_tests;
#[cfg(test)]
mod setup_tests;
mod strategy;
#[cfg(test)]
mod strategy_tests;
mod tech;
#[cfg(test)]
mod tech_connections_tests;
#[cfg(test)]
mod technique_use_tests;
mod technique_uses;
mod titles;
mod unison;
#[cfg(test)]
mod unison_tests;

#[derive(Debug, Clone, Copy)]
pub struct ResultStyle {
    pub play_music: bool,
    pub celebrate: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedGroup {
    pub id: VictoryGroupId,
    pub leader: Character,
    pub participants: Vec<Character>,
    pub required_leader: Option<Character>,
    pub condition: resonance_content::battle_victory::Condition,
    pub selection: resonance_content::battle_victory::SelectionRule,
    pub voice: Sound,
}

/// One admitted enemy instance. Scanning and rewards share its species identity.
pub struct PreparedEnemy {
    pub actor: ActorId,
    pub level: u8,
    pub grade: i16,
    pub steal: Option<u16>,
    pub steal_chance: u8,
    pub reward: EnemyReward,
}

pub struct Setup {
    /// Saved event flag 0x3FA, immutable while the field is suspended.
    pub devils_arms_unlocked: bool,
    pub enemies: Vec<PreparedEnemy>,
    pub level_difference: i8,
    pub actors: Vec<(ActorId, Character)>,
    /// Prepared capacity is shared by menus and live control; saved membership gates use.
    pub formation: u16,
    pub formation_count: usize,
    pub hunting_symbol: Option<u32>,
    pub style: ResultStyle,
    pub story: u32,
    /// Colette's character state, independent of main story progression.
    pub colette_state: ColetteState,
    /// Event flags 27 and 28, retained while the suspended field cannot mutate.
    pub victory_story_flags: [bool; 2],
    pub groups: Vec<PreparedGroup>,
    pub performances: Vec<Performance>,
    pub postures: Vec<super::victory::PostureBinding>,
    /// Ordinary character voice cues resolved before activation.
    pub victory_voices: BTreeMap<Character, Vec<[Option<Sound>; 2]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultNotice {
    Level {
        character: Character,
        level: u8,
    },
    TpRecovery {
        character: Character,
        amount: u16,
    },
    MaximumVital {
        character: Character,
        vital: MaximumVital,
        amount: u16,
    },
    /// EX33 active-recipient result notices. These remain separate from
    /// ordinary growth and technique notices to retain their display order.
    HappinessExperience {
        character: Character,
        amount: u32,
    },
    HappinessGald {
        character: Character,
        amount: u32,
    },
    Technique {
        character: Character,
        technique: TechniqueId,
    },
    CompoundEx {
        character: Character,
    },
    Title {
        character: Character,
        title: u8,
    },
    Cooking {
        character: Character,
        recipe: u8,
        success: bool,
    },
}

/// Translate reward bonuses into result notices.
fn happiness_notices(
    awards: impl IntoIterator<Item = HappinessAward>,
) -> impl Iterator<Item = ResultNotice> {
    awards
        .into_iter()
        .flat_map(|award| {
            [
                (award.experience != 0).then_some(ResultNotice::HappinessExperience {
                    character: award.character,
                    amount: award.experience,
                }),
                (award.gald != 0).then_some(ResultNotice::HappinessGald {
                    character: award.character,
                    amount: award.gald,
                }),
            ]
        })
        .flatten()
}
#[derive(Debug, Clone)]
pub struct Results {
    pub rewards: Rewards,
    pub grade: i32,
    pub maximum_combo: u16,
    pub combat_ticks: u32,
    pub character_names: [String; 9],
    pub notices: Vec<ResultNotice>,
    /// Selected persistent chef while result cooking is available.
    pub cook_prompt: Option<Character>,
    /// Parallel to rewards.items; computed before inventory is changed.
    pub overflow: Vec<bool>,
}

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub actor: ActorId,
    pub character: Character,
    pub pose: Option<u8>,
    pub group: Option<VictoryGroupId>,
    /// Speech remains available even when the selected motion is missing.
    pub voice: Option<Sound>,
}

/// Presentation owns result pages; the game owns rewards and confirmation.
pub trait Presentation {
    /// Consume the final frame once, after simulation and result transitions.
    fn present(&mut self, _frame: &resonance_battle::BattleFrame) -> Result<()> {
        Ok(())
    }
    fn results_on_last_page(&self) -> bool {
        true
    }
    fn request(&mut self, kind: Request, results: Option<&Results>, battle: &Battle) -> Result<()>;
}

#[derive(Clone)]
pub struct Candidate {
    setup: Arc<Setup>,
    party: Party,
    active_order: QuickOrder,
    /// Shared menu pages see only actors currently present in battle.
    menu_party: Option<Party>,
    menu_actors: Vec<(ActorId, Character)>,
    gameplay_random: resonance_events::GameplayRandom,
    cosmetic_random: resonance_battle::Random,
    session: Arc<SessionData>,
    menus: Arc<MenuData>,
    catalogue: Arc<Catalogue>,
    new_ex_skills: BTreeSet<Character>,
    selection: Option<Selection>,
    performance: Option<super::victory::Playing>,
    results: Option<Results>,
    accepted: bool,
    recorded_escape: bool,
    recorded_ordinary_escape: bool,
    stolen_enemies: BTreeSet<ActorId>,
}

/// Borrowed pending state for checkpoint observation; this does not commit it.
pub struct PendingResults<'a> {
    pub party: &'a Party,
    pub gameplay_random: &'a resonance_events::GameplayRandom,
    pub results: &'a Results,
    pub accepted: bool,
}

pub struct Completed {
    pub party: Party,
    pub gameplay_random: resonance_events::GameplayRandom,
    pub result: BattleResult,
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
        // Scripted returns keep their requested cover until the caller reveals the field.
        let return_fade = if request.transition_white.is_none()
            && world
                .fade
                .as_ref()
                .is_some_and(|fade| fade.alpha(world.tick) as i32 <= 0)
        {
            Some(resonance_events::Fade {
                start_tick: world.tick.checked_add(1).context("field clock overflow")?,
                duration: 20,
                from: 255.,
                to: -1., // Complete the return fade past fully transparent.
                white: false,
            })
        } else {
            None
        };
        let previous_party = world.party.replace(self.party);
        let previous_gameplay_random =
            std::mem::replace(&mut world.gameplay_random, self.gameplay_random);
        if let Err(error) = world.complete_battle(request, outcome) {
            world.party = previous_party;
            world.gameplay_random = previous_gameplay_random;
            anyhow::bail!(error);
        }
        if let Some(fade) = return_fade {
            world.fade = Some(fade);
        }
        Ok(())
    }
}

impl Candidate {
    /// The actual uncommitted persistent party. Observation never synchronizes
    /// live battle vitals or poses, exports a member, or advances result owners.
    pub fn persistent_party(&self) -> &Party {
        &self.party
    }

    pub(in crate::battle) fn refresh_menu_roster(&mut self, battle: &Battle) {
        self.menu_actors = self
            .setup
            .actors
            .iter()
            .copied()
            .filter(|(id, _)| {
                battle.actors()[id.index()].availability
                    != resonance_battle::ActorAvailability::Absent
            })
            .collect();
    }

    fn menu_party(&self) -> &Party {
        self.menu_party.as_ref().unwrap_or(&self.party)
    }

    fn begin_menu(&mut self, battle: &Battle) -> Result<()> {
        self.sync_party(battle)?;
        self.refresh_menu_roster(battle);
        ensure!(
            !self.menu_actors.is_empty(),
            "battle menu has no present party"
        );
        let mut party = self.party.clone();
        party.formation.retain(|character| {
            !self.setup.actors.iter().any(|&(actor, member)| {
                member as u8 == *character
                    && battle.actors()[actor.index()].availability
                        == resonance_battle::ActorAvailability::Absent
            })
        });
        for (slot, &(actor, _)) in self.menu_actors.iter().enumerate() {
            let original = self
                .setup
                .actors
                .iter()
                .position(|&(id, _)| id == actor)
                .unwrap();
            party.settings.battle_controls[slot] = self.party.settings.battle_controls[original];
        }
        self.menu_party = Some(party);
        Ok(())
    }

    /// Page slots may be compacted; saved formation and controller slots never are.
    fn restore_menu_roster(&self, party: &mut Party) {
        let selected = party.settings.battle_controls;
        party.settings.battle_controls = self.party.settings.battle_controls;
        for (slot, &(actor, _)) in self.menu_actors.iter().enumerate() {
            let original = self
                .setup
                .actors
                .iter()
                .position(|&(id, _)| id == actor)
                .unwrap();
            party.settings.battle_controls[original] = selected[slot];
        }
        party.formation.clone_from(&self.party.formation);
    }

    fn publish_menu_edits(&mut self) {
        if let Some(mut party) = self.menu_party.clone() {
            self.restore_menu_roster(&mut party);
            self.party = party;
        }
    }

    pub fn items(&self) -> super::items::Inventory<'_> {
        super::items::Inventory {
            counts: &self.party.items,
            definitions: &self.menus.items,
            recent: &self.party.recent_items,
            roster: &self.menu_actors,
            names: std::array::from_fn(|index| {
                self.party.members[index]
                    .name
                    .as_deref()
                    .unwrap_or(&self.menus.initial_names[index])
            }),
        }
    }

    pub fn equipment_page<'a>(
        &'a self,
        state: &'a crate::menu::equipment::Equipment,
        character: usize,
    ) -> crate::menu::equipment::Page<'a> {
        crate::menu::equipment::Page {
            state,
            party: self.menu_party(),
            session: &self.session,
            data: &self.menus,
            names: &self.menus.initial_names,
            character,
        }
    }

    fn equipment_replacement(
        &self,
        battle: &Battle,
        party: &Party,
        actor_id: ActorId,
        character: Character,
    ) -> Result<resonance_battle::EquipmentReplacement> {
        let member = character.index();
        let live = battle
            .actors()
            .get(actor_id.index())
            .context("equipment actor is absent")?;
        let loadout = super::party::loadout(&self.menus, &party.members[member], character)?;
        let (mut attributes, conditions) =
            loadout.equipment_attributes(&party.members[member], &live.conditions, false)?;
        attributes.stats = loadout.battle_stats(
            &party.formation,
            self.setup.devils_arms_unlocked,
            party.battles.kills[character.index()]
                .saturating_add(battle.ledger().kills[actor_id.index()])
                .min(5_000),
        );
        let current = party.members[member].equipment;
        let previous = self.party.members[member].equipment;
        let attachments_changed = previous[0] != current[0] || previous[5] != current[5];
        Ok(resonance_battle::EquipmentReplacement {
            actor: actor_id,
            attributes,
            conditions,
            equipment: attachments_changed.then_some([current[0], current[5]]),
        })
    }

    pub(in crate::battle) fn begin_equipment(
        &mut self,
        battle: &mut Battle,
        remembered: usize,
    ) -> Result<(crate::menu::equipment::Equipment, usize)> {
        self.begin_menu(battle)?;
        let party = self.menu_party.as_mut().unwrap();
        party.formation = self
            .menu_actors
            .iter()
            .map(|&(_, member)| member as u8)
            .collect();
        let count = party.formation.len();
        Ok((
            crate::menu::equipment::Equipment::opening(),
            remembered.min(count - 1),
        ))
    }

    pub(in crate::battle) fn step_equipment(
        &mut self,
        battle: &mut Battle,
        state: &mut crate::menu::equipment::Equipment,
        character: &mut usize,
        input: crate::menu::Input,
    ) -> Result<crate::menu::equipment::Visit> {
        let draft = self
            .menu_party
            .as_mut()
            .context("equipment page has no draft")?;
        let visit = state.step_shared(input, draft, &self.session, &self.menus, character)?;
        if visit.closed {
            let next = self
                .menu_party
                .take()
                .context("equipment page has no draft")?;
            if let Err(error) = self.commit_equipment(battle, next) {
                battle.diagnostics().report("battle equipment", error)?;
            }
        }
        Ok(visit)
    }

    pub(in crate::battle) fn cancel_equipment(&mut self) {
        self.menu_party = None;
    }

    fn commit_equipment(&mut self, battle: &mut Battle, mut next: Party) -> Result<()> {
        self.restore_menu_roster(&mut next);
        let magic_mist = super::escape::magic_mist(
            &next,
            &self.menus,
            self.setup.actors.iter().map(|&(_, character)| character),
        )?;
        let mut replacements = Vec::with_capacity(self.setup.actors.len());
        for &(actor, character) in &self.setup.actors {
            replacements.push(self.equipment_replacement(battle, &next, actor, character)?);
        }
        battle.replace_equipment_batch(replacements)?;
        for &(actor, character) in &self.setup.actors {
            let member = character.index();
            if self.party.members[member].equipment[0] != next.members[member].equipment[0] {
                battle.record_weapon_change(actor);
                if character == Character::Lloyd {
                    next.battles
                        .observe_lloyd_battle_weapon(next.members[member].equipment[0]);
                }
            }
        }
        self.party = next;
        battle.refresh_escape_magic_mist(magic_mist);
        Ok(())
    }

    fn item_character(&self, actor: ActorId) -> Result<Character> {
        self.setup
            .actors
            .iter()
            .find(|&&(id, _)| id == actor)
            .map(|&(_, character)| character)
            .context("item actor has no prepared character")
    }

    fn validate_item_stack(&self, item: u16) -> Result<()> {
        let definition = self
            .session
            .items
            .get(usize::from(item))
            .context("missing battle inventory descriptor")?;
        ensure!(
            self.menus
                .items
                .get(usize::from(item))
                .is_some_and(|row| row.battle_usable),
            "item is not battle usable"
        );
        let count = self.party.items.get(&item).copied().unwrap_or(0);
        ensure!(
            count > 0 && count <= self.party.item_limit(definition),
            "invalid battle item quantity"
        );
        Ok(())
    }

    /// All external assets are prepared before this activation transaction.
    pub fn new(
        setup: Setup,
        actors: &[resonance_battle::Actor],
        mut party: Party,
        gameplay_random: resonance_events::GameplayRandom,
        session: Arc<SessionData>,
        menus: Arc<MenuData>,
        catalogue: Arc<Catalogue>,
    ) -> Result<Self> {
        ensure!(
            (1..=resonance_battle::PARTY_CAPACITY).contains(&setup.actors.len())
                && setup.formation_count > 0
                && !setup.enemies.is_empty()
                && (-8..=8).contains(&setup.level_difference)
                && setup.enemies.iter().all(|enemy| enemy.level != 0),
            "invalid result roster"
        );
        ensure!(
            party.members.len() == 9
                && party
                    .formation
                    .iter()
                    .take(4)
                    .copied()
                    .eq(setup.actors.iter().map(|&(_, id)| id as u8)),
            "result party differs from active roster"
        );
        for (slot, enemy) in setup.enemies.iter().enumerate() {
            ensure!(
                usize::from(enemy.reward.monster) < resonance_content::monster::MONSTER_COUNT
                    && !setup.actors.iter().any(|&(id, _)| id == enemy.actor)
                    && !setup.enemies[..slot]
                        .iter()
                        .any(|other| other.actor == enemy.actor),
                "invalid battle monster identity"
            );
        }
        let mut activation = Vec::with_capacity(setup.actors.len());
        for (slot, &(actor, character)) in setup.actors.iter().enumerate() {
            ensure!(
                !setup.actors[..slot]
                    .iter()
                    .any(|&(other, id)| other == actor || id == character),
                "victory character is not prepared"
            );
            let prepared = actors
                .get(actor.index())
                .context("missing prepared result actor")?;
            ensure!(
                prepared.side == resonance_battle::Side::Party,
                "result actor is not a party member"
            );
            activation.push((
                character as u8,
                prepared.equipment.contact.technique_balance,
            ));
        }
        let mut group_ids = std::collections::BTreeSet::new();
        for group in &setup.groups {
            ensure!(
                (1..64).contains(&group.id.0)
                    && group_ids.insert(group.id)
                    && (1..=resonance_battle::PARTY_CAPACITY).contains(&group.participants.len())
                    && group.participants.contains(&group.leader)
                    && group
                        .participants
                        .iter()
                        .enumerate()
                        .all(|(index, character)| {
                            setup.actors.iter().any(|&(_, id)| id == *character)
                                && !group.participants[..index].contains(character)
                        }),
                "invalid prepared victory group"
            );
        }
        // Live conditions were prepared from these requests before activation.
        // Clear only participating members, using character identity rather than slot count.
        for &(_, character) in &setup.actors {
            let member = &mut party.members[character.index()];
            member.queued_buffs.clear();
        }
        party.begin_battle(&activation)?;
        party.battles.previous_formation = Some(setup.formation);
        // Discover active compound skills at battle entry and retain their result notices.
        let mut new_ex_skills = BTreeSet::new();
        for &(_, character) in &setup.actors {
            let index = character.index();
            let member = &mut party.members[index];
            for (row, _) in menus.ex_skills.characters[index].equipped_compounds(&member.ex_skills)
            {
                let row =
                    u8::try_from(row).context("compound EX index exceeds saved representation")?;
                if member.compound_ex_skills.insert(row) {
                    member.recent_compound_ex_skills.insert(row);
                    new_ex_skills.insert(character);
                }
            }
        }
        let menu_actors = setup.actors.clone();
        Ok(Self {
            setup: Arc::new(setup),
            party,
            active_order: QuickOrder::Cancel,
            menu_party: None,
            menu_actors,
            cosmetic_random: resonance_battle::Random::new(!gameplay_random.state()),
            gameplay_random,
            session,
            menus,
            catalogue,
            new_ex_skills,
            selection: None,
            performance: None,
            results: None,
            accepted: false,
            recorded_escape: false,
            recorded_ordinary_escape: false,
            stolen_enemies: BTreeSet::new(),
        })
    }

    pub fn results(&self) -> Option<&Results> {
        self.results.as_ref()
    }
    pub fn pending_results(&self) -> Option<PendingResults<'_>> {
        Some(PendingResults {
            party: &self.party,
            gameplay_random: &self.gameplay_random,
            results: self.results.as_ref()?,
            accepted: self.accepted,
        })
    }
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }
    fn leader(&self, battle: &Battle) -> Result<(ActorId, Character)> {
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
    fn conditions(&self, battle: &Battle) -> Vec<ConditionSet> {
        self.setup
            .actors
            .iter()
            .map(|&(id, _)| battle.actors()[id.index()].conditions.effective())
            .collect()
    }
    fn sync_party(&mut self, battle: &Battle) -> Result<()> {
        let party = self.snapshot_party(battle)?;
        self.party = party;
        Ok(())
    }

    fn snapshot_party(&self, battle: &Battle) -> Result<Party> {
        let mut party = self.party.clone();
        // Once rewards publish, Party owns level-up learning for the next battle.
        if self.results.is_none() {
            Self::project_techniques(battle, &self.setup.actors, &mut party);
        }
        for &(id, character) in &self.setup.actors {
            let actor = battle
                .actors()
                .get(id.index())
                .context("missing result actor")?;
            let index = character.index();
            let member = party
                .members
                .get_mut(index)
                .context("missing result character")?;
            member.hp = u16::try_from(actor.hp).context("invalid persistent HP")?;
            member.tp = actor.tp;
            member.overlimit = actor.overlimit.saved_percent();
            member.ailments = super::conditions::export_ailments(actor.conditions.base());
            for capability in actor
                .equipment
                .recovery
                .lethal
                .equipment
                .into_iter()
                .flatten()
            {
                if let resonance_battle::RescueEquipment::Consumed(slot) = capability {
                    *member
                        .equipment
                        .get_mut(usize::from(slot))
                        .context("consumed equipment slot is invalid")? = 0;
                }
            }
        }
        party.unison_gauge = battle.saved_unison_gauge();
        Ok(party)
    }
    fn construct_rewards(&mut self, battle: &mut Battle) -> Result<()> {
        ensure!(
            self.results.is_none()
                && self.selection.is_some()
                && battle.phase() == BattlePhase::Results,
            "repeated or unselected victory rewards"
        );
        let maximum_combo = battle.ledger().maximum_combo;
        let party = self.snapshot_party(battle)?;
        let enemies: Vec<_> = self
            .setup
            .enemies
            .iter()
            .filter(|enemy| battle.ledger().reward_eligible(enemy.actor))
            .map(|enemy| enemy.reward.clone())
            .collect();
        let super::rewards::Awarded {
            mut party,
            random,
            rewards,
            advancement,
            maximum_vitals,
            happiness,
            overflow,
        } = super::rewards::award(
            party,
            &self.session,
            &self.catalogue,
            &self.menus.titles,
            super::rewards::RewardInput {
                enemies: &enemies,
                formation_count: self.setup.formation_count,
                maximum_combo,
                technical_smash_bonus: battle.ledger().technical_smash_bonus,
                level_difference: self.setup.level_difference,
                modifiers: self.reward_modifiers(battle)?,
            },
            self.gameplay_random,
        )?;
        let mut notices = Vec::new();
        let vitals = self
            .setup
            .actors
            .iter()
            .map(|&(id, character)| {
                let member = &mut party.members[character.index()];
                let loadout = super::party::loadout(&self.menus, member, character)?;
                let max_hp = u16::try_from(loadout.attributes.max_hp)?;
                let max_tp = loadout.attributes.max_tp;
                ensure!(
                    max_hp > 0 && member.hp <= max_hp && member.tp <= max_tp,
                    "invalid rewarded actor vitals"
                );
                if let Some(tp) = super::rewards::recover_tp_for_actor(
                    member.tp,
                    max_tp,
                    if loadout.spirit_healer { 5 } else { 0 },
                    battle.actors()[id.index()].availability,
                    i32::from(member.hp),
                ) {
                    let amount = tp - member.tp;
                    if amount != 0 {
                        notices.push(ResultNotice::TpRecovery { character, amount });
                    }
                    member.tp = tp;
                }
                Ok((id, member.hp, max_hp, member.tp, max_tp))
            })
            .collect::<Result<Vec<_>>>()?;
        let conditions = self.conditions(battle);
        let grades: Vec<_> = self.setup.enemies.iter().map(|enemy| enemy.grade).collect();
        let grade = battle.finalize_grade(&conditions, &grades, self.setup.level_difference)?;
        let multiplier = super::rewards::combined_multiplier_halves(self.setup.formation_count)?;
        let grade = i32::try_from(i64::from(grade) * i64::from(multiplier) / 2)
            .context("combined Grade reward overflow")?;
        let grade = if party
            .new_game_plus
            .benefits
            .contains(&resonance_content::grade::Benefit::IncreasedGrade)
        {
            grade.checked_mul(2).context("Grade Shop reward overflow")?
        } else {
            grade
        };
        battle.normalize_result_overlimit()?;
        for &(id, character) in &self.setup.actors {
            party.members[character.index()].overlimit =
                battle.actors()[id.index()].overlimit.saved_percent();
        }
        for (id, hp, maximum_hp, tp, maximum_tp) in vitals {
            battle.set_actor_vitals(id, i32::from(hp), i32::from(maximum_hp), tp, maximum_tp)?;
        }
        notices.extend(
            maximum_vitals
                .into_iter()
                .map(|award| ResultNotice::MaximumVital {
                    character: award.character,
                    vital: award.vital,
                    amount: award.amount,
                })
                .chain(happiness_notices(happiness))
                .chain(advancement.iter().flat_map(|row| {
                    row.notices
                        .iter()
                        .map(move |&technique| ResultNotice::Technique {
                            character: row.character,
                            technique: TechniqueId(technique),
                        })
                })),
        );
        for row in &advancement {
            if row.levels != 0 {
                notices.push(ResultNotice::Level {
                    character: row.character,
                    level: party.members[row.character.index()].level,
                });
            }
        }
        if party.settings.preferences.battle_rank >= 1 {
            party.battles.hard_victories =
                party.battles.hard_victories.saturating_add(1).min(9_999);
        }
        let presea_leads = self.setup.actors.first().is_some_and(|&(id, character)| {
            character == Character::Presea
                && battle.actors()[id.index()].control != resonance_battle::Control::Auto
        });
        party
            .battles
            .record_hunt(self.setup.hunting_symbol.filter(|_| presea_leads));
        notices.extend(titles::award(
            &self.menus.title_awards,
            &mut party,
            &self.setup.actors,
            battle.actors(),
            battle.ledger(),
            self.setup.formation,
            self.setup.level_difference,
        ));
        for &(_, character) in &self.setup.actors {
            if self.new_ex_skills.contains(&character) {
                notices.push(ResultNotice::CompoundEx { character });
            }
        }
        party.battles.add_grade(grade);
        party.grade_hundredths = party
            .grade_hundredths
            .saturating_add_signed(grade)
            .min(resonance_content::grade::MAX_GRADE);
        party.battles.maximum_combo = party.battles.maximum_combo.max(maximum_combo);
        let results = Results {
            rewards,
            grade,
            maximum_combo,
            combat_ticks: battle.ledger().combat_ticks,
            character_names: std::array::from_fn(|index| {
                party.members[index]
                    .name
                    .clone()
                    .unwrap_or_else(|| self.menus.initial_names[index].clone())
            }),
            notices,
            cook_prompt: party
                .cooking_chef(&self.menus)
                .ok()
                .and_then(|id| Character::try_from(id).ok()),
            overflow,
        };
        self.party = party;
        self.gameplay_random = random;
        self.results = Some(results);
        Ok(())
    }
    /// Equipment bonuses apply to admitted members; global skill bonuses require available actors.
    fn reward_modifiers(&self, battle: &Battle) -> Result<RewardModifiers> {
        self.reward_modifiers_with(|actor_id| {
            battle
                .actors()
                .get(actor_id.index())
                .map_or((false, 0, 1), |actor| {
                    (actor.available(), actor.hp, actor.equipment.max_hp)
                })
        })
    }

    fn reward_modifiers_with(
        &self,
        mut actor_state: impl FnMut(ActorId) -> (bool, i32, i32),
    ) -> Result<RewardModifiers> {
        let mut modifiers = RewardModifiers::default();
        // Read Raine's luck independently; recipient availability determines bonus eligibility.
        let raine = Character::Raine.index();
        if super::party::happiness(&self.party.members[raine]) {
            let luck = self.party.members[raine].stats_for(&self.menus, raine).luck;
            modifiers.happiness_recipient_luck = Some(luck);
        }
        let mut experience_plus = false;
        for &(actor_id, character) in &self.setup.actors {
            let index = character.index();
            let member = &self.party.members[index];
            // Committed gear contributes even when its admitted owner is knocked out.
            let loadout = super::party::loadout(&self.menus, member, character)?;
            let gear = &loadout.gear;
            modifiers.happiness_recipients[index] = loadout.happiness;
            modifiers.equipment_experience_percent[index] = gear.experience_percent;
            modifiers.gald_one_and_a_half |= gear.gald_one_and_a_half;
            modifiers.gald_double |= gear.gald_double;
            let (available, hp, maximum_hp) = actor_state(actor_id);
            if character == Character::Raine && loadout.happiness && available {
                // The global drop query remains availability-gated.
                modifiers.happiness_luck = modifiers.happiness_recipient_luck;
            }
            if available {
                modifiers.maximum_vital_growth[index] = loadout.maximum_vital_growth;
                if loadout.item_finder {
                    modifiers.item_drop_bonus = 10;
                }
                modifiers.gald_finder |= loadout.gald_finder;
                experience_plus |= loadout.experience_plus;
            }
            modifiers.experience_bonus_percent[index] = super::rewards::experience_bonus_percent(
                loadout.increase_experience,
                loadout.tough_experience,
                hp,
                maximum_hp,
            );
        }
        if experience_plus {
            for &(_, character) in &self.setup.actors {
                modifiers.experience_bonus_percent[character.index()] = 10;
            }
        }
        use resonance_events::battle::Rules;
        let rules = self.party.battle_rules.modifiers;
        modifiers.floor_experience_double = rules & Rules::EXPERIENCE_DOUBLE != 0;
        modifiers.floor_gald_double = rules & Rules::GALD_DOUBLE != 0;
        if rules & Rules::ITEM_DROP_BONUS != 0 {
            modifiers.item_drop_bonus += 25;
        }
        Ok(modifiers)
    }

    fn perform(&mut self, battle: &mut Battle) -> Result<()> {
        let selection = self
            .selection
            .as_ref()
            .context("victory performance has no selection")?;
        if let Some(pose) = selection.pose {
            let performance = self
                .setup
                .performances
                .iter()
                .find(|row| row.character == selection.character && row.selector == pose)
                .context("missing selected performance")?;
            if selection.character != Character::Colette
                || self.setup.colette_state != ColetteState::Sealed
            {
                self.performance = Some(super::victory::Playing::start(
                    battle,
                    selection.actor,
                    performance,
                )?);
            }
        }
        if let Some(voice) = selection.voice {
            battle.request_result_voice(selection.actor, voice)?;
        }
        Ok(())
    }

    /// Consuming the candidate is the one persistent commit boundary. The
    /// lifecycle must already have returned the exact live core outcome.
    pub fn finish(mut self, battle: &Battle, outcome: &BattleOutcome) -> Result<Completed> {
        ensure!(
            !battle.is_diagnostic(),
            "diagnostic battle cannot commit its candidate"
        );
        ensure!(
            battle.owns_outcome(outcome),
            "result candidate does not match completed encounter"
        );
        match outcome.result {
            BattleResult::Victory => ensure!(
                self.results.is_some() && self.accepted,
                "victory lifecycle is incomplete"
            ),
            BattleResult::Escaped
                if battle.departure_reason()
                    != Some(resonance_battle::DepartureReason::EnemyRetreat) =>
            {
                ensure!(self.recorded_escape, "escape lifecycle is incomplete");
                ensure!(
                    battle.ledger().ordinary_escape.is_none() || self.recorded_ordinary_escape,
                    "ordinary escape history is incomplete"
                )
            }
            BattleResult::Escaped | BattleResult::Defeat => {}
        }
        self.sync_party(battle)?;
        if outcome.result != BattleResult::Victory {
            self.party.battles.record_hunt(None);
        }
        let ledger = battle.ledger();
        self.party.battles.combat_ticks = ledger.combat_ticks;
        self.party.battles.formation_count = u32::try_from(self.setup.formation_count)?;
        self.party.battles.maximum_combo_damage = self
            .party
            .battles
            .maximum_combo_damage
            .max(ledger.maximum_combo_damage);
        for (slot, &(id, character)) in self.setup.actors.iter().enumerate() {
            let index = character.index();
            self.party.settings.battle_controls[slot] = match battle.actors()[id.index()].control {
                resonance_battle::Control::Manual => 0,
                resonance_battle::Control::SemiAuto => 1,
                resonance_battle::Control::Auto => 2,
                resonance_battle::Control::Enemy => anyhow::bail!("party actor has enemy control"),
            };
            self.party.battles.kills[index] = self.party.battles.kills[index]
                .saturating_add(ledger.kills[id.index()])
                .min(5_000);
            self.party
                .battles
                .record_deaths(index, ledger.deaths[id.index()]);
            self.party.battles.items[index] = self.party.battles.items[index]
                .saturating_add(ledger.items[id.index()])
                .min(250);
        }
        Ok(Completed {
            party: self.party,
            gameplay_random: self.gameplay_random,
            result: outcome.result,
        })
    }
}

impl resonance_battle::item::Provider for Candidate {
    fn steal(&mut self, request: resonance_battle::item::StealRequest) -> Result<Option<String>> {
        self.item_character(request.user)?;
        let enemy = self
            .setup
            .enemies
            .iter()
            .find(|enemy| enemy.actor == request.target)
            .context("steal target has no prepared monster")?;
        ensure!(enemy.steal_chance <= 100, "invalid enemy steal chance");
        if enemy.steal.is_none() && !request.gald {
            return Ok(None);
        }
        if self.stolen_enemies.contains(&request.target)
            || (!request.guaranteed && request.adjusted_roll >= u16::from(enemy.steal_chance))
        {
            return Ok(None);
        }
        let name = enemy
            .steal
            .map(|item| self.menus.item_text(item).map(|text| text.name.clone()))
            .transpose()?;
        let gald = request.gald.then_some(enemy.reward.gald / 10);
        let description = match (name, gald) {
            (Some(name), Some(gald)) => format!("{name} + {gald} Gald"),
            (Some(name), None) => name,
            (None, Some(gald)) => format!("{gald} Gald"),
            (None, None) => unreachable!(),
        };
        // Capacity affects acquisition, not the successful attempt or depletion.
        if let Some(item) = enemy.steal {
            self.party
                .change_item(&self.session, item, 1)
                .map_err(anyhow::Error::msg)?;
        }
        if let Some(gald) = gald {
            self.party.add_gald(gald as i32);
        }
        self.stolen_enemies.insert(request.target);
        self.party
            .monsters
            .entry(enemy.reward.monster)
            .or_default()
            .steal = true;
        Ok(Some(description))
    }

    fn acquire_item(
        &mut self,
        request: resonance_battle::item::Release,
    ) -> Result<resonance_battle::item::ItemLoan<'_>> {
        use resonance_battle::item::ItemLoan;
        use std::collections::btree_map::Entry;
        self.item_character(request.user)?;
        self.item_character(request.target)?;
        self.validate_item_stack(request.item)?;
        let Party { items, battles, .. } = &mut self.party;
        let Entry::Occupied(stack) = items.entry(request.item) else {
            anyhow::bail!("battle item stack disappeared");
        };
        ItemLoan::new(stack, Some(&mut battles.battle_gel_used))
    }

    fn acquire_scan(
        &mut self,
        request: resonance_battle::item::Release,
    ) -> Result<resonance_battle::item::ScanLoan<'_>> {
        use resonance_battle::item::{ItemLoan, ScanLoan};
        use std::collections::btree_map::Entry;
        let user_character = self.item_character(request.user)?;
        let monster = self
            .setup
            .enemies
            .iter()
            .find(|enemy| enemy.actor == request.target)
            .map(|enemy| enemy.reward.monster)
            .context("scan target has no prepared monster")?;
        self.validate_item_stack(request.item)?;
        ensure!(
            self.party
                .monsters
                .get(&monster)
                .is_none_or(|row| row.variant < 16),
            "invalid monster knowledge"
        );
        let Party {
            items, monsters, ..
        } = &mut self.party;
        let Entry::Occupied(stack) = items.entry(request.item) else {
            anyhow::bail!("battle Lens stack disappeared");
        };
        let item = ItemLoan::new(stack, None)?;
        // Acquisition is the release's final fallible operation. Do not add a
        // fallible tail after this first persistent write for a new species.
        let knowledge = monsters.entry(monster).or_default();
        let location = if user_character == super::party::Character::Raine {
            Some(&mut knowledge.location)
        } else {
            None
        };
        Ok(ScanLoan::new(item, &mut knowledge.scanned, location))
    }
}

impl Candidate {
    pub(in crate::battle) fn tech_target_actor(
        &self,
        _battle: &Battle,
        member: usize,
    ) -> Result<ActorId> {
        Self::tech_actor(&self.setup, member)
    }
    pub fn world_update(
        &mut self,
        battle: &mut Battle,
        input: resonance_battle::BattleInput,
    ) -> Result<Vec<Cue>> {
        let advance_performance = !input.paused;
        let cues = battle.update(input, self)?;
        if advance_performance
            && battle.phase() == BattlePhase::Results
            && let Some(performance) = &mut self.performance
        {
            performance.advance(battle)?;
        }
        self.sync_escape_history(battle)?;
        Ok(cues)
    }
    pub(in crate::battle) fn result_style(&self) -> ResultStyle {
        self.setup.style
    }

    pub(crate) fn choose_victory(&mut self, battle: &mut Battle) -> Result<()> {
        let mut random = self.cosmetic_random;
        let selection =
            victory_selection::select(&self.victory_context(battle)?, || random.next_u16());
        self.cosmetic_random = random;
        self.select_victory(battle, selection)
    }

    pub fn victory_context(&self, battle: &Battle) -> Result<victory_selection::Context<'_>> {
        use victory_selection::{Context, Participant};
        let candidate = self;
        let affinity = affinity_order(std::array::from_fn(|index| {
            candidate.party.members[index].affinity
        }));
        let party = candidate
            .setup
            .actors
            .iter()
            .map(|&(id, character)| {
                let actor = &battle.actors()[id.index()];
                let rank = affinity.iter().position(|&id| id == character).unwrap_or(8);
                Participant {
                    character,
                    available: actor.available(),
                    dead: actor.availability == ActorAvailability::Dead,
                    hp_percent: actor.hp_percent() as u8,
                    close_to_lloyd: rank == 0,
                    distant_from_lloyd: rank >= 3,
                    participation: u32::from(
                        candidate.party.battles.participation[character.index()],
                    ),
                    poisoned: actor.conditions.effective().intersects(POISON),
                    control: actor.control,
                    kills: u32::from(battle.ledger().kills[id.index()]),
                }
            })
            .collect();
        Ok(Context {
            leader: candidate.leader(battle)?.1,
            formation: candidate.setup.formation,
            party,
            colette: candidate.setup.colette_state,
            presea_recovered: candidate.setup.victory_story_flags[1],
            regal_recovered: candidate.setup.victory_story_flags[0],
            party_was_hit: battle.ledger().party_was_hit,
            enemy_was_scanned: battle.ledger().enemy_was_scanned,
            level_difference: i16::from(candidate.setup.level_difference),
            enemy_count: candidate.setup.enemies.len(),
            prepared_groups: if candidate.setup.style.celebrate {
                &candidate.setup.groups
            } else {
                &[]
            },
        })
    }
    pub fn select_victory(
        &mut self,
        battle: &Battle,
        choice: victory_selection::Selection,
    ) -> Result<()> {
        ensure!(
            self.selection.is_none(),
            "invalid repeated victory selection"
        );
        self.selection = Some(match choice {
            victory_selection::Selection::Solo(requested_pose) => {
                let (actor, character) = self.leader(battle)?;
                let can_speak =
                    character != Character::Colette || self.setup.colette_state.can_speak();
                let voice = self
                    .setup
                    .victory_voices
                    .get(&character)
                    .filter(|_| can_speak)
                    .and_then(|choices| {
                        victory_selection::ordinary_voice(
                            character,
                            requested_pose,
                            self.cosmetic_random.next_u16(),
                            choices,
                        )
                    });
                let pose = self
                    .setup
                    .performances
                    .iter()
                    .find(|row| row.character == character && row.selector == requested_pose)
                    .or_else(|| {
                        self.setup
                            .performances
                            .iter()
                            .find(|row| row.character == character)
                    })
                    .map(|row| row.selector);
                Selection {
                    actor,
                    character,
                    pose,
                    group: None,
                    voice,
                }
            }
            victory_selection::Selection::Group(group) => {
                let selected = self
                    .setup
                    .groups
                    .iter()
                    .find(|row| row.id == group)
                    .context("unprepared victory group")?;
                // A dialogue keeps its designated speaker even when they were knocked out.
                let &(actor, character) = self
                    .setup
                    .actors
                    .iter()
                    .find(|&&(_, character)| character == selected.leader)
                    .context("absent group leader")?;
                Selection {
                    actor,
                    character,
                    pose: None,
                    group: Some(group),
                    voice: Some(selected.voice),
                }
            }
        });
        Ok(())
    }

    pub fn prepare_results(&mut self, battle: &mut Battle) -> Result<Vec<Cue>> {
        let mut cues = Vec::new();
        self.construct_rewards(battle)?;
        // Award titles before hiding enemies and arranging the result scene.
        battle.hide_result_enemies()?;
        let selection = self.selection.context("unselected result construction")?;
        battle.arrange_result_actors(selection.actor)?;
        cues.extend(super::victory::construct_result_actors(
            battle,
            selection.actor,
            &self.setup.actors,
            &self.setup.postures,
            self.setup.colette_state,
        )?);
        if self.setup.style.celebrate {
            self.perform(battle)?;
        }
        Ok(cues)
    }

    pub(super) fn result_advancement(&self, age: u16) -> Option<Cue> {
        const LEVEL_FEEDBACK_AGE: u16 = 100;
        const COMPOUND_EX_FEEDBACK_AGE: u16 = 180;
        if !matches!(age, LEVEL_FEEDBACK_AGE | COMPOUND_EX_FEEDBACK_AGE) {
            return None;
        }
        let results = self.results.as_ref()?;
        let actors: Vec<_> = self
            .setup
            .actors
            .iter()
            .filter_map(|&(actor, character)| {
                results
                    .notices
                    .iter()
                    .any(|notice| match *notice {
                        ResultNotice::Level {
                            character: recipient,
                            ..
                        } => age == LEVEL_FEEDBACK_AGE && recipient == character,
                        ResultNotice::CompoundEx {
                            character: recipient,
                        } => age == COMPOUND_EX_FEEDBACK_AGE && recipient == character,
                        _ => false,
                    })
                    .then_some(actor)
            })
            .collect();
        (!actors.is_empty()).then_some(Cue::ResultAdvancement {
            actors,
            level_up: age == LEVEL_FEEDBACK_AGE,
        })
    }

    pub(super) fn update_cooking(&mut self, battle: &mut Battle) -> Result<bool> {
        let before = self
            .results
            .as_ref()
            .context("cooking before rewards")?
            .notices
            .len();
        self.cook(battle)?;
        Ok(self.results.as_ref().unwrap().notices.len() != before)
    }

    pub fn accept_victory(&mut self) -> Result<()> {
        ensure!(!self.accepted, "victory confirmation already accepted");
        ensure!(self.results.is_some(), "victory rewards are incomplete");
        if let Some(group) = self
            .selection
            .context("unselected victory confirmation")?
            .group
        {
            self.party.battles.victory_groups |= 1 << group.0;
        }
        self.accepted = true;
        Ok(())
    }

    pub(super) fn record_escape(&mut self, battle: &Battle) -> Result<()> {
        ensure!(!self.recorded_escape, "escape already recorded");
        self.sync_escape_history(battle)?;
        self.party.battles.record_escape();
        self.recorded_escape = true;
        Ok(())
    }
}

/// Rank Lloyd's companions by affinity.
fn affinity_order(affinities: [i32; 9]) -> [Character; 8] {
    let mut order = std::array::from_fn(|index| Character::ALL[index + 1]);
    order.sort_by_key(|&id| std::cmp::Reverse(affinities[id.index()]));
    order
}

#[cfg(test)]
mod return_tests {
    use super::*;

    #[test]
    fn successful_return_fades_on_field_visits_and_preserves_requested_fades() {
        use resonance_events::{EventRuntime, Fade, GameWorld, ResourceLibrary};
        use symphonia_script::{NativeCall, Program};
        for (retained, transition_white) in [
            None,
            Some((0., false)),
            Some((0.5, false)),
            Some((1.5, false)),
            Some((128., true)),
        ]
        .into_iter()
        .flat_map(|fade| [None, Some(false), Some(true)].map(|clear| (fade, clear)))
        {
            let initialize =
                transition_white.is_none() && retained.is_some_and(|(alpha, _)| alpha < 1.);
            let party: Party = serde_json::from_value(serde_json::json!({
                "members": [], "battles": resonance_events::party::BattleStatistics::default(),
                "formation": [], "items": {}, "found_items": [],
                "recent_items": [], "gald": 500, "spent_gald": 0,
                "settings": {"battle_controls": [1,2,2,2]}
            }))
            .unwrap();
            let mut world = GameWorld::default();
            world.party = Some(party.clone());
            world.tick = 12;
            world.random_state = 123;
            world.next_transition_white = transition_white;
            world.fade = retained.map(|(from, white)| Fade {
                start_tick: 12,
                duration: 20,
                from,
                to: 0.,
                white,
            });
            let old_fade = format!("{:?}", world.fade);
            let mut words = vec![4u16, 0, 0, 0];
            for arg in [1i32, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
                words.extend([
                    0x0200,
                    arg as u16,
                    (arg as u32 >> 16) as u16,
                    0x3000,
                    0x4000,
                ]);
            }
            words.extend([0x2000 | NativeCall::StartBattle as u16, 0x3000, 0x20ff]);
            let mut events = EventRuntime::with_state(
                Arc::new(
                    Program::decode(
                        &words
                            .into_iter()
                            .flat_map(u16::to_be_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ),
                Arc::new(ResourceLibrary::default()),
                world,
                Default::default(),
            )
            .unwrap();
            let request = events.world.battle_request.take().unwrap();
            let mut meal_random = resonance_events::GameplayRandom::default();
            for _ in 0..3 {
                meal_random.next_u32();
            }
            let completed = |gald| {
                let mut result_party = party.clone();
                result_party.gald = gald;
                Completed {
                    party: result_party,
                    gameplay_random: meal_random,
                    result: BattleResult::Victory,
                }
            };
            completed(510).commit(&mut events.world, &request).unwrap();
            assert_eq!(events.tick(), 12);
            assert!(!events.battle_pending());
            assert!(!events.world.input_enabled);
            assert_eq!(events.world.party.as_ref().unwrap().gald, 510);
            assert_eq!(events.world.gameplay_random, meal_random);
            assert_eq!(events.world.random_state, 123);
            if let Some(white) = transition_white {
                let fade = events.world.fade.as_ref().unwrap();
                assert_eq!(fade.alpha(12), 255.);
                assert_eq!(fade.white, white);
                assert_eq!(fade.duration, 0);
            } else if !initialize {
                assert_eq!(format!("{:?}", events.world.fade), old_fade);
            } else {
                let fade = events.world.fade.as_ref().unwrap();
                assert_eq!(fade.alpha(12), 255.);
                assert!(!fade.white);
            }
            let committed_fade = format!("{:?}", events.world.fade);
            assert!(completed(999).commit(&mut events.world, &request).is_err());
            assert_eq!(events.world.party.as_ref().unwrap().gald, 510);
            assert_eq!(format!("{:?}", events.world.fade), committed_fade);
            assert_eq!(events.world.gameplay_random, meal_random);
            assert_eq!(events.world.random_state, 123);
            // Resource holds do not advance the return fade.
            for (visit, expected) in [(1, 242.2), (2, 229.4), (3, 216.6)] {
                events.step().unwrap();
                assert_eq!(events.tick(), 12 + visit);
                if let Some(white) = transition_white {
                    let fade = events.world.fade.as_ref().unwrap();
                    assert_eq!(fade.alpha(events.tick()), 255.);
                    assert_eq!(fade.white, white);
                } else if initialize {
                    let fade = events.world.fade.as_ref().unwrap();
                    assert!((fade.alpha(events.tick()) - expected).abs() < 0.0001);
                } else {
                    assert_eq!(format!("{:?}", events.world.fade), old_fade);
                }
            }
            if initialize {
                let fade = events.world.fade.as_ref().unwrap();
                assert!(fade.alpha(31) > 0.);
                assert_eq!(fade.alpha(32), 0.);
            }
        }
    }
}
