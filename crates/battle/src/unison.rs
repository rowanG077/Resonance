//! A team attack temporarily owns actor admission and scene placement.
use crate::{
    ActionDefinition, ActionExecution, ActionId, ActionKey, ActionRequest, ActorId, Battle,
    CameraPose, Cue, EffectAppearance, EffectRequest, MotionBinding, Pose, PreparedAttack, Side,
    Sound, TechniqueId,
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, sync::Arc};

const WINDUP_TICKS: u16 = 60;
const FIRST_SELECTION_TICKS: u16 = 90;
const COMBINATION_POSE_TICKS: u16 = 60;
const START_GRADE: i64 = 50;
const RETAIN_GAUGE_PERCENT: u16 = 10;
const CAMERA_MIN_RADIUS: f32 = 1800.;
const CAMERA_BASE_RADIUS: f32 = 1000.;
const CAMERA_DISTANCE_SCALE: f32 = 3.5;
const CAMERA_FOCUS_HEIGHT: f32 = 100.;
const CAMERA_PITCH: f32 = 12.5;

#[cfg(test)]
mod tests;

/// Canonical arte identity used to match combination ingredients. Several
/// learned techniques can share this identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnisonArteId(pub u16);

/// A recipe row is distinct from the arte it produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnisonRecipeId(pub u8);

#[derive(Debug, Clone, Copy)]
pub struct UnisonArte {
    pub action: ActionKey,
    pub native_id: UnisonArteId,
    pub duration: u16,
    /// Distance between the participants' contact bodies.
    pub distance: f32,
    pub altitude: f32,
}

#[derive(Debug, Clone)]
pub struct UnisonMember {
    pub actor: ActorId,
    pub opener: PreparedAttack,
    pub opener_contact_ticks: u16,
    pub windup_motion: MotionBinding,
    pub windup_pose: Pose,
    pub windup_voice: Option<Sound>,
    /// Prepared eligible actions and the independently selected Unison arte.
    pub artes: Vec<UnisonArte>,
    pub selected: Option<TechniqueId>,
    pub short_weapon: bool,
}

impl UnisonMember {
    fn selected_arte(&self, battle: &Battle) -> Option<UnisonArte> {
        let action = battle
            .prepared_technique(self.actor, self.selected?)?
            .action;
        battle.learned_technique(self.actor, action)?;
        self.artes
            .iter()
            .find(|arte| arte.action == action)
            .copied()
    }
}

#[derive(Debug, Clone)]
pub struct UnisonRecipe {
    pub id: UnisonRecipeId,
    pub native_id: UnisonArteId,
    pub duration: u16,
    pub camera_pitch: f32,
    /// Alternatives in authored priority order. Ingredients use canonical arte identities.
    pub ingredients: Vec<Vec<UnisonArteId>>,
    /// Prepared character bindings for this recipe's two ordered roles.
    pub execution: Option<UnisonCombination>,
}

#[derive(Debug, Clone, Copy)]
pub struct UnisonPlacement {
    pub direction: [f32; 3],
    pub distance: f32,
}

#[derive(Debug, Clone)]
pub struct UnisonRole {
    pub actor: ActorId,
    pub action: ActionKey,
    pub placement: Option<UnisonPlacement>,
    pub visible: bool,
    pub temporary_weapons: Vec<Arc<crate::WeaponDefinition>>,
}

#[derive(Debug, Clone)]
pub struct UnisonCombination {
    pub roles: [Vec<UnisonRole>; 2],
    pub color: Option<[u8; 3]>,
    /// Ordered participant pair, its speaking actor, and the prepared voice.
    pub voices: Vec<([ActorId; 2], ActorId, Sound)>,
}

#[derive(Debug, Clone)]
pub struct UnisonPrelude {
    pub first_x: f32,
    pub spacing: f32,
    pub camera: CameraPose,
    pub color: Option<[u8; 3]>,
    pub effect: EffectAppearance,
    pub sound: Sound,
    pub voices: Vec<(ActorId, Sound)>,
}

