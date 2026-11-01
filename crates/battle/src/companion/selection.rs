//! Autonomous selection. This module cannot mutate a Battle: its only mutable
//! input is the explicitly supplied gameplay random stream.
use super::{NORMAL_OPENING_MIN_HP_PERCENT, PositionStrategy, ResolvedPolicy};
use crate::Character::{Colette, Kratos, Lloyd, Sheena, Zelos};
use crate::NormalAttack::{Finisher, Low, Neutral, Rising, Thrust};
use crate::action_selection::BufferedAction;
use crate::conditions::Cure;
use crate::control::NormalState::{self, Attack};
use crate::{ActionRequest, Actor, ActorId, NormalAttack, PreparedTechnique, Random};
use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub(super) enum OpeningAction {
    None,
    Formation,
    Action {
        opponent: ActorId,
        request: ActionRequest,
    },
}
#[derive(Debug, Clone, Copy)]
pub(super) struct OpeningDecision {
    pub target_visits: [Option<ActorId>; 2],
    pub action: OpeningAction,
}

pub(super) struct Selection<'a, 'r> {
    pub battle: &'a crate::Battle,
    pub owner: ActorId,
    pub policy: ResolvedPolicy,
    pub random: &'r mut Random,
}

impl Selection<'_, '_> {
    fn request(&self, action: crate::ActionKey, target: ActorId) -> ActionRequest {
        ActionRequest {
            actor: self.owner,
            action,
            target,
        }
    }

    fn allies(&self) -> impl Iterator<Item = (ActorId, &Actor)> + Clone {
        let side = self.battle.actors[self.owner.index()].side;
        self.battle
            .actors
            .iter()
            .enumerate()
            .filter(move |(_, actor)| actor.side == side)
            .map(|(index, actor)| (ActorId(index as u8), actor))
    }

    pub(super) fn techniques(&self) -> impl Iterator<Item = &PreparedTechnique> {
        self.battle.prepared.actor_setup[self.owner.index()]
            .techniques
            .as_slice()
            .iter()
            .filter(|row| {
                self.battle.prepared.special_guard(self.owner) != Some(row.action)
                    && self.battle.technique_ready(self.owner, row.action)
                    && self.battle.technique_enabled(self.owner, row.action)
                    && (self.policy.character != Sheena
                        || !self.battle.actors[self.owner.index()].overlimit.is_active()
                        || row.capabilities.summon)
            })
    }

    pub(super) fn opening(&mut self) -> Result<OpeningDecision> {
        let battle = self.battle;
        let owner = self.owner;
        let strategy = self.policy;
        let mut decision = OpeningDecision {
            target_visits: [None; 2],
            action: OpeningAction::None,
        };
        if strategy.position == PositionStrategy::HoldPosition {
            decision.action = OpeningAction::Formation;
            return Ok(decision);
        }
        let mut targets = battle.target_roster();
        let Some(first) =
            crate::target::select_target(&targets, owner, strategy.target, &mut || {
                self.random.next_u16()
            })?
        else {
            return Ok(decision);
        };
        decision.target_visits[0] = Some(first);
        targets[owner.index()].target = Some(first);
        let limits = strategy.limits;
        let support = if strategy.idle_techniques {
            self.revival()
                .or_else(|| self.healing(limits.healing_threshold_percent))
                .or_else(|| self.condition_category(Some(Cure::Physical)))
                .or_else(|| self.condition_category(Some(Cure::AntiMagic)))
                .or_else(|| {
                    if battle.prepared.companion_level_advantage <= limits.buff_max_level_advantage
                    {
                        self.condition_category(None)
                    } else {
                        None
                    }
                })
        } else {
            None
        };
        let action = match support {
            Some(action) => Some(action),
            None if self.prefers_normal_opening() => None,
            None => self.offensive_opening_choice(first),
        };
        if let Some(request) = action {
            decision.action = OpeningAction::Action {
                opponent: first,
                request,
            };
            return Ok(decision);
        }
        // Normal fallback and ranged waiting perform their own target visit.
        let Some(opponent) =
            crate::target::select_target(&targets, owner, strategy.target, &mut || {
                self.random.next_u16()
            })?
        else {
            return Ok(decision);
        };
        decision.target_visits[1] = Some(opponent);
        decision.action = if strategy.position.is_ranged()
            && self.battle.actors[self.owner.index()].tp_percent()
                > i16::from(limits.idle_tp_reserve_percent)
            && self.techniques().next().is_some()
            && battle
                .actors
                .iter()
                .filter(|actor| actor.side == crate::Side::Party && actor.available())
                .count()
                > 1
        {
            OpeningAction::Formation
        } else {
            OpeningAction::Action {
                opponent,
                request: self.normal_choice(opponent),
            }
        };
        Ok(decision)
    }

