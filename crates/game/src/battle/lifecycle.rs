//! One encounter owner driving the existing authored VM and the live battle.
//! Native operations are primitives; all terminal ordering lives in encounter.sym.
mod script;
use super::command;
use anyhow::{Context, Result, ensure};
use resonance_battle::{Battle, BattleFrame, BattleInput, Cue};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::Program;
use symphonia_script_compiler::{ScriptKind, SourceResolver};
use symphonia_script_tools::PreparationCache;
use symphonia_script_vm::{Memory, RunEvent, Tasks, Vm};

pub use script::native_declarations;

/// Prepared result resources and source-dependent predicates. Missing resources
/// stay false; a renderer must not claim readiness merely because time elapsed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Observations {
    pub music_ready: bool,
    pub performance_ready: bool,
    /// Source confirmation gate: true for ordinary performances; group events
    /// additionally require their completion or the leader voice finishing.
    pub performance_finished: bool,
    pub results_ready: bool,
    pub has_ex_notice: bool,
    pub suppress_music: bool,
    pub suppress_performance: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    SelectVictory { group: u8, pose: u8 },
    AcceptVictory,
    VictoryBanner,
    ConstructRewards,
    RecoverTp,
    PerformVictory,
    LevelNotices,
    ExNotices,
    UpdateResults { age: u32, confirm: bool },
    ResultCamera { age: u32 },
    DefeatNotice,
    EscapeNotice,
    RecordEscape,
    RequestMusic { track: u16 },
    PlayMusic { track: u16, fade_ms: u16 },
    StopMusic { fade_ms: u16 },
}

/// A synchronous request from the one encounter VM. The game applies persistent
/// operations to its candidate party, then acknowledges this exact request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    serial: u64,
    pub kind: RequestKind,
}

