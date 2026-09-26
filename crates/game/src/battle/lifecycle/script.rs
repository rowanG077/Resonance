use super::*;
use resonance_battle::{BattlePhase, BattleResult, TransitionKind};
use symphonia_script::authored::{NativeDeclaration, Type};
use symphonia_script_vm::{Host, NativeBindings, NativeResult};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Native {
    Recognize,
    World,
    Next,
    SelectVictory,
    AcceptVictory,
    Leader,
    Available,
    Dead,
    HpPercent,
    AffinityRank,
    Participation,
    Poisoned,
    Story,
    PartyWasHit,
    AllHealthy,
    Random,
    DeathWaiting,
    DeathReady,
    VictoryBanner,
    FadeOut,
    FadeIn,
    Wipe,
    TransitionStep,
    Retire,
    ConstructRewards,
    RecoverTp,
    Confirm,
    MusicReady,
    PerformanceReady,
    PerformanceFinished,
    ResultsReady,
    HasExNotice,
    SuppressMusic,
    SuppressPerformance,
    RequestMusic,
    PlayMusic,
    StopMusic,
    PerformVictory,
    LevelNotices,
    ExNotices,
    UpdateResults,
    ResultCamera,
    DefeatNotice,
    EscapeNotice,
    RecordEscape,
    ForcedEscape,
    Finish,
}
impl Native {
    const fn declaration(self) -> NativeDeclaration {
        let (name, parameters, result, suspends): (&str, &[Type], Option<Type>, bool) = match self {
            Self::Recognize => ("encounter::recognize", &[], Some(Type::I32), false),
            Self::World => ("encounter::world_update", &[], None, false),
            Self::Next => ("encounter::next_update", &[], None, true),
            Self::SelectVictory => (
                "encounter::select_victory",
                &[Type::I32, Type::I32],
                None,
                false,
            ),
            Self::AcceptVictory => ("encounter::accept_victory", &[], None, false),
            Self::Leader => ("encounter::leader", &[], Some(Type::I32), false),
            Self::Available => (
                "encounter::available",
                &[Type::I32],
                Some(Type::Bool),
                false,
            ),
            Self::Dead => ("encounter::dead", &[Type::I32], Some(Type::Bool), false),
            Self::HpPercent => (
                "encounter::hp_percent",
                &[Type::I32],
                Some(Type::I32),
                false,
            ),
            Self::AffinityRank => (
                "encounter::affinity_rank",
                &[Type::I32],
                Some(Type::I32),
                false,
            ),
            Self::Participation => (
                "encounter::participation",
                &[Type::I32],
                Some(Type::I32),
                false,
            ),
            Self::Poisoned => ("encounter::poisoned", &[Type::I32], Some(Type::Bool), false),
            Self::Story => ("encounter::story", &[], Some(Type::I32), false),
            Self::PartyWasHit => ("encounter::party_was_hit", &[], Some(Type::Bool), false),
            Self::AllHealthy => ("encounter::all_healthy", &[], Some(Type::Bool), false),
            Self::Random => ("encounter::random", &[], Some(Type::I32), false),
            Self::DeathWaiting => ("encounter::death_waiting", &[], Some(Type::Bool), false),
            Self::DeathReady => ("encounter::death_ready", &[], Some(Type::Bool), false),
            Self::VictoryBanner => ("encounter::victory_banner", &[], None, false),
            Self::FadeOut => (
                "encounter::fade_out",
                &[Type::I32, Type::I32, Type::I32, Type::I32],
                None,
                false,
            ),
            Self::FadeIn => (
                "encounter::fade_in",
                &[Type::I32, Type::I32, Type::I32, Type::I32],
                None,
                false,
            ),
            Self::Wipe => ("encounter::wipe_out", &[Type::I32], None, false),
            Self::TransitionStep => ("encounter::transition_step", &[], Some(Type::Bool), false),
            Self::Retire => ("encounter::retire_combat", &[], None, false),
            Self::ConstructRewards => ("encounter::construct_rewards", &[], None, false),
            Self::RecoverTp => ("encounter::recover_tp", &[], None, false),
            Self::Confirm => ("encounter::confirm", &[], Some(Type::Bool), false),
            Self::MusicReady => ("encounter::music_ready", &[], Some(Type::Bool), false),
            Self::PerformanceReady => {
                ("encounter::performance_ready", &[], Some(Type::Bool), false)
            }
            Self::PerformanceFinished => (
                "encounter::performance_finished",
                &[],
                Some(Type::Bool),
                false,
            ),
            Self::ResultsReady => ("encounter::results_ready", &[], Some(Type::Bool), false),
            Self::HasExNotice => ("encounter::has_ex_notice", &[], Some(Type::Bool), false),
            Self::SuppressMusic => ("encounter::suppress_music", &[], Some(Type::Bool), false),
            Self::SuppressPerformance => (
                "encounter::suppress_performance",
                &[],
                Some(Type::Bool),
                false,
            ),
            Self::RequestMusic => ("encounter::request_music", &[Type::I32], None, false),
            Self::PlayMusic => (
                "encounter::play_music",
                &[Type::I32, Type::I32],
                None,
                false,
            ),
            Self::StopMusic => ("encounter::stop_music", &[Type::I32], None, false),
            Self::PerformVictory => ("encounter::perform_victory", &[], None, false),
            Self::LevelNotices => ("encounter::level_notices", &[], None, false),
            Self::ExNotices => ("encounter::ex_notices", &[], None, false),
            Self::UpdateResults => ("encounter::update_results", &[Type::I32], None, false),
            Self::ResultCamera => ("encounter::result_camera", &[Type::I32], None, false),
            Self::DefeatNotice => ("encounter::defeat_notice", &[], None, false),
            Self::EscapeNotice => ("encounter::escape_notice", &[], None, false),
            Self::RecordEscape => ("encounter::record_escape", &[], None, false),
            Self::ForcedEscape => ("encounter::forced_escape", &[], Some(Type::Bool), false),
            Self::Finish => ("encounter::finish", &[], None, false),
        };
        NativeDeclaration {
            name,
            opcode: self as u8,
            parameters,
            result,
            suspends,
        }
    }
}