    pub(super) fn combo(&mut self, normal: Option<NormalState>) -> Option<BufferedAction> {
        let battle = self.battle;
        let owner = self.owner;
        let target = battle.runtime[owner.index()].target;
        if let Some(action) = self.offensive_combo_choice(target, normal)
            && matches!(
                self.battle.prepared.actions[action.action].execution,
                crate::ActionExecution::Attack(_)
            )
            && battle.chain_technique_available(owner, action.action)
            && battle
                .action_candidate(
                    owner,
                    action.action,
                    battle.selection_range(owner, action.action),
                )
                .is_ok()
        {
            return Some(BufferedAction::Technique(action.action));
        }
        let normal = normal?;
        let combo = battle.runtime[owner.index()].combo;
        let control = battle.prepared.actor_setup[owner.index()]
            .control
            .as_deref()
            .unwrap();
        let equipment = &battle.actors[self.owner.index()].equipment;
        if combo.normal_links + 1 >= equipment.normal_combo_limit
            || normal.airborne() && !equipment.combo_traits.sky_combo
        {
            return None;
        }
        let continuation = normal.continuations(control, equipment.combo_traits.jump_combo);
        let neutral = matches!(normal, Attack(Neutral));
        let mut selected = loop {
            let choice = NormalAttack::ALL[usize::from(self.random.next_u16()) % 5];
            if choice != Rising
                || self.battle.actors[target.index()].position[1]
                    >= self.policy.normal_selection.airborne_target_height
            {
                break choice;
            }
        };
        if self.policy.character == Colette && selected != Rising && neutral {
            selected = Finisher;
        }
        let next = if continuation.directions & (1 << selected as u8) != 0 {
            selected
        } else {
            let fallback = continuation.fallback?;
            if self.policy.character == Lloyd && neutral && fallback == Finisher {
                Low
            } else {
                fallback
            }
        };
        let action = control.normals[next as usize].action;
        (combo.history.is_empty()
            && battle
                .action_candidate(
                    self.owner,
                    action,
                    battle.selection_range(self.owner, action),
                )
                .is_ok())
        .then_some(BufferedAction::Normal(next))
    }

    fn revival(&mut self) -> Option<ActionRequest> {
        let fallen: Vec<_> = self
            .allies()
            .filter(|(_, actor)| actor.availability == crate::ActorAvailability::Dead)
            .map(|(id, _)| id)
            .collect();
        let sampled = sample(&fallen, self.random)?;
        let target = self
            .battle
            .preferred_party_leader()
            .filter(|leader| fallen.contains(leader))
            .unwrap_or(sampled);
        let choices: Vec<_> = self
            .techniques()
            .filter(|row| row.capabilities.revives && row.capabilities.ai_target_scope.single())
            .map(|row| self.request(row.action, target))
            .collect();
        sample(&choices, self.random)
    }