#[derive(Debug, Clone)]
pub struct UnisonDefinition {
    /// Formation order also determines simultaneous selection priority.
    pub members: Vec<UnisonMember>,
    pub placement: [[f32; 3]; crate::PARTY_CAPACITY],
    pub minimum_distance: f32,
    pub short_weapon_penalty: f32,
    pub restore_height: f32,
    pub immune_targets: BTreeSet<ActorId>,
    pub retain_gauge: bool,
    pub windup_effect: EffectAppearance,
    pub windup_sound: Sound,
    pub windup_color: Option<[u8; 3]>,
    /// None means the catalogue is unavailable, not that no combination exists.
    /// Some must contain the complete ordered recipe catalogue.
    pub recipes: Option<Vec<UnisonRecipe>>,
    pub combination_prelude: Option<UnisonPrelude>,
    pub damage_bonus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnisonInput {
    Activate(ActorId),
    Select { pressed_slots: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnisonPhase {
    Windup,
    Opening,
    Selecting,
    Staging,
    Executing,
    CombinationPose,
    Combined,
    Restoring,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnisonCompletion {
    Cancelled,
    NoCombination,
    RecipeCatalogueUnavailable,
    CombinationPending {
        id: UnisonRecipeId,
        native_id: UnisonArteId,
        actors: Vec<ActorId>,
    },
    CombinationExecuted {
        id: UnisonRecipeId,
        native_id: UnisonArteId,
        actors: Vec<ActorId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnisonCombinationFrame {
    pub id: UnisonRecipeId,
    pub native_id: UnisonArteId,
    pub actors: [ActorId; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnisonFeedback {
    pub generation: u64,
    pub expansion: u8,
    pub alpha: u8,
    pub limit: u8,
    pub remaining: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnisonFrame {
    pub generation: u64,
    pub phase: UnisonPhase,
    pub initiator: ActorId,
    pub target: Option<ActorId>,
    pub elapsed: u16,
    pub blocked_slots: u8,
    pub selected: Vec<ActorId>,
    pub selected_actions: Vec<(ActorId, ActionKey)>,
    pub hidden: Vec<ActorId>,
    pub stage_color: Option<[u8; 3]>,
    pub completion: Option<UnisonCompletion>,
    pub combination: Option<UnisonCombinationFrame>,
    pub feedback: Option<UnisonFeedback>,
}

impl UnisonFrame {
    pub(crate) fn animates(&self, actor: ActorId) -> bool {
        match self.phase {
            UnisonPhase::Windup => actor == self.initiator,
            UnisonPhase::Opening => true,
            UnisonPhase::Executing => self.target == Some(actor) || self.selected.contains(&actor),
            UnisonPhase::CombinationPose => self
                .combination
                .as_ref()
                .is_some_and(|row| row.actors.contains(&actor)),
            UnisonPhase::Combined => {
                self.target == Some(actor)
                    || self
                        .combination
                        .as_ref()
                        .is_some_and(|row| row.actors.contains(&actor))
            }
            UnisonPhase::Selecting | UnisonPhase::Staging | UnisonPhase::Restoring => false,
        }
    }
}

#[derive(Clone, Copy)]
struct Selection {
    slot: usize,
    arte: UnisonArte,
}

struct Placement {
    position: [f32; 3],
    heading: f32,
    facing: [f32; 3],
    target: ActorId,
}

struct Orbit {
    yaw: f32,
    speed: f32,
    pitch: f32,
}

impl Orbit {
    fn new(random: &mut crate::Random, pitch: f32) -> Self {
        let yaw = f32::from(random.next_u16() % 360);
        let speed = loop {
            let speed = (random.next_u16() as i16) % 50;
            if speed != 0 {
                break f32::from(speed) * 0.01;
            }
        };
        Self { yaw, speed, pitch }
    }
}

struct Combined {
    recipe: usize,
    actors: [ActorId; 2],
    roles: [UnisonRole; 2],
    actions: [Option<ActionId>; 2],
}

struct Run {
    initiator: ActorId,
    phase: UnisonPhase,
    elapsed: u16,
    deadline: u16,
    blocked: u8,
    geometric_contact: Option<ActorId>,
    effective_contact: Option<ActorId>,
    opener_action: Option<ActionId>,
    opening_combo_target: Option<ActorId>,
    target: Option<ActorId>,
    selections: Vec<Selection>,
    placements: Vec<Placement>,
    pending_defeat: Option<bool>,
    orbit: Option<Orbit>,
    camera: Option<CameraPose>,
    completion: Option<UnisonCompletion>,
    combined: Option<Combined>,
    color: Option<[u8; 3]>,
    feedback: Option<UnisonFeedback>,
    feedback_request_update: Option<u64>,
}

#[derive(Default)]
pub(crate) struct State {
    definition: Option<Arc<UnisonDefinition>>,
    openers: Vec<ActionKey>,
    run: Option<Run>,
    generation: u64,
    completion: Option<UnisonCompletion>,
    feedback_generation: u64,
}

#[must_use]
pub struct UnisonArteEdit<'a> {
    selected: Option<&'a mut Option<TechniqueId>>,
    value: Option<TechniqueId>,
}
impl UnisonArteEdit<'_> {
    pub fn commit(self) {
        if let Some(selected) = self.selected {
            *selected = self.value;
        }
    }
}

impl State {
    pub(crate) fn release_immunity(&mut self, actor: ActorId) {
        if let Some(definition) = &mut self.definition {
            Arc::make_mut(definition).immune_targets.remove(&actor);
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.run.is_some()
    }

    pub(crate) fn staged(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| !run.placements.is_empty())
    }

    pub(crate) fn holds_world(&self) -> bool {
        self.run.as_ref().is_some_and(|run| {
            !matches!(
                run.phase,
                UnisonPhase::Opening | UnisonPhase::Executing | UnisonPhase::Combined
            )
        })
    }

    pub(crate) fn holds_actor(&self, actor: ActorId) -> bool {
        self.run.as_ref().is_some_and(|run| {
            if run.phase == UnisonPhase::Combined {
                return !run.combined.as_ref().unwrap().actors.contains(&actor);
            }
            run.phase != UnisonPhase::Opening
                && (run.phase != UnisonPhase::Executing
                    || !run.selections.iter().any(|selection| {
                        self.definition.as_ref().unwrap().members[selection.slot].actor == actor
                    }))
        })
    }

    pub(crate) fn owns_commands(&self, actor: ActorId) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.phase != UnisonPhase::Opening || run.initiator == actor)
    }

    pub(crate) fn target(&self) -> Option<ActorId> {
        self.run
            .as_ref()
            .filter(|_| self.staged())
            .and_then(|run| run.target)
    }

    pub(crate) fn holds_opening_combo(&self, actor: ActorId) -> bool {
        self.run.as_ref().is_some_and(|run| {
            run.phase == UnisonPhase::Opening && run.opening_combo_target == Some(actor)
        })
    }

    pub(crate) fn camera(&self) -> Option<CameraPose> {
        self.run.as_ref().and_then(|run| run.camera)
    }

    pub(crate) fn frame(&self, actors: usize) -> Option<UnisonFrame> {
        let run = self.run.as_ref()?;
        let definition = self.definition.as_ref().unwrap();
        let (selected, selected_actions): (Vec<_>, Vec<_>) = run
            .selections
            .iter()
            .map(|selection| {
                let actor = definition.members[selection.slot].actor;
                (actor, (actor, selection.arte.action))
            })
            .unzip();
        let hidden = (0..actors)
            .map(|index| ActorId(index as u8))
            .filter(|actor| {
                self.staged()
                    && run.phase != UnisonPhase::Staging
                    // The two-person pose excludes the target until the attack starts.
                    && (Some(*actor) != run.target || run.phase == UnisonPhase::CombinationPose)
                    && if let Some(combined) = &run.combined {
                        !combined.roles.iter().any(|role| {
                            role.actor == *actor
                                && (run.phase == UnisonPhase::CombinationPose || role.visible)
                        })
                    } else {
                        !selected.contains(actor)
                    }
            })
            .collect();
        Some(UnisonFrame {
            generation: self.generation,
            phase: run.phase,
            initiator: run.initiator,
            target: run.target,
            elapsed: run.elapsed,
            blocked_slots: run.blocked,
            selected,
            selected_actions,
            hidden,
            stage_color: run.color.or_else(|| {
                (run.phase == UnisonPhase::Windup)
                    .then_some(definition.windup_color)
                    .flatten()
            }),
            completion: run.completion.clone(),
            feedback: run.feedback,
            combination: run.combined.as_ref().map(|combined| {
                let recipe = &definition.recipes.as_ref().unwrap()[combined.recipe];
                UnisonCombinationFrame {
                    id: recipe.id,
                    native_id: recipe.native_id,
                    actors: combined.actors,
                }
            }),
        })
    }
}

impl Battle {
    pub fn unison_arte(&self, actor: ActorId) -> Option<TechniqueId> {
        self.unison
            .definition
            .as_ref()?
            .members
            .iter()
            .find(|m| m.actor == actor)?
            .selected
    }
    pub fn prepare_unison_arte(
        &mut self,
        actor: ActorId,
        value: Option<TechniqueId>,
    ) -> Result<UnisonArteEdit<'_>> {
        ensure!(
            self.phase() == crate::BattlePhase::Combat && !self.unison.active(),
            "Unison loadout requires ordinary combat"
        );
        ensure!(
            self.actor(actor)?.side == Side::Party,
            "Unison loadout requires a party member"
        );
        let action = value
            .map(|id| {
                let row = self
                    .prepared_technique(actor, id)
                    .context("Unison arte is not prepared")?;
                ensure!(
                    row.capabilities.target == crate::TechniqueTarget::Enemy,
                    "Arte is unavailable for Unison"
                );
                self.learned_technique(actor, row.action)
                    .context("Unison arte is not learned")?;
                Ok::<_, anyhow::Error>(row.action)
            })
            .transpose()?;
        let Some(definition) = self.unison.definition.as_mut() else {
            // A solo encounter has no live team-attack owner. The menu can
            // still save the character's loadout for the next full party.
            return Ok(UnisonArteEdit {
                selected: None,
                value,
            });
        };
        let member = Arc::make_mut(definition)
            .members
            .iter_mut()
            .find(|m| m.actor == actor)
            .context("Character cannot join Unison")?;
        ensure!(
            action.is_none_or(|a| member.artes.iter().any(|arte| arte.action == a)),
            "Arte is unavailable for Unison"
        );
        Ok(UnisonArteEdit {
            selected: Some(&mut member.selected),
            value,
        })
    }
    fn place_unison_actor(
        &mut self,
        owner: ActorId,
        target: ActorId,
        direction: [f32; 3],
        distance: f32,
        altitude: f32,
    ) -> Result<()> {
        let direction = crate::distance::normalize(direction);
        let center = self.actors[target.index()].position;
        let actor = &mut self.actors[owner.index()];
        actor.position = [
            center[0] + direction[0] * distance,
            altitude,
            center[2] + direction[2] * distance,
        ];
        actor.facing_direction = [-direction[0], 0., -direction[2]];
        actor.heading = actor.facing_direction[0]
            .atan2(actor.facing_direction[2])
            .to_degrees();
        actor.movement.direction = actor.facing_direction;
        // Sample the retained pose at this placement before binding the next action.
        let gap = match &self.models {
            Some(models) => models
                .body_gap(
                    owner,
                    &self.actors[owner.index()],
                    target,
                    &self.actors[target.index()],
                )
                .context("Unison placement needs prepared body spheres")?,
            None => self.body_gap(owner, target),
        };
        let correction = distance - gap;
        self.actors[owner.index()].position[0] += direction[0] * correction;
        self.actors[owner.index()].position[2] += direction[2] * correction;
        Ok(())
    }

    /// Install already prepared artwork and action bindings before receiving battle input.
    pub fn configure_unison(&mut self, definition: UnisonDefinition) -> Result<()> {
        ensure!(
            self.update == 0 && self.unison.definition.is_none(),
            "Unison is already configured or battle has advanced"
        );
        ensure!(
            (2..=crate::PARTY_CAPACITY).contains(&definition.members.len()),
            "Unison needs two to four party members"
        );
        ensure!(
            definition.minimum_distance.is_finite()
                && definition.minimum_distance >= 0.
                && definition.short_weapon_penalty.is_finite()
                && definition.short_weapon_penalty >= 0.
                && definition.restore_height.is_finite()
                && definition.placement.iter().all(|direction| direction
                    .iter()
                    .all(|value| value.is_finite())
                    && direction[1] == 0.
                    && direction[0].hypot(direction[2]) > 0.),
            "invalid Unison placement"
        );
        let mut actors = BTreeSet::new();
        for member in &definition.members {
            ensure!(
                actors.insert(member.actor) && self.actor(member.actor)?.side == Side::Party,
                "invalid Unison member"
            );
            member.opener.validate()?;
            ensure!(
                member.opener_contact_ticks > 0,
                "missing Unison opener deadline"
            );
            let mut actions = BTreeSet::new();
            for arte in &member.artes {
                ensure!(
                    arte.distance.is_finite() && arte.distance >= 0. && arte.altitude.is_finite(),
                    "invalid Unison arte placement"
                );
                ensure!(
                    actions.insert(arte.action)
                        && self
                            .prepared
                            .technique(member.actor, arte.action)
                            .is_some_and(
                                |row| row.capabilities.target == crate::TechniqueTarget::Enemy
                            ),
                    "unprepared Unison arte"
                );
            }
        }
        for &target in &definition.immune_targets {
            ensure!(
                self.actor(target)?.side == Side::Enemy,
                "invalid Unison immunity target"
            );
        }
        if let Some(prelude) = &definition.combination_prelude {
            ensure!(
                prelude.first_x.is_finite()
                    && prelude.spacing.is_finite()
                    && prelude
                        .camera
                        .eye
                        .iter()
                        .chain(&prelude.camera.focus)
                        .all(|value| value.is_finite()),
                "invalid Unison prelude placement"
            );
            for &(actor, _) in &prelude.voices {
                ensure!(
                    actors.contains(&actor),
                    "Unison prelude voice is not a member"
                );
            }
        }
        if let Some(recipes) = &definition.recipes {
            let mut ids = BTreeSet::new();
            for recipe in recipes {
                ensure!(
                    recipe.id.0 != 0
                        && ids.insert(recipe.id)
                        && recipe.duration > 0
                        && recipe.camera_pitch.is_finite()
                        && recipe
                            .ingredients
                            .iter()
                            .all(|row| (2..=crate::PARTY_CAPACITY).contains(&row.len())
                                && row.iter().all(|id| id.0 != 0)),
                    "invalid Unison combination recipe"
                );
                if let Some(execution) = &recipe.execution {
                    ensure!(
                        definition.combination_prelude.is_some(),
                        "Unison combination has no prelude"
                    );
                    for role in &execution.roles {
                        let mut bound = BTreeSet::new();
                        ensure!(!role.is_empty(), "Unison combination has an unbound role");
                        for performer in role {
                            ensure!(
                                actors.contains(&performer.actor)
                                    && bound.insert(performer.actor)
                                    && self.prepared.actions.get(performer.action).is_some_and(
                                        |action| matches!(
                                            action.execution,
                                            ActionExecution::Attack(_)
                                        )
                                    ),
                                "invalid Unison combination action"
                            );
                            if let Some(placement) = performer.placement {
                                ensure!(
                                    placement.distance.is_finite()
                                        && placement.distance >= 0.
                                        && placement
                                            .direction
                                            .iter()
                                            .all(|value| value.is_finite())
                                        && placement.direction[1] == 0.
                                        && placement.direction[0].hypot(placement.direction[2])
                                            > 0.,
                                    "invalid Unison combination placement"
                                );
                            }
                        }
                    }
                    for &(pair, speaker, _) in &execution.voices {
                        ensure!(
                            pair[0] != pair[1]
                                && pair.contains(&speaker)
                                && pair
                                    .iter()
                                    .enumerate()
                                    .all(|(role, actor)| execution.roles[role]
                                        .iter()
                                        .any(|binding| binding.actor == *actor)),
                            "invalid Unison combination speaker"
                        );
                    }
                }
            }
        }
        self.unison.openers = definition
            .members
            .iter()
            .map(|member| {
                self.prepared.actions.insert(ActionDefinition {
                    normal: None,
                    execution: ActionExecution::Attack(member.opener.clone()),
                    tp_cost: 0,
                })
            })
            .collect();
        self.unison.definition = Some(Arc::new(definition));
        Ok(())
    }

    pub fn last_unison_completion(&self) -> Option<&UnisonCompletion> {
        self.unison.completion.as_ref()
    }

    pub(crate) fn step_unison(
        &mut self,
        input: Option<UnisonInput>,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let mut state = std::mem::take(&mut self.unison);
        let result = state.step(self, input, cues);
        self.unison = state;
        result
    }

    pub(crate) fn unison_contact_allowed(&self, owner: ActorId, target: ActorId) -> bool {
        self.unison
            .target()
            .is_none_or(|held| target == held && !self.unison.holds_actor(owner))
    }

    pub(crate) fn unison_opener_contact(
        &mut self,
        owner: ActorId,
        action: ActionId,
        target: ActorId,
        effective: bool,
    ) {
        if let Some(run) = &mut self.unison.run
            && run.phase == UnisonPhase::Opening
            && owner == run.initiator
            && run.opener_action == Some(action)
        {
            run.geometric_contact = Some(target);
            if effective {
                run.effective_contact = Some(target);
            }
        }
    }

    pub(crate) fn defer_unison_defeat(&mut self, target: ActorId, finisher: bool) -> bool {
        if self.unison.target() != Some(target) {
            return false;
        }
        self.unison.run.as_mut().unwrap().pending_defeat = Some(finisher);
        true
    }

    pub(crate) fn unison_defeat_pending(&self, target: ActorId) -> bool {
        self.unison.target() == Some(target)
            && self
                .unison
                .run
                .as_ref()
                .is_some_and(|run| run.pending_defeat.is_some())
    }

    pub(crate) fn restore_combination_weapons(&mut self, action: ActionId, actor: ActorId) {
        if self
            .unison
            .run
            .as_ref()
            .and_then(|run| run.combined.as_ref())
            .is_some_and(|combined| combined.actions.contains(&Some(action)))
            && let Some(models) = &mut self.models
        {
            models.restore_unison_weapons(actor);
        }
    }

    pub(crate) fn unison_feedback(&mut self, duration: u16, limit: u8) {
        let Some(run) = &mut self.unison.run else {
            return;
        };
        if run.feedback_request_update != Some(self.update) {
            self.unison.feedback_generation += 1;
            run.feedback_request_update = Some(self.update);
        }
        run.feedback = Some(UnisonFeedback {
            generation: self.unison.feedback_generation,
            expansion: run
                .feedback
                .map_or(1, |feedback| feedback.expansion)
                .min(limit),
            alpha: 160,
            limit,
            remaining: duration.saturating_sub(1),
        });
    }

    pub(crate) fn unison_contact_mode(
        &self,
        owner: ActorId,
        action: ActionId,
    ) -> crate::damage::ContactMode {
        let Some(run) = &self.unison.run else {
            return crate::damage::ContactMode::Ordinary;
        };
        if self.unison.staged() {
            crate::damage::ContactMode::Unison {
                held: true,
                bonus: self.unison.definition.as_ref().unwrap().damage_bonus,
            }
        } else if run.phase == UnisonPhase::Opening
            && owner == run.initiator
            && run.opener_action == Some(action)
        {
            crate::damage::ContactMode::Unison {
                held: false,
                bonus: false,
            }
        } else {
            crate::damage::ContactMode::Ordinary
        }
    }

    fn clear_unison_work(&mut self, cues: &mut Vec<Cue>) {
        for index in 0..self.actors.len() {
            let actor = ActorId(index as u8);
            self.cancel_pending_item(actor);
            self.clear_technique_command(actor);
            self.interrupt_actor(actor, cues);
            self.actors[index].stored_spell = None;
        }
        for (action, _) in std::mem::take(&mut self.volleys) {
            cues.push(Cue::Interrupted { action });
        }
        let projectiles: Vec<_> = self.projectiles.keys().copied().collect();
        for projectile in projectiles {
            self.expire_projectile(projectile, cues);
        }
        self.retire_weapon_flights();
        self.clear_hourglass();
        self.timed_hold = None;
    }
}

impl State {
    fn step(
        &mut self,
        battle: &mut Battle,
        input: Option<UnisonInput>,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if self.run.is_none() {
            if let Some(UnisonInput::Activate(actor)) = input {
                self.activate(battle, actor, cues)?;
            }
            return Ok(());
        }
        let definition = self.definition.as_ref().unwrap();
        let run = self.run.as_mut().unwrap();
        if let Some(feedback) = &mut run.feedback {
            if feedback.remaining != 0 {
                feedback.expansion = feedback.expansion.saturating_add(1).min(feedback.limit);
                feedback.remaining -= 1;
            } else {
                feedback.expansion = feedback.expansion.saturating_sub(1);
                if feedback.expansion == 0 {
                    run.feedback = None;
                }
            }
        }
        if run.phase == UnisonPhase::Staging {
            battle.actors[run.target.unwrap().index()].position = [0.; 3];
            run.phase = UnisonPhase::Executing;
            run.elapsed = 0;
            run.deadline = 0;
            run.launch(battle, definition, run.selections[0], cues)?;
            run.orbit = Some(Orbit::new(&mut battle.random, CAMERA_PITCH));
        }
        if let Some(UnisonInput::Select { pressed_slots }) = input {
            if matches!(run.phase, UnisonPhase::Selecting | UnisonPhase::Executing) {
                for (slot, member) in definition.members.iter().enumerate() {
                    if pressed_slots & (1 << slot) == 0 || run.blocked & (1 << slot) != 0 {
                        continue;
                    }
                    let Some(arte) = member.selected_arte(battle) else {
                        continue;
                    };
                    if !battle.actors[member.actor.index()].available() {
                        continue;
                    }
                    let selection = Selection { slot, arte };
                    run.blocked |= 1 << slot;
                    run.selections.push(selection);
                    if run.phase == UnisonPhase::Selecting {
                        run.stage(battle, definition, cues)?;
                        return Ok(());
                    }
                    run.launch(battle, definition, selection, cues)?;
                    break;
                }
            }
        }
        match run.phase {
            UnisonPhase::Windup if run.elapsed > WINDUP_TICKS => {
                let slot = definition
                    .members
                    .iter()
                    .position(|member| member.actor == run.initiator)
                    .unwrap();
                let target = battle
                    .target(run.initiator)
                    .context("Unison opener has no target")?;
                battle.interrupt_actor(run.initiator, cues);
                let (id, mut sequence) =
                    battle.allocate_sequence(self.openers[slot], run.initiator, target)?;
                sequence.unison = true;
                sequence.combo = None;
                battle.put_sequence(id, sequence);
                run.opener_action = Some(id);
                cues.push(Cue::Started {
                    definition: Some(self.openers[slot]),
                    action: id,
                    actor: run.initiator,
                });
                run.phase = UnisonPhase::Opening;
                run.elapsed = 1;
                run.deadline = definition.members[slot].opener_contact_ticks;
                return Ok(());
            }
            UnisonPhase::Opening if run.elapsed >= run.deadline => {
                let eligible = |target: ActorId| {
                    battle.actors[target.index()].available()
                        && !definition.immune_targets.contains(&target)
                };
                if run.effective_contact.is_some_and(eligible) {
                    run.target = run
                        .geometric_contact
                        .filter(|target| eligible(*target))
                        .or(run.effective_contact);
                    run.phase = UnisonPhase::Selecting;
                    run.elapsed = 0;
                    return Ok(());
                }
                self.completion = Some(UnisonCompletion::Cancelled);
                self.run = None;
                return Ok(());
            }
            UnisonPhase::Selecting if run.elapsed > FIRST_SELECTION_TICKS => {
                self.completion = Some(UnisonCompletion::Cancelled);
                self.run = None;
                return Ok(());
            }
            UnisonPhase::Executing if run.elapsed > run.deadline => {
                if run.begin_combination(battle, definition, cues) {
                    return Ok(());
                }
                run.finish(battle, definition, cues)?;
                run.phase = UnisonPhase::Restoring;
                run.elapsed = 0;
                return Ok(());
            }
            UnisonPhase::CombinationPose if run.elapsed > COMBINATION_POSE_TICKS => {
                run.launch_combination(battle, definition, cues)?;
            }
            UnisonPhase::Combined if run.elapsed > run.deadline => {
                let combined = run.combined.as_ref().unwrap();
                let recipe = &definition.recipes.as_ref().unwrap()[combined.recipe];
                run.completion = Some(UnisonCompletion::CombinationExecuted {
                    id: recipe.id,
                    native_id: recipe.native_id,
                    actors: combined.actors.to_vec(),
                });
                run.finish(battle, definition, cues)?;
                run.phase = UnisonPhase::Restoring;
                run.elapsed = 0;
                return Ok(());
            }
            UnisonPhase::Restoring => {
                run.restore(battle, cues);
                self.completion = run.completion.take();
                self.run = None;
                return Ok(());
            }
            _ => {}
        }
        if matches!(run.phase, UnisonPhase::Executing | UnisonPhase::Combined) {
            run.frame_camera(battle, definition);
        }
        if run.phase == UnisonPhase::Combined && run.elapsed >= run.deadline.saturating_sub(15) {
            run.color = None;
        }
        run.elapsed = run.elapsed.saturating_add(1);
        Ok(())
    }

    fn activate(&mut self, battle: &mut Battle, actor: ActorId, cues: &mut Vec<Cue>) -> Result<()> {
        let Some(definition) = &self.definition else {
            return Ok(());
        };
        let Some(member) = definition
            .members
            .iter()
            .find(|member| member.actor == actor)
        else {
            return Ok(());
        };
        let owner = battle.actor(actor)?;
        if !battle.prepared.unison_available
            || battle.unison_gauge != crate::MAX_UNISON_GAUGE
            || battle.phase() != crate::BattlePhase::Combat
            || battle.is_paused()
            || !owner.available()
            || owner.airborne()
            || !matches!(
                owner.control,
                crate::Control::Manual | crate::Control::SemiAuto
            )
            // Finisher artes lock the rest of their chain. The final normal
            // strike remains cancellable into the opening team attack.
            || battle.runtime[actor.index()].combo.history.finisher
            || matches!(
                battle.activity(actor),
                crate::Activity::Hurt
                    | crate::Activity::KnockedDown
                    | crate::Activity::GettingUp
                    | crate::Activity::Stunned
            )
            || definition
                .members
                .iter()
                .filter(|member| battle.actors[member.actor.index()].available())
                .count()
                < 2
        {
            return Ok(());
        }
        let mut blocked = !((1u8 << definition.members.len()) - 1);
        for (slot, member) in definition.members.iter().enumerate() {
            let actor = &battle.actors[member.actor.index()];
            if !actor.available()
                || actor
                    .conditions
                    .effective()
                    .contains(crate::conditions::Condition::Curse)
                || member.selected_arte(battle).is_none()
            {
                blocked |= 1 << slot;
            }
        }
        if !definition.retain_gauge || battle.random.next_u16() % 100 >= RETAIN_GAUGE_PERCENT {
            battle.unison_gauge = 0;
        }
        // Accepted activation hands the selected opponent's live combo to the
        // opener. Its recovery resumes on a miss; an already ended combo is not
        // revived, and geometric contact cannot transfer this ownership.
        let opening_combo_target = battle.target(actor).filter(|target| {
            let opponent = &battle.actors[target.index()];
            opponent.available()
                && opponent.reaction.combo_hits > 0
                && !definition.immune_targets.contains(target)
        });
        battle.interrupt_actor(actor, cues);
        battle.request_pose(actor, Some(member.windup_motion), member.windup_pose);
        if let Some(voice) = member.windup_voice {
            battle.request_voice(actor, voice, crate::VoicePriority::Action);
        }
        cues.push(Cue::EffectDuringPause(EffectRequest {
            owner: actor,
            target: actor,
            appearance: definition.windup_effect,
            origin: battle.actors[actor.index()].effect_origin(),
            heading: battle.actors[actor.index()].heading,
            follow: None,
            scale: battle.actors[actor.index()].effect_scale,
            tint: Default::default(),
        }));
        cues.push(Cue::Sound {
            actor,
            sound: definition.windup_sound,
            position: battle.actors[actor.index()].position,
            priority: 2,
        });
        let starts = &mut battle.ledger.achievements[actor.index()].unison_starts;
        *starts = starts.saturating_add(1);
        battle.ledger.adjust(START_GRADE);
        self.generation += 1;
        self.feedback_generation += 1;
        self.completion = None;
        self.run = Some(Run {
            initiator: actor,
            phase: UnisonPhase::Windup,
            elapsed: 1,
            deadline: 0,
            blocked,
            geometric_contact: None,
            effective_contact: None,
            opener_action: None,
            opening_combo_target,
            target: None,
            selections: Vec::new(),
            placements: Vec::new(),
            pending_defeat: None,
            orbit: None,
            camera: None,
            completion: None,
            combined: None,
            color: None,
            feedback: Some(UnisonFeedback {
                generation: self.feedback_generation,
                expansion: 1,
                alpha: 176,
                limit: 2,
                remaining: WINDUP_TICKS - 1,
            }),
            feedback_request_update: None,
        });
        Ok(())
    }
}

impl Run {
    fn stage(
        &mut self,
        battle: &mut Battle,
        definition: &UnisonDefinition,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        self.placements = battle
            .actors
            .iter()
            .zip(&battle.runtime)
            .map(|(actor, state)| Placement {
                position: [
                    actor.position[0],
                    definition.restore_height,
                    actor.position[2],
                ],
                heading: actor.heading,
                facing: actor.facing_direction,
                target: state.target,
            })
            .collect();
        if let Some(models) = &mut battle.models {
            models.begin_unison_weapons();
        }
        battle.clear_unison_work(cues);
        for index in 0..battle.actors.len() {
            let actor = ActorId(index as u8);
            battle.end_overlimit(actor)?;
            if battle.actors[index].available() {
                if Some(actor) == self.target {
                    battle.reset_idle_state(actor);
                } else {
                    battle.enter_idle(actor);
                }
            }
            battle.actors[index].hit_stop = 0;
        }
        self.phase = UnisonPhase::Staging;
        self.elapsed = 0;
        Ok(())
    }

    fn launch(
        &mut self,
        battle: &mut Battle,
        definition: &UnisonDefinition,
        selection: Selection,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let member = &definition.members[selection.slot];
        let target = self.target.unwrap();
        let gap = if member.short_weapon {
            (selection.arte.distance - definition.short_weapon_penalty)
                .max(definition.minimum_distance)
        } else {
            selection.arte.distance
        };
        battle.place_unison_actor(
            member.actor,
            target,
            definition.placement[selection.slot],
            gap,
            selection.arte.altitude,
        )?;
        let duration = battle.start_unison_arte(
            ActionRequest {
                actor: member.actor,
                action: selection.arte.action,
                target,
            },
            cues,
        )?;
        self.deadline = self.deadline.max(
            self.elapsed
                .saturating_add(selection.arte.duration.max(duration)),
        );
        Ok(())
    }

    fn begin_combination(
        &mut self,
        battle: &mut Battle,
        definition: &UnisonDefinition,
        cues: &mut Vec<Cue>,
    ) -> bool {
        let Some(recipes) = &definition.recipes else {
            self.completion = Some(UnisonCompletion::RecipeCatalogueUnavailable);
            return false;
        };
        for (recipe_index, recipe) in recipes.iter().enumerate() {
            for ingredients in &recipe.ingredients {
                let mut actors = Vec::new();
                for native in ingredients {
                    let Some(selected) = self.selections.iter().find(|selection| {
                        selection.arte.native_id == *native
                            && !actors.contains(&definition.members[selection.slot].actor)
                    }) else {
                        break;
                    };
                    actors.push(definition.members[selected.slot].actor);
                }
                if actors.len() == ingredients.len() {
                    self.completion = Some(UnisonCompletion::CombinationPending {
                        id: recipe.id,
                        native_id: recipe.native_id,
                        actors: actors.clone(),
                    });
                    let Some(execution) = &recipe.execution else {
                        return false;
                    };
                    let Ok(actors) = <[ActorId; 2]>::try_from(actors) else {
                        return false;
                    };
                    let roles = std::array::from_fn::<_, 2, _>(|role| {
                        execution.roles[role]
                            .iter()
                            .find(|binding| binding.actor == actors[role])
                            .cloned()
                    });
                    let [Some(first), Some(second)] = roles else {
                        return false;
                    };
                    self.combined = Some(Combined {
                        recipe: recipe_index,
                        actors,
                        roles: [first, second],
                        actions: [None; 2],
                    });
                    self.completion = None;
                    self.combination_pose(battle, definition, cues);
                    return true;
                }
            }
        }
        self.completion = Some(UnisonCompletion::NoCombination);
        false
    }

    fn combination_pose(
        &mut self,
        battle: &mut Battle,
        definition: &UnisonDefinition,
        cues: &mut Vec<Cue>,
    ) {
        let prelude = definition.combination_prelude.as_ref().unwrap();
        let combined = self.combined.as_mut().unwrap();
        battle.clear_unison_work(cues);
        battle.actors[self.target.unwrap().index()].position = [0.; 3];
        for (slot, &actor) in combined.actors.iter().enumerate() {
            let member = definition
                .members
                .iter()
                .find(|member| member.actor == actor)
                .unwrap();
            let owner = &mut battle.actors[actor.index()];
            owner.position = [prelude.first_x + prelude.spacing * slot as f32, 0., 0.];
            owner.heading = 0.;
            owner.facing_direction = [0., 0., 1.];
            owner.movement.forward = 0.;
            owner.movement.vertical = 0.;
            owner.hit_stop = 0;
            battle.request_pose(actor, Some(member.windup_motion), member.windup_pose);
            cues.push(Cue::EffectDuringPause(EffectRequest {
                owner: actor,
                target: actor,
                appearance: prelude.effect,
                origin: battle.actors[actor.index()].effect_origin(),
                heading: 0.,
                follow: None,
                scale: battle.actors[actor.index()].effect_scale,
                tint: Default::default(),
            }));
        }
        let speaker =
            combined.actors[usize::from(battle.random.next_u16()) % combined.actors.len()];
        if let Some((_, voice)) = prelude.voices.iter().find(|(actor, _)| *actor == speaker) {
            battle.request_voice(speaker, *voice, crate::VoicePriority::Action);
        }
        cues.push(Cue::Sound {
            actor: speaker,
            sound: prelude.sound,
            position: battle.actors[speaker.index()].position,
            priority: 2,
        });
        self.phase = UnisonPhase::CombinationPose;
        self.elapsed = 1;
        self.orbit = None;
        self.camera = Some(prelude.camera);
        self.color = prelude.color;
    }

    fn launch_combination(
        &mut self,
        battle: &mut Battle,
        definition: &UnisonDefinition,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        let combined = self.combined.as_mut().unwrap();
        let recipe = &definition.recipes.as_ref().unwrap()[combined.recipe];
        let execution = recipe.execution.as_ref().unwrap();
        let target = self.target.unwrap();
        battle.clear_unison_work(cues);
        for (index, role) in combined.roles.iter().enumerate() {
            if let Some(placement) = role.placement {
                battle.place_unison_actor(
                    role.actor,
                    target,
                    placement.direction,
                    placement.distance,
                    0.,
                )?;
            }
            battle.start_unison_combination(
                ActionRequest {
                    actor: role.actor,
                    action: role.action,
                    target,
                },
                cues,
            )?;
            combined.actions[index] = battle.runtime[role.actor.index()]
                .task()
                .action()
                .map(|(id, _)| *id);
            if !role.temporary_weapons.is_empty() {
                battle
                    .models
                    .as_mut()
                    .context("Unison temporary weapons need prepared models")?
                    .override_unison_weapons(role.actor, &role.temporary_weapons)?;
            }
        }
        self.phase = UnisonPhase::Combined;
        self.elapsed = 0;
        self.deadline = recipe.duration;
        self.color = execution.color;
        if let Some((_, speaker, sound)) = execution
            .voices
            .iter()
            .find(|(actors, _, _)| *actors == combined.actors)
        {
            battle.request_voice(*speaker, *sound, crate::VoicePriority::Action);
        }
        self.orbit = Some(Orbit::new(
            &mut battle.random,
            CAMERA_PITCH + recipe.camera_pitch,
        ));
        Ok(())
    }

    fn finish(
        &mut self,
        battle: &mut Battle,
        definition: &UnisonDefinition,
        cues: &mut Vec<Cue>,
    ) -> Result<()> {
        if let Some(finisher) = self.pending_defeat.take() {
            let target = self.target.unwrap();
            if battle.actors[target.index()].hp != 0 {
                return Ok(());
            }
            // Ordinary Unison credits its final selected performer when the scene finishes.
            let attacker = self.combined.as_ref().map_or_else(
                || definition.members[self.selections.last().unwrap().slot].actor,
                |combined| combined.actors[0],
            );
            battle.record_technical_smash(target, true, finisher, cues);
            battle.enter_death(target, cues);
            if finisher {
                battle.ledger.finisher();
            }
            battle.ledger.kill(attacker);
            if battle.actors[attacker.index()].available() {
                battle.recover_after_kill(attacker, cues);
            }
            battle.retarget_after_defeat(target)?;
        }
        Ok(())
    }

    fn restore(&mut self, battle: &mut Battle, cues: &mut Vec<Cue>) {
        battle.clear_unison_work(cues);
        if let Some(models) = &mut battle.models {
            models.end_unison_weapons();
        }
        for (index, saved) in self.placements.iter().enumerate() {
            let actor = &mut battle.actors[index];
            actor.position = saved.position;
            actor.heading = saved.heading;
            actor.facing_direction = saved.facing;
            actor.movement.direction = saved.facing;
            actor.movement.forward = 0.;
            actor.movement.vertical = 0.;
            actor.movement.acceleration = 0.;
            actor.hit_stop = 0;
            battle.runtime[index].target = saved.target;
            if actor.available() {
                let id = ActorId(index as u8);
                if Some(id) == self.target && actor.reaction.profile.can_knock_down {
                    // The surviving victim leaves the scene in its authored
                    // down recovery. Entering idle here would end the received
                    // combo before ordinary recovery or a follow-up contact.
                    battle.enter_knockdown(id, cues);
                    battle.initialize_knockdown_window(index, cues);
                } else {
                    battle.enter_idle(id);
                }
            }
        }
    }

    fn frame_camera(&mut self, battle: &Battle, definition: &UnisonDefinition) {
        let Some(orbit) = &mut self.orbit else {
            return;
        };
        let actor = self.combined.as_ref().map_or_else(
            || definition.members[self.selections.last().unwrap().slot].actor,
            |combined| combined.actors[0],
        );
        let actor = battle.actors[actor.index()].position;
        let target = battle.actors[self.target.unwrap().index()].position;
        let radius = (CAMERA_DISTANCE_SCALE * (actor[0] - target[0]).hypot(actor[2] - target[2])
            + CAMERA_BASE_RADIUS)
            .max(CAMERA_MIN_RADIUS);
        let focus = [
            (actor[0] + target[0]) * 0.5,
            CAMERA_FOCUS_HEIGHT,
            (actor[2] + target[2]) * 0.5,
        ];
        let (sin, cos) = orbit.yaw.to_radians().sin_cos();
        self.camera = Some(CameraPose {
            eye: [
                focus[0] + cos * radius,
                focus[1] + orbit.pitch.to_radians().sin() * radius,
                focus[2] + sin * radius,
            ],
            focus,
            pitch: orbit.pitch,
            yaw: orbit.yaw,
            radius,
        });
        orbit.yaw += orbit.speed;
    }
}