pub(super) struct EncounterHost<'a, S> {
    pub lifecycle: &'a mut Lifecycle,
    pub battle: &'a mut Battle,
    pub services: &'a mut S,
    pub input: &'a mut Option<Input>,
    pub confirm: bool,
    pub frame: &'a mut Option<BattleFrame>,
    pub cues: &'a mut Vec<Cue>,
    pub wait: &'a mut Option<Wait>,
    pub handle: i32,
}

fn continued() -> Result<NativeResult, String> {
    Ok(NativeResult::Continue(None))
}
fn boolean(value: bool) -> Result<NativeResult, String> {
    Ok(NativeResult::Continue(Some(i32::from(value))))
}
fn byte(value: i32) -> Result<u8, String> {
    u8::try_from(value).map_err(|_| "invalid transition byte".into())
}
fn short(value: i32) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| "invalid music value".into())
}
fn age(value: i32) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "invalid result age".into())
}

impl<S: Services> EncounterHost<'_, S> {
    fn query(&self, query: SelectionQuery) -> Result<NativeResult, String> {
        self.services
            .selection_query(query, self.battle)
            .map(|value| NativeResult::Continue(Some(value)))
            .map_err(|error| error.to_string())
    }
    fn request(&mut self, kind: RequestKind) -> Result<NativeResult, String> {
        let bit = match kind {
            RequestKind::SelectVictory { .. } => 1,
            RequestKind::AcceptVictory => 16,
            RequestKind::ConstructRewards => 2,
            RequestKind::RecoverTp => 4,
            RequestKind::RecordEscape => 8,
            _ => 0,
        };
        if self.lifecycle.once & bit != 0 {
            return Err("encounter operation was already acknowledged".into());
        }
        match kind {
            RequestKind::ConstructRewards if self.battle.phase() != BattlePhase::Results => {
                return Err("rewards require retired combat".into());
            }
            RequestKind::RecoverTp if self.lifecycle.once & 2 == 0 => {
                return Err("TP recovery precedes reward construction".into());
            }
            RequestKind::SelectVictory { .. }
            | RequestKind::AcceptVictory
            | RequestKind::ConstructRewards
            | RequestKind::RecoverTp
                if self.battle.recognize_result() != Some(BattleResult::Victory) =>
            {
                return Err("victory operation outside victory".into());
            }
            RequestKind::RecordEscape
                if self.battle.recognize_result() != Some(BattleResult::Escaped) =>
            {
                return Err("escape counter outside escape".into());
            }
            _ => {}
        }
        let request = Request {
            serial: self.lifecycle.next_request,
            kind,
        };
        let next = request
            .serial
            .checked_add(1)
            .ok_or("encounter request handles exhausted")?;
        let acknowledged = self
            .services
            .request(request, self.battle)
            .map_err(|error| error.to_string())?;
        if acknowledged.request != request {
            return Err("stale or mismatched encounter acknowledgement".into());
        }
        if let Some(frame) = self.frame.as_mut() {
            frame.cues.extend(acknowledged.cues);
        } else {
            self.cues.extend(acknowledged.cues);
        }
        self.lifecycle.next_request = next;
        self.lifecycle.once |= bit;
        continued()
    }
    fn transition(&mut self, kind: TransitionKind, args: &[i32]) -> Result<NativeResult, String> {
        self.battle
            .begin_transition(
                kind,
                [byte(args[0])?, byte(args[1])?, byte(args[2])?],
                byte(args[3])?,
            )
            .map_err(|error| error.to_string())?;
        continued()
    }
}