    fn healing(&mut self, threshold: u8) -> Option<ActionRequest> {
        use crate::SpellBenefit::{Hp, HpByDistance};
        let wounded = self
            .allies()
            .filter(|(_, actor)| actor.available() && actor.hp_percent() <= i16::from(threshold));
        let needs_group = wounded.clone().count() >= 2;
        let target = wounded
            .min_by_key(|(index, actor)| (actor.hp_percent(), std::cmp::Reverse(index.index())))
            .map(|(id, _)| id)?;
        let mut choices: Vec<_> =
            self.techniques()
                .filter(|row| {
                    row.capabilities.healing
                        && self
                            .battle
                            .prepared
                            .casting(row.action)
                            .is_some_and(|cast| {
                                cast.release.recovery.iter().any(|pulse| {
                                    matches!(pulse.benefit, Hp(_) | HpByDistance { .. })
                                })
                            })
                })
                .copied()
                .collect();
        let group = needs_group
            && choices
                .iter()
                .any(|row| row.capabilities.ai_target_scope.group());
        choices.retain(|row| {
            let scope = row.capabilities.ai_target_scope;
            if group { scope.group() } else { scope.single() }
        });
        let choice = sample(&choices, self.random)?;
        Some(self.request(choice.action, target))
    }

    fn condition_category(&mut self, category: Option<Cure>) -> Option<ActionRequest> {
        let battle = self.battle;
        use crate::TechniqueTarget;
        let owner = self.owner;
        let side = self.battle.actors[self.owner.index()].side;
        let targets: Vec<_> = self
            .allies()
            .filter(|(_, actor)| match category {
                Some(cure) => actor.conditions.needs_cure(cure),
                None => actor.available(),
            })
            .map(|(id, _)| id)
            .collect();
        let target = sample(&targets, self.random)?;
        let living = self.allies().filter(|(_, actor)| actor.available()).count();
        for group in [true, false] {
            if group && (category.is_some() || living < 2) {
                continue;
            }
            let choices: Vec<_> = self
                .techniques()
                .filter(|row| {
                    matches!(
                        row.capabilities.target,
                        TechniqueTarget::Ally | TechniqueTarget::SelfTarget
                    ) && if group {
                        row.capabilities.ai_target_scope.group()
                    } else {
                        row.capabilities.ai_target_scope.single()
                    }
                })
                .filter(|row| {
                    let Some(cast) = self.battle.prepared.casting(row.action) else {
                        return false;
                    };
                    cast.release.recovery.iter().any(|pulse| {
                        let needs = |actor: &crate::Actor| {
                            condition_benefit_needed(actor, side, category, pulse.benefit)
                        };
                        if row.capabilities.target == TechniqueTarget::SelfTarget
                            || pulse.targets == crate::RecoveryTargets::Caster
                        {
                            needs(&battle.actors[owner.index()])
                        } else if group {
                            battle.actors.iter().any(needs)
                        } else {
                            needs(&battle.actors[target.index()])
                        }
                    })
                })
                .copied()
                .collect();
            if let Some(row) = sample(&choices, self.random) {
                let recipient = if row.capabilities.target == TechniqueTarget::SelfTarget {
                    owner
                } else {
                    target
                };
                return Some(self.request(row.action, recipient));
            }
        }
        None
    }