#[derive(Debug, Clone)]
pub struct Acknowledgement {
    request: Request,
    cues: Vec<Cue>,
}
impl Acknowledgement {
    pub fn with_cues(mut self, cues: Vec<Cue>) -> Self {
        self.cues = cues;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SelectionQuery {
    Leader,
    Available(u8),
    Dead(u8),
    HpPercent(u8),
    AffinityRank(u8),
    Participation(u8),
    Poisoned(u8),
    Story,
    PartyWasHit,
    AllHealthy,
}
impl Request {
    pub fn serial(&self) -> u64 {
        self.serial
    }
    pub fn acknowledge(self) -> Acknowledgement {
        Acknowledgement {
            request: self,
            cues: Vec::new(),
        }
    }
}

pub trait Services {
    fn after_world(&mut self, _frame: &BattleFrame) -> Result<()> {
        Ok(())
    }
    fn observations(&self, battle: &Battle) -> Observations;
    fn selection_query(&self, _query: SelectionQuery, _battle: &Battle) -> Result<i32> {
        anyhow::bail!("victory selection metadata is not prepared")
    }
    /// The callback may use Battle::draw_random and set_actor_vitals. It must
    /// finish validation/mutation before acknowledging. Failures invalidate the
    /// encounter; the field owner must discard its uncommitted party candidate.
    fn request(&mut self, request: Request, battle: &mut Battle) -> Result<Acknowledgement>;
}

#[derive(Debug, Default)]
pub struct Input {
    pub battle: BattleInput,
    pub command: command::Input,
    pub confirm: bool,
}

pub struct PreparedLifecycle {
    program: Arc<Program>,
    entry: u32,
    command_setup: Option<command::Setup>,
}
impl PreparedLifecycle {
    pub fn prepare(cache: &mut PreparationCache, sources: &impl SourceResolver) -> Result<Self> {
        let generation = cache.prepare(["battle::encounter"], sources, &native_declarations())?;
        let module = generation
            .module("battle::encounter")
            .context("missing encounter module")?;
        ensure!(
            module.kind == ScriptKind::Battle,
            "encounter must declare script battle"
        );
        ensure!(
            module.assets.is_empty(),
            "encounter resources must be prepared by its game owner"
        );
        let entry = module
            .program
            .authored()
            .context("encounter must be authored")?
            .functions
            .iter()
            .find(|f| f.name == "battle::encounter::run")
            .context("missing encounter run task")?;
        ensure!(
            entry.is_task && entry.parameters == 0 && entry.results == 0,
            "invalid encounter run task"
        );
        Ok(Self {
            program: module.program.clone(),
            entry: entry.entry,
            command_setup: None,
        })
    }

    pub fn with_command_setup(mut self, setup: command::Setup) -> Self {
        self.command_setup = Some(setup);
        self
    }

    pub fn start(&self) -> Result<Lifecycle> {
        Ok(Lifecycle {
            program: self.program.clone(),
            tasks: BTreeMap::from([(
                1,
                Task {
                    vm: Vm::new(self.program.clone(), self.entry)?,
                    wait: None,
                },
            )]),
            ownership: Tasks::default(),
            next_task: 2,
            next_request: 1,
            once: 0,
            finished: false,
            faulted: false,
            command: command::Owner::default(),
            command_setup: self.command_setup.clone(),
            command_events: Vec::new(),
            command_repeat_reset: false,
        })
    }
}

struct Task {
    vm: Vm,
    wait: Option<Wait>,
}
enum Wait {
    Update,
    Join(i32),
}

pub struct Lifecycle {
    program: Arc<Program>,
    tasks: BTreeMap<i32, Task>,
    ownership: Tasks,
    next_task: i32,
    next_request: u64,
    once: u8,
    finished: bool,
    faulted: bool,
    command: command::Owner,
    command_setup: Option<command::Setup>,
    command_events: Vec<command::Event>,
    command_repeat_reset: bool,
}

impl Lifecycle {
    fn cancel_children(&mut self, parent: i32) {
        for child in self.ownership.children(parent) {
            self.cancel_children(child);
            self.tasks.remove(&child);
            self.ownership.remove(child);
        }
    }

    /// Exactly one shared world visit per ordinary update. The authored finalizer
    /// is the only update allowed to complete without a world visit. Presentation
    /// and audio acknowledgments remain live throughout the result lifecycle.
    pub fn step(
        &mut self,
        battle: &mut Battle,
        mut input: Input,
        services: &mut impl Services,
    ) -> Result<BattleFrame> {
        ensure!(
            !self.finished && !self.faulted,
            "encounter lifecycle has ended"
        );
        if input.battle.menu_open {
            return battle.step(input.battle);
        }
        if battle.phase() != resonance_battle::BattlePhase::Combat {
            self.command = command::Owner::default();
        }
        let result = (|| {
            let admission = self
                .command_setup
                .as_ref()
                .map(|setup| setup.admission(battle, input.command.controller))
                .transpose()?
                .flatten();
            let command = self.command.step(input.command, admission);
            self.command_events.extend(command.events);
            self.command_repeat_reset |= command.reset_repeat;
            if command.pause_for_visit {
                input.battle.command_pause = true;
                battle.step(input.battle).and_then(|frame| {
                    services.after_world(&frame)?;
                    Ok(frame)
                })
            } else {
                self.advance(battle, input, services)
            }
        })();
        if result.is_err() {
            self.tasks.clear();
            self.ownership.clear();
            self.faulted = true;
            // Fatal/result ownership handoff cannot retain a source command
            // pause or renderer frame after the battle has been invalidated.
            self.command = command::Owner::default();
            self.command_events.clear();
            self.command_repeat_reset = false;
            battle.invalidate();
        }
        result
    }

    /// Immutable command state is consumed directly by the presentation strip.
    pub fn command_frame(&self) -> Option<command::Frame> {
        self.command.frame()
    }

    pub fn take_command_repeat_reset(&mut self) -> bool {
        std::mem::take(&mut self.command_repeat_reset)
    }

    /// Presentation/audio drains source 9E38-equivalent events after the
    /// retained world visit.  The lifecycle does not own a mixer.
    pub fn take_command_events(&mut self) -> Vec<command::Event> {
        std::mem::take(&mut self.command_events)
    }

    fn advance(
        &mut self,
        battle: &mut Battle,
        input: Input,
        services: &mut impl Services,
    ) -> Result<BattleFrame> {
        let confirm = input.confirm;
        let mut input = Some(input);
        let mut frame = None;
        let mut cues = Vec::new();
        let mut memory = Memory::default();
        let mut budget = 8192;
        let mut cursor = 0;
        while let Some(handle) = self.tasks.range((cursor + 1)..).next().map(|(&id, _)| id) {
            cursor = handle;
            let mut task = self.tasks.remove(&handle).unwrap();
            match task.wait.take() {
                Some(Wait::Update) => task.vm.complete(None, &mut memory)?,
                Some(Wait::Join(child)) => {
                    if let Some(result) = self
                        .ownership
                        .join(handle, child)
                        .map_err(anyhow::Error::msg)?
                    {
                        task.vm.complete_task(&result)?;
                    } else {
                        task.wait = Some(Wait::Join(child));
                        self.tasks.insert(handle, task);
                        continue;
                    }
                }
                None => {}
            }
            let mut wait = None;
            let mut host = script::EncounterHost {
                lifecycle: self,
                battle,
                services,
                input: &mut input,
                confirm,
                frame: &mut frame,
                cues: &mut cues,
                wait: &mut wait,
                handle,
            };
            let result = task
                .vm
                .run(&mut host, &mut memory, budget)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "encounter source {:?}: {error}",
                        task.vm.source_trace(error.pc)
                    )
                })?;
            budget -= result.steps;
            match result.event {
                RunEvent::Halted => {
                    self.cancel_children(handle);
                    self.ownership
                        .finish(handle, task.vm.result().unwrap_or_default());
                }
                RunEvent::Suspended { .. } => {
                    task.wait = Some(wait.context("encounter wait has no completion condition")?);
                    self.tasks.insert(handle, task);
                }
                RunEvent::SuspendedTask { handle: child } => {
                    task.wait = Some(Wait::Join(child));
                    self.tasks.insert(handle, task);
                }
            }
        }
        ensure!(
            !self.tasks.is_empty() || self.finished,
            "encounter returned before finalization"
        );
        let mut frame = frame.context("encounter update did not visit the world or finish")?;
        if frame.outcome.is_none() {
            let mut current = battle.snapshot();
            current.cues = frame.cues;
            frame = current;
        }
        Ok(frame)
    }
}

#[cfg(test)]
mod tests;