impl<S: Services> Host for EncounterHost<'_, S> {
    const AUTHORED_NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
        .register_typed(Native::Recognize.declaration(), |host, _, _| {
            let result = match host.battle.recognize_result() {
                None => 0,
                Some(BattleResult::Escaped) => 1,
                Some(BattleResult::Victory) => 2,
                Some(BattleResult::Defeat) => 3,
            };
            Ok(NativeResult::Continue(Some(result)))
        })
        .register_typed(Native::World.declaration(), |host, _, _| {
            let input = host
                .input
                .take()
                .ok_or("encounter visited the world twice in one update")?;
            let mut frame = host
                .battle
                .step(input.battle)
                .map_err(|error| error.to_string())?;
            host.services
                .after_world(&frame)
                .map_err(|error| error.to_string())?;
            host.cues.append(&mut frame.cues);
            frame.cues = std::mem::take(host.cues);
            *host.frame = Some(frame);
            continued()
        })
        .register_typed(Native::Next.declaration(), |host, _, _| {
            if host.frame.is_none() {
                return Err("encounter yielded without a world visit".into());
            }
            *host.wait = Some(Wait::Update);
            Ok(NativeResult::Suspend)
        })
        .register_typed(Native::SelectVictory.declaration(), |host, args, _| {
            host.request(RequestKind::SelectVictory {
                group: byte(args[0])?,
                pose: byte(args[1])?,
            })
        })
        .register_typed(Native::AcceptVictory.declaration(), |host, _, _| {
            host.request(RequestKind::AcceptVictory)
        })
        .register_typed(Native::Leader.declaration(), |host, _, _| {
            host.query(SelectionQuery::Leader)
        })
        .register_typed(Native::Available.declaration(), |host, args, _| {
            host.query(SelectionQuery::Available(byte(args[0])?))
        })
        .register_typed(Native::Dead.declaration(), |host, args, _| {
            host.query(SelectionQuery::Dead(byte(args[0])?))
        })
        .register_typed(Native::HpPercent.declaration(), |host, args, _| {
            host.query(SelectionQuery::HpPercent(byte(args[0])?))
        })
        .register_typed(Native::AffinityRank.declaration(), |host, args, _| {
            host.query(SelectionQuery::AffinityRank(byte(args[0])?))
        })
        .register_typed(Native::Participation.declaration(), |host, args, _| {
            host.query(SelectionQuery::Participation(byte(args[0])?))
        })
        .register_typed(Native::Poisoned.declaration(), |host, args, _| {
            host.query(SelectionQuery::Poisoned(byte(args[0])?))
        })
        .register_typed(Native::Story.declaration(), |host, _, _| {
            host.query(SelectionQuery::Story)
        })
        .register_typed(Native::PartyWasHit.declaration(), |host, _, _| {
            host.query(SelectionQuery::PartyWasHit)
        })
        .register_typed(Native::AllHealthy.declaration(), |host, _, _| {
            host.query(SelectionQuery::AllHealthy)
        })
        .register_typed(Native::Random.declaration(), |host, _, _| {
            Ok(NativeResult::Continue(Some(i32::from(
                host.battle.draw_random(),
            ))))
        })
        .register_typed(Native::DeathWaiting.declaration(), |host, _, _| {
            boolean(host.battle.victory_death_waiting())
        })
        .register_typed(Native::DeathReady.declaration(), |host, _, _| {
            boolean(
                host.battle
                    .victory_death_ready()
                    .map_err(|e| e.to_string())?,
            )
        })
        .register_typed(Native::VictoryBanner.declaration(), |host, _, _| {
            host.request(RequestKind::VictoryBanner)
        })
        .register_typed(Native::FadeOut.declaration(), |host, args, _| {
            host.transition(TransitionKind::FadeOut, args)
        })
        .register_typed(Native::FadeIn.declaration(), |host, args, _| {
            host.transition(TransitionKind::FadeIn, args)
        })
        .register_typed(Native::Wipe.declaration(), |host, args, _| {
            host.battle
                .begin_transition(TransitionKind::Wipe, [0; 3], byte(args[0])?)
                .map_err(|error| error.to_string())?;
            continued()
        })
        .register_typed(Native::TransitionStep.declaration(), |host, _, _| {
            boolean(
                host.battle
                    .advance_transition()
                    .map_err(|error| error.to_string())?,
            )
        })
        .register_typed(Native::Retire.declaration(), |host, _, _| {
            let cues = host
                .battle
                .retire_combat()
                .map_err(|error| error.to_string())?;
            host.frame
                .as_mut()
                .ok_or("combat retirement requires a world visit")?
                .cues
                .extend(cues);
            continued()
        })
        .register_typed(Native::ConstructRewards.declaration(), |host, _, _| {
            host.request(RequestKind::ConstructRewards)
        })
        .register_typed(Native::RecoverTp.declaration(), |host, _, _| {
            host.request(RequestKind::RecoverTp)
        })
        .register_typed(Native::Confirm.declaration(), |host, _, _| {
            boolean(host.confirm)
        })
        .register_typed(Native::MusicReady.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).music_ready)
        })
        .register_typed(Native::PerformanceReady.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).performance_ready)
        })
        .register_typed(Native::PerformanceFinished.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).performance_finished)
        })
        .register_typed(Native::ResultsReady.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).results_ready)
        })
        .register_typed(Native::HasExNotice.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).has_ex_notice)
        })
        .register_typed(Native::SuppressMusic.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).suppress_music)
        })
        .register_typed(Native::SuppressPerformance.declaration(), |host, _, _| {
            boolean(host.services.observations(host.battle).suppress_performance)
        })
        .register_typed(Native::RequestMusic.declaration(), |host, args, _| {
            host.request(RequestKind::RequestMusic {
                track: short(args[0])?,
            })
        })
        .register_typed(Native::PlayMusic.declaration(), |host, args, _| {
            host.request(RequestKind::PlayMusic {
                track: short(args[0])?,
                fade_ms: short(args[1])?,
            })
        })
        .register_typed(Native::StopMusic.declaration(), |host, args, _| {
            host.request(RequestKind::StopMusic {
                fade_ms: short(args[0])?,
            })
        })
        .register_typed(Native::PerformVictory.declaration(), |host, _, _| {
            host.request(RequestKind::PerformVictory)
        })
        .register_typed(Native::LevelNotices.declaration(), |host, _, _| {
            host.request(RequestKind::LevelNotices)
        })
        .register_typed(Native::ExNotices.declaration(), |host, _, _| {
            host.request(RequestKind::ExNotices)
        })
        .register_typed(Native::UpdateResults.declaration(), |host, args, _| {
            host.request(RequestKind::UpdateResults {
                age: age(args[0])?,
                confirm: host.confirm,
            })
        })
        .register_typed(Native::ResultCamera.declaration(), |host, args, _| {
            host.request(RequestKind::ResultCamera { age: age(args[0])? })
        })
        .register_typed(Native::DefeatNotice.declaration(), |host, _, _| {
            host.request(RequestKind::DefeatNotice)
        })
        .register_typed(Native::EscapeNotice.declaration(), |host, _, _| {
            host.request(RequestKind::EscapeNotice)
        })
        .register_typed(Native::RecordEscape.declaration(), |host, _, _| {
            host.request(RequestKind::RecordEscape)
        })
        .register_typed(Native::ForcedEscape.declaration(), |host, _, _| {
            boolean(host.battle.forced_escape())
        })
        .register_typed(Native::Finish.declaration(), |host, _, _| {
            if host.frame.is_some() {
                return Err("finalizer must have its own source visit".into());
            }
            if host.battle.recognize_result() == Some(BattleResult::Victory)
                && host.lifecycle.once & 22 != 22
            {
                return Err("victory finalization requires reward and TP acknowledgements".into());
            }
            *host.frame = Some(
                host.battle
                    .finish_result()
                    .map_err(|error| error.to_string())?,
            );
            host.lifecycle.finished = true;
            continued()
        });

    fn spawn(&mut self, function: u16, arguments: &[i32]) -> Result<i32, String> {
        if self.lifecycle.tasks.len() >= 63 {
            return Err("encounter task limit exceeded".into());
        }
        let function = self
            .lifecycle
            .program
            .authored()
            .and_then(|program| program.functions.get(usize::from(function)))
            .filter(|function| function.is_task)
            .ok_or("encounter spawn target is not a task")?;
        let vm = Vm::with_arguments(self.lifecycle.program.clone(), function.entry, arguments)
            .map_err(|error| error.to_string())?;
        let handle = self.lifecycle.next_task;
        let next = handle
            .checked_add(1)
            .ok_or("encounter task handles exhausted")?;
        self.lifecycle.ownership.register(self.handle, handle)?;
        self.lifecycle.next_task = next;
        self.lifecycle.tasks.insert(handle, Task { vm, wait: None });
        Ok(handle)
    }
    fn join(&mut self, child: i32) -> Result<Option<Vec<i32>>, String> {
        self.lifecycle.ownership.join(self.handle, child)
    }
}

pub fn native_declarations() -> Vec<NativeDeclaration> {
    struct DeclarationHost;
    impl Services for DeclarationHost {
        fn observations(&self, _: &Battle) -> Observations {
            Observations::default()
        }
        fn request(&mut self, _: Request, _: &mut Battle) -> Result<Acknowledgement> {
            anyhow::bail!("declarations do not execute")
        }
    }
    EncounterHost::<DeclarationHost>::AUTHORED_NATIVES
        .declarations()
        .collect()
}