    pub(super) fn offensive_opening_choice(&mut self, target: ActorId) -> Option<ActionRequest> {
        let battle = self.battle;
        let owner = self.owner;
        let strategy = self.policy;
        let limits = strategy.limits;
        let reserve_reached = strategy.position != PositionStrategy::SkillsAndMagic
            && battle.actors[owner.index()].tp_percent() <= i16::from(limits.tp_reserve_percent);
        if !strategy.idle_techniques || !strategy.reduced_offense && reserve_reached {
            return None;
        }
        let opponent = &battle.actors[target.index()];
        let mut choices: Vec<_> = self
            .offensive_candidates(target)
            .filter(|row| {
                strategy
                    .opening_weapon
                    .is_none_or(|v| v == row.capabilities.uses_weapon_reach)
                    && strategy
                        .opening_spell
                        .is_none_or(|v| v == row.capabilities.spell)
            })
            .copied()
            .collect();
        if opponent.weakness_known
            && self.battle.prepared.companion_level_advantage <= 2
            && choices
                .iter()
                .any(|row| opponent.affinity(row.element) == crate::Affinity::Weak)
        {
            choices.retain(|row| opponent.affinity(row.element) == crate::Affinity::Weak);
        }

        let choice = sample(&choices, self.random)?;
        // Moderation, healing and support policies reduce offensive use below
        // their reserve instead of refusing every affordable technique.
        let sheena_overlimit =
            self.policy.character == Sheena && battle.actors[owner.index()].overlimit.is_active();
        if reserve_reached && !sheena_overlimit && self.random.next_u16() % 100 >= 25 {
            return None;
        }
        let capabilities = choice.capabilities;
        // Ordinary melee techniques remain available at the combo boundary;
        // ranged techniques and spells can also open a new action.
        let allowed = !capabilities.uses_weapon_reach
            || capabilities.spell
            || strategy.position == PositionStrategy::SkillsAndMagic
            || matches!(strategy.character, Kratos | Zelos)
            || sheena_overlimit;
        allowed.then(|| self.request(choice.action, target))
    }

    pub(super) fn offensive_candidates(
        &self,
        target: ActorId,
    ) -> impl Iterator<Item = &PreparedTechnique> {
        let opponent = &self.battle.actors[target.index()];
        self.techniques()
            .filter(|row| row.capabilities.offensive && row.capabilities.ai_target_scope.single())
            .filter(move |row| {
                !opponent.resistance_known
                    || !matches!(
                        opponent.affinity(row.element),
                        crate::Affinity::Absorb | crate::Affinity::Immune
                    )
            })
    }

    pub(super) fn offensive_combo_admitted(&mut self, normal: Option<NormalState>) -> bool {
        let owner = self.owner;
        let strategy = self.policy;
        let actor = &self.battle.actors[owner.index()];
        let low_tp = actor.tp_percent() <= i16::from(strategy.limits.tp_reserve_percent);
        let unrestricted = strategy.position == PositionStrategy::SkillsAndMagic;
        if !unrestricted && !strategy.reduced_offense && low_tp {
            return false;
        }
        let overlimit = strategy.character == Sheena && actor.overlimit.is_active();
        if unrestricted || overlimit {
            return true;
        }
        let throwing =
            strategy.character == Colette && matches!(normal, Some(Attack(Thrust | Low)));
        // Preserve the two throw visits around the ordinary gate: each draw
        // happens only if all earlier paths have rejected the continuation.
        if throwing && (!low_tp || self.random.next_u16() % 100 < 50) {
            return true;
        }
        let last_normal = self.battle.runtime[self.owner.index()].combo.normal_links
            >= actor.equipment.normal_combo_limit.saturating_sub(1);
        let ordinary = if last_normal {
            !low_tp || self.random.next_u16() % 100 < 25
        } else {
            let level = actor.equipment.stats.level;
            level < 30 && self.random.next_u16() % u16::from(level / 2 + 1) == 0
        };
        ordinary || throwing && (!low_tp || self.random.next_u16() % 100 < 50)
    }

    pub(super) fn offensive_combo_choice(
        &mut self,
        target: ActorId,
        normal: Option<NormalState>,
    ) -> Option<ActionRequest> {
        if !self.offensive_combo_admitted(normal) {
            return None;
        }
        let choices: Vec<_> = self
            .offensive_candidates(target)
            .filter(|row| {
                self.battle
                    .chain_descriptor_available(self.owner, row.action)
            })
            .copied()
            .collect();
        // Combo selection has bounded retries for candidates that are spells.
        // The ordinary attack remains committed if all retries reject.
        for _ in 0..(100 * 9) {
            let choice = sample(&choices, self.random)?;
            if !choice.capabilities.spell {
                return Some(self.request(choice.action, target));
            }
        }
        None
    }

