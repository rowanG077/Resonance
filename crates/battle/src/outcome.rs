//! Recognition, transition primitives and explicit terminal ownership. The game
//! authors the result timeline; recognition never destroys the live battle.
use crate::{ActorId, Battle, BattleFrame, BattleOutcome, BattleResult, Cue, Side};
use anyhow::{Context, Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattlePhase {
    Combat,
    Ending,
    Results,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    FadeOut,
    FadeIn,
    Wipe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionFrame {
    pub color: [u8; 3],
    pub alpha: u8,
    /// One horizontal start offset per original screen row. Rendering scales
    /// the original 640 by 480 coordinates to the destination viewport.
    pub wipe_rows: Vec<u16>,
    pub wipe_progress: u16,
}

struct Transition {
    frame: TransitionFrame,
    kind: TransitionKind,
    speed: u8,
    complete: bool,
}

#[derive(Default)]
pub(crate) struct Terminal {
    pub result: Option<BattleResult>,
    retired: bool,
    escape: Option<bool>,
    transition: Option<Transition>,
}

impl Battle {
    pub fn phase(&self) -> BattlePhase {
        if self.ended {
            BattlePhase::Finished
        } else if self.terminal.retired {
            BattlePhase::Results
        } else if self.terminal.result.is_some() {
            BattlePhase::Ending
        } else {
            BattlePhase::Combat
        }
    }

    /// Source 2108/C988: the actor lifecycle controls eligibility, independently
    /// of HP and animation completion. Called before the shared world visit.
    pub fn recognize_result(&mut self) -> Option<BattleResult> {
        if self.entry_pending() {
            return None;
        }
        if self.ended || self.terminal.result.is_some() {
            return self.terminal.result;
        }
        // 3EB74/3E42C set/clear the shared stored-spell transition flag.
        // A released resident slot alone does not postpone recognition.
        if self.transition_owner().is_some() || self.target_selector.is_some() {
            return None;
        }
        let unavailable = |side| {
            self.actors.iter().any(|actor| actor.side == side)
                && self
                    .actors
                    .iter()
                    .filter(|actor| actor.side == side)
                    .all(|actor| !actor.available())
        };
        self.terminal.result = if self.terminal.escape == Some(true) {
            Some(BattleResult::Escaped)
        } else if unavailable(Side::Party) {
            Some(BattleResult::Defeat)
        } else if unavailable(Side::Enemy) {
            Some(BattleResult::Victory)
        } else if self.terminal.escape == Some(false) {
            Some(BattleResult::Escaped)
        } else {
            None
        };
        self.terminal.result
    }

    /// The escape owner calls this only after its source success condition.
    pub fn recognize_escape(&mut self, forced: bool) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Combat,
            "battle is already ending"
        );
        self.terminal.escape = Some(forced);
        Ok(())
    }

    pub fn forced_escape(&self) -> bool {
        self.terminal.escape == Some(true)
    }

    /// Source C764's explicit boundary: retire combat object groups and released
    /// spells while retaining actor commands, models, voices and the battle RNG.
    pub fn retire_combat(&mut self) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == BattlePhase::Ending,
            "combat retirement is not pending"
        );
        let mut cues = Vec::new();
        self.sequences.retain(|&action, sequence| {
            let keep = sequence.definition.phase.is_actor();
            if !keep {
                cues.push(Cue::Interrupted { action });
            }
            keep
        });
        self.projectiles.clear();
        self.retire_weapon_flights();
        self.particles.clear();
        self.scenes = [None, None];
        self.actors_visible = true;
        self.terminal.retired = true;
        Ok(cues)
    }

    /// Gameplay RNG remains with the live battle through reward, voice and wipe
    /// selection. The game must not resume a copy taken at the killing contact.
    pub fn draw_random(&mut self) -> u16 {
        self.random.next()
    }

    /// 57718 hides every enemy after title awards, regardless of death fade.
    /// Actor callbacks remain on their ordinary lifecycle until final cleanup.
    pub fn hide_result_enemies(&mut self) -> Result<()> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "enemy result hiding outside results"
        );
        for (actor, model) in self.actors.iter_mut().zip(&mut self.models) {
            if actor.side != Side::Enemy {
                continue;
            }
            actor.availability = crate::ActorAvailability::Dead;
            if let Some(model) = model {
                model.shown.visible = false;
                let slots: Vec<_> = model
                    .definition
                    .weapons
                    .iter()
                    .map(|weapon| weapon.slot)
                    .collect();
                for slot in slots {
                    model.weapon_visible(slot, false)?;
                }
            }
        }
        Ok(())
    }

    /// Start a prepared result controller on the same actor/task machinery used
    /// by combat. Its initializer runs at this callback's source visit.
    pub fn start_result_action(&mut self, actor: ActorId, action: u16) -> Result<Vec<Cue>> {
        ensure!(
            self.actor(actor)?.available(),
            "result performance needs an available actor"
        );
        self.start_result_controller(actor, action, true)
    }

    /// Result construction also initializes a petrified actor's expression while
    /// retaining its pose. The prepared initializer runs once even in that state.
    pub fn start_result_controller(
        &mut self,
        actor: ActorId,
        action: u16,
        performance: bool,
    ) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "result controller outside results"
        );
        ensure!(
            self.actor(actor)?.side == Side::Party,
            "result controller needs a party actor"
        );
        let definition = self
            .prepared
            .actions
            .iter()
            .position(|entry| entry.id == action)
            .context("unprepared result controller")?;
        ensure!(
            self.prepared.actions[definition].phase == crate::ActionPhase::Controller,
            "result action must be an actor controller"
        );
        let mut cues = Vec::new();
        self.interrupt_actor(actor, &mut cues);
        self.actors[actor.index()].hud.result_performance = performance;
        let (id, mut sequence) = self.allocate_sequence(definition, actor, actor)?;
        crate::script::step_sequence(self, id, &mut sequence, &mut cues)?;
        self.sequences.insert(id, sequence);
        cues.push(Cue::Started { action: id, actor });
        Ok(cues)
    }

    /// Independent prepared result effects must leave actor pose controllers running.
    pub fn start_result_resident(&mut self, actor: ActorId, action: u16) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == BattlePhase::Results && self.actor(actor)?.side == Side::Party,
            "result resident needs a party actor in results"
        );
        let definition = self
            .prepared
            .actions
            .iter()
            .position(|entry| entry.id == action)
            .context("unprepared result resident")?;
        let entry = &self.prepared.actions[definition];
        ensure!(
            entry.phase == crate::ActionPhase::Resident && entry.tp_cost == 0,
            "result resident must be a free prepared task"
        );
        let mut cues = Vec::new();
        let (id, mut sequence) = self.allocate_sequence(definition, actor, actor)?;
        crate::script::step_sequence(self, id, &mut sequence, &mut cues)?;
        self.sequences.insert(id, sequence);
        cues.push(Cue::Started { action: id, actor });
        Ok(cues)
    }

    /// 57718 / C908 replace party combat callbacks with the result controller.
    /// The source normalizes availability after choosing a pose: zero HP stays
    /// zero, while petrification retains both availability and the current pose.
    pub fn reset_result_actor(&mut self, id: ActorId, performance: bool) -> Result<Vec<Cue>> {
        ensure!(
            self.phase() == BattlePhase::Results,
            "actor reset outside results"
        );
        ensure!(
            self.actor(id)?.side == Side::Party,
            "result reset needs a party actor"
        );
        let index = id.index();
        ensure!(
            self.models[index].is_some(),
            "result actor model is not prepared"
        );
        let mut cues = Vec::new();
        self.interrupt_actor(id, &mut cues);
        self.sequences.retain(|&action, sequence| {
            let remove =
                sequence.actor == id && sequence.definition.phase == crate::ActionPhase::Decision;
            if remove {
                cues.push(Cue::Interrupted { action });
            }
            !remove
        });
        self.approaches[index] = None;
        self.decisions[index] = None;
        self.controls[index] = None;
        self.weapon_flights.retain(|&(owner, _), _| owner != id);
        let actor = &mut self.actors[index];
        actor.activity = crate::Activity::Idle;
        actor.position[1] = 0.;
        actor.hit_stop = 0;
        actor.movement = crate::Movement::default();
        actor.guard.active = false;
        actor.reset_hud_targets();
        actor.hud.result_performance = performance;
        actor.body.tint = [64, 64, 64, 255];
        // 57718 instructions59800..59820 clear the flag and model vector,
        // retaining the actor's signed countdown.
        actor.body.jitter.stop();
        actor.body.jitter.take_acceleration();
        if actor.availability != crate::ActorAvailability::Petrified {
            actor.availability = crate::ActorAvailability::Active;
        }
        let model = self.models[index].as_mut().unwrap();
        model.shown.root_translation = [0.; 3];
        model.shown.light = Some(std::array::from_fn(|axis| {
            actor.position[axis] + [150., 300., 200.][axis]
        }));
        model.attach_weapons(actor);
        let slots: Vec<_> = model
            .definition
            .weapons
            .iter()
            .map(|weapon| weapon.slot)
            .collect();
        for slot in slots {
            model.weapon_visible(slot, true)?;
        }
        Ok(cues)
    }

    pub fn set_actor_vitals(
        &mut self,
        actor: ActorId,
        hp: i32,
        maximum_hp: i32,
        tp: u16,
        maximum_tp: u16,
    ) -> Result<()> {
        ensure!(
            !self.ended && maximum_hp > 0 && (0..=maximum_hp).contains(&hp) && tp <= maximum_tp,
            "invalid result vitals"
        );
        let actor = self
            .actors
            .get_mut(actor.index())
            .context("unknown result actor")?;
        actor.hp = hp;
        actor.max_hp = maximum_hp;
        actor.tp = tp;
        actor.max_tp = maximum_tp;
        Ok(())
    }

    /// Source 11880. Wipe construction consumes all 480 draws immediately.
    pub fn begin_transition(
        &mut self,
        kind: TransitionKind,
        color: [u8; 3],
        speed: u8,
    ) -> Result<()> {
        ensure!(
            !self.ended && (1..=127).contains(&speed),
            "invalid battle transition"
        );
        let rows = if kind == TransitionKind::Wipe {
            (0..480).map(|_| self.random.next() % 640).collect()
        } else {
            Vec::new()
        };
        self.terminal.transition = Some(Transition {
            frame: TransitionFrame {
                color,
                alpha: if kind == TransitionKind::FadeIn {
                    255
                } else {
                    0
                },
                wipe_rows: rows,
                wipe_progress: 0,
            },
            kind,
            speed,
            complete: false,
        });
        Ok(())
    }

    /// Source 117E8: the visit which reaches the bound still returns false;
    /// completion becomes observable on the next visit.
    pub fn advance_transition(&mut self) -> Result<bool> {
        let transition = self
            .terminal
            .transition
            .as_mut()
            .context("no battle transition")?;
        if transition.complete {
            return Ok(true);
        }
        match transition.kind {
            TransitionKind::FadeIn => {
                transition.frame.alpha = transition.frame.alpha.saturating_sub(transition.speed);
                transition.complete = transition.frame.alpha == 0;
            }
            TransitionKind::FadeOut | TransitionKind::Wipe => {
                transition.frame.alpha = transition.frame.alpha.saturating_add(transition.speed);
                transition.complete = transition.frame.alpha == 255;
                if transition.kind == TransitionKind::Wipe {
                    transition.frame.wipe_progress += 16;
                }
            }
        }
        Ok(false)
    }

    pub fn transition(&self) -> Option<&TransitionFrame> {
        self.terminal
            .transition
            .as_ref()
            .map(|transition| &transition.frame)
    }

    /// Called only by the completed encounter lifecycle. Invalidates all task
    /// owners after capturing final gameplay state, and cannot run twice.
    pub fn finish_result(&mut self) -> Result<BattleFrame> {
        ensure!(!self.ended, "battle outcome was already consumed");
        let result = self
            .terminal
            .result
            .context("battle has no recognized result")?;
        let outcome = BattleOutcome {
            result,
            actors: self.actors.clone(),
            random_state: self.random.0,
        };
        self.invalidate();
        Ok(self.frame(Vec::new(), Some(outcome)))
    }

    /// A failed lifecycle may not later commit its partial persistent candidate.
    pub fn invalidate(&mut self) {
        self.sequences.clear();
        self.projectiles.clear();
        self.retire_weapon_flights();
        self.particles.clear();
        self.scenes = [None, None];
        self.voices.fill(Default::default());
        self.actors_visible = true;
        self.ended = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{actor, prepared};

    #[test]
    fn result_notice_preserves_the_running_victory_controller() {
        use crate::{ActionDefinition, ActionPhase, BattleInput, PreparedBattle, ResourceBinding};
        use std::{collections::BTreeMap, sync::Arc};
        let definition = |id, source: &str, phase| {
            let source = BTreeMap::from([("test".into(), source.into())]);
            let compiled =
                symphonia_script_compiler::compile("test", &source, &crate::native_declarations())
                    .unwrap();
            let entry = compiled
                .program
                .authored()
                .unwrap()
                .functions
                .iter()
                .find(|function| function.name == "test::run")
                .unwrap()
                .entry;
            ActionDefinition {
                id,
                phase,
                program: Arc::new(compiled.program),
                entry,
                duration: 0,
                tp_cost: 0,
                resources: vec![ResourceBinding::Effect(37)],
            }
        };
        let controller = definition(
            1,
            r#"
            script battle; use battle;
            asset common: battle::Effect = "test/common";
            pub task run() {
                await battle::at_age(ticks(2));
                battle::show(common, 1, battle::owner());
                battle::finish();
            }
        "#,
            ActionPhase::Controller,
        );
        let notice = definition(
            2,
            include_str!("../../../scripts/battle/result_notice.sym"),
            ActionPhase::Resident,
        );
        let mut owner = actor(Side::Party);
        owner.position = [10., 20., 30.];
        owner.body.center_offset = [2., 4., 6.];
        owner.heading = 90.;
        let prepared = PreparedBattle::new(
            vec![owner],
            vec![controller, notice],
            1,
            vec![],
            vec![crate::tests::effect_binding(37, [1, 20])],
        )
        .unwrap();
        let mut battle = Battle::new(Arc::new(prepared));
        // Reach the terminal boundary through its public transitions.
        battle.recognize_escape(true).unwrap();
        assert_eq!(battle.recognize_result(), Some(BattleResult::Escaped));
        battle.retire_combat().unwrap();
        let started = battle.start_result_controller(ActorId(0), 1, true).unwrap();
        let controller = started
            .iter()
            .find_map(|cue| match cue {
                Cue::Started { action, .. } => Some(*action),
                _ => None,
            })
            .unwrap();
        let cues = battle.start_result_resident(ActorId(0), 2).unwrap();
        assert!(
            !cues
                .iter()
                .any(|cue| matches!(cue, Cue::Interrupted { .. }))
        );
        assert!(cues.iter().any(|cue| matches!(cue,
            Cue::Effect { member: 20, position, heading, .. }
            if *position == [13.5, 27., 40.5] && *heading == 90.
        )));
        assert!(battle.sequences.contains_key(&controller));
        let mut observed = false;
        for _ in 0..3 {
            let frame = battle.step(BattleInput::default()).unwrap();
            assert!(
                !frame
                    .cues
                    .contains(&Cue::Interrupted { action: controller })
            );
            observed |= frame
                .cues
                .iter()
                .any(|cue| matches!(cue, Cue::Effect { member: 1, .. }));
        }
        assert!(
            observed,
            "victory controller must reach its delayed command"
        );
    }

    #[test]
    fn transition_completion_is_observed_on_the_following_visit() {
        let mut battle = Battle::new(prepared("pub task run() {}", vec![actor(Side::Party)], 0));
        battle
            .begin_transition(TransitionKind::FadeOut, [128; 3], 6)
            .unwrap();
        for _ in 0..43 {
            assert!(!battle.advance_transition().unwrap());
        }
        assert_eq!(battle.transition().unwrap().alpha, 255);
        assert!(battle.advance_transition().unwrap());
        battle
            .begin_transition(TransitionKind::FadeIn, [128; 3], 5)
            .unwrap();
        for _ in 0..51 {
            assert!(!battle.advance_transition().unwrap());
        }
        assert_eq!(battle.transition().unwrap().alpha, 0);
        assert!(battle.advance_transition().unwrap());
    }

    #[test]
    fn wipe_consumes_480_rng_values_at_construction_and_keeps_its_render_rows() {
        let mut battle = Battle::new(prepared("pub task run() {}", vec![actor(Side::Party)], 0));
        let mut random = crate::state::Random(battle.random_state());
        let rows: Vec<_> = (0..480).map(|_| random.next() % 640).collect();
        battle
            .begin_transition(TransitionKind::Wipe, [0; 3], 4)
            .unwrap();
        assert_eq!(battle.random_state(), random.0);
        assert_eq!(battle.transition().unwrap().wipe_rows, rows);
        for _ in 0..64 {
            assert!(!battle.advance_transition().unwrap());
        }
        assert_eq!(battle.transition().unwrap().wipe_progress, 1024);
        assert!(battle.advance_transition().unwrap());
        assert_eq!(battle.random_state(), random.0);
        assert!(
            battle
                .begin_transition(TransitionKind::Wipe, [0; 3], 0)
                .is_err()
        );
        assert_eq!(battle.random_state(), random.0);
    }
}