    pub(super) fn normal_choice(&mut self, target: ActorId) -> ActionRequest {
        let battle = self.battle;
        let definition = self.battle.prepared.actor_setup[self.owner.index()]
            .control
            .as_deref()
            .unwrap();
        let roll = usize::from(self.random.next_u16()) % 4;
        let selection = [Neutral, Rising, Thrust, Low][roll];
        let selection = if self.policy.character == Colette {
            colette_normal(
                &battle.actors[self.owner.index()],
                &battle.actors[target.index()],
                self.policy,
                selection,
                || battle.body_gap(self.owner, target),
            )
        } else {
            selection
        };
        self.request(definition.normals[selection as usize].action, target)
    }

    pub(super) fn prefers_normal_opening(&self) -> bool {
        let owner = self.owner;
        let strategy = self.policy;
        matches!(self.policy.character, Kratos | Zelos)
            && !strategy.position.is_ranged()
            && strategy.position != PositionStrategy::SkillsAndMagic
            && self.battle.actors[owner.index()].hp_percent() >= NORMAL_OPENING_MIN_HP_PERCENT
    }
}

fn condition_benefit_needed(
    actor: &crate::Actor,
    side: crate::Side,
    category: Option<Cure>,
    benefit: crate::SpellBenefit,
) -> bool {
    use crate::{SpellBenefit, conditions::Cure};
    actor.side == side
        && benefit.accepts(actor)
        && match (category, benefit) {
            (Some(category), SpellBenefit::Cure(cure)) => {
                (cure == category || cure == Cure::All)
                    && actor.conditions.needs_cure(category)
                    && actor.conditions.needs_cure(cure)
            }
            (None, SpellBenefit::Stat { condition, .. }) => {
                !actor.conditions.rejects(condition)
                    && !actor.conditions.effective().contains(condition)
            }
            (None, SpellBenefit::Buff(buff)) => actor.conditions.needs_buff(buff),
            _ => false,
        }
}

/// Approach ticks inspect height first; geometry is evaluated only by Colette's
/// close-range rule after a grounded Rising normal needs reselection.
pub(super) fn approach(
    owner: &Actor,
    target: &Actor,
    policy: ResolvedPolicy,
    previous: NormalAttack,
    random: &mut Random,
    body_gap: impl FnOnce() -> f32,
) -> Option<NormalAttack> {
    let airborne = target.position[1] >= policy.normal_selection.airborne_target_height;
    if airborne && previous != Rising {
        return Some(Rising);
    }
    if airborne || previous != Rising {
        return None;
    }
    let next = loop {
        let next = NormalAttack::ALL[usize::from(random.next_u16()) % 4];
        if next != Rising {
            break next;
        }
    };
    Some(if policy.character == Colette {
        colette_normal(owner, target, policy, next, body_gap)
    } else {
        next
    })
}

fn colette_normal(
    owner: &Actor,
    target: &Actor,
    policy: ResolvedPolicy,
    requested: NormalAttack,
    body_gap: impl FnOnce() -> f32,
) -> NormalAttack {
    use NormalAttack::*;
    let selected = match policy.position {
        PositionStrategy::Frontlines => Neutral,
        PositionStrategy::DontPursue
        | PositionStrategy::LongRangeSkills
        | PositionStrategy::LongRangeMagic => {
            if target.movement.flying {
                Low
            } else {
                Thrust
            }
        }
        _ => requested,
    };
    if target.position[1] < policy.normal_selection.airborne_target_height
        && owner.hp_percent() >= 75
        && body_gap() < policy.normal_selection.colette_melee_gap
    {
        Neutral
    } else {
        selected
    }
}

fn sample<T: Copy>(choices: &[T], random: &mut Random) -> Option<T> {
    (!choices.is_empty()).then(|| choices[usize::from(random.next_u16()) % choices.len()])
}
