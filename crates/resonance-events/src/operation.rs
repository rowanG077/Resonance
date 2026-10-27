//! Completion tokens shared with presentation and other game services.
//! Dropping a scene cancels its outstanding work; late callbacks cannot complete
//! an operation belonging to a different scene or a reused dialogue slot.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed(Option<i32>),
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub ready: bool,
    pub position: u32,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone)]
pub struct Operation {
    id: u64,
    state: Arc<Mutex<Progress>>,
}

impl Operation {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn is_pending(&self) -> bool {
        self.progress().outcome.is_none()
    }
    pub fn progress(&self) -> Progress {
        *self.state.lock().expect("operation state poisoned")
    }
    pub fn advance(&self, position: u32) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "operation state poisoned")?;
        if state.outcome.is_some() || position < state.position {
            return Err("cannot advance a completed or rewound operation".into());
        }
        state.ready = true;
        state.position = position;
        Ok(())
    }
    pub fn complete(&self, value: Option<i32>) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| "operation state poisoned")?;
        if state.outcome.is_some() {
            return Err("operation already completed or cancelled".into());
        }
        state.outcome = Some(Outcome::Completed(value));
        Ok(())
    }
    pub fn cancel(&self) {
        let mut state = self.state.lock().expect("operation state poisoned");
        if state.outcome.is_none() {
            state.outcome = Some(Outcome::Cancelled);
        }
    }
}

#[derive(Default)]
pub(crate) struct OperationScope {
    active: Vec<Weak<Mutex<Progress>>>,
    // A prediction allocates IDs locally, without advancing the live allocator.
    prediction_ids: Option<Arc<AtomicU64>>,
}
impl OperationScope {
    pub(crate) fn inherit_allocator(&mut self, source: &Self) {
        self.prediction_ids.clone_from(&source.prediction_ids);
    }

    pub(crate) fn fork(&self, fork: &mut OperationFork) -> Self {
        Self {
            active: self
                .active
                .iter()
                .filter_map(Weak::upgrade)
                .map(|state| Arc::downgrade(&fork.state(&state)))
                .collect(),
            prediction_ids: Some(fork.ids.clone()),
        }
    }
    /// Also retain cancellation ownership for an operation published by a game service.
    pub fn track(&mut self, operation: &Operation) -> Result<(), String> {
        self.active.retain(|task| task.strong_count() != 0);
        if self.active.len() >= 4096 {
            return Err("too many retained game operations".into());
        }
        self.active.push(Arc::downgrade(&operation.state));
        Ok(())
    }
    pub fn begin(&mut self) -> Result<Operation, String> {
        let id = self
            .prediction_ids
            .as_deref()
            .unwrap_or(&NEXT_ID)
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "operation ID exhausted")?;
        let state = Arc::new(Mutex::new(Progress::default()));
        let operation = Operation { id, state };
        self.track(&operation)?;
        Ok(operation)
    }
    pub fn cancel(&mut self) {
        for state in self.active.drain(..).filter_map(|w| w.upgrade()) {
            let mut state = state.lock().expect("operation state poisoned");
            if state.outcome.is_none() {
                state.outcome = Some(Outcome::Cancelled);
            }
        }
    }
}

/// Copies an operation graph while preserving aliases *within* a prediction.
/// Never share this context between independent predictions. Immutable assets
/// can be shared, but completion/cancellation state must belong to the branch.
pub struct OperationFork {
    states: std::collections::BTreeMap<usize, ForkedProgress>,
    flags: Vec<(Weak<AtomicBool>, bool, Arc<AtomicBool>)>,
    ids: Arc<AtomicU64>,
}
struct ForkedProgress {
    source: Weak<Mutex<Progress>>,
    observed: Progress,
    branch: Arc<Mutex<Progress>>,
}
impl Default for OperationFork {
    fn default() -> Self {
        Self {
            states: Default::default(),
            flags: Vec::new(),
            ids: Arc::new(AtomicU64::new(NEXT_ID.load(Ordering::Relaxed))),
        }
    }
}
impl OperationFork {
    fn state(&mut self, source: &Arc<Mutex<Progress>>) -> Arc<Mutex<Progress>> {
        self.states
            .entry(Arc::as_ptr(source) as usize)
            .or_insert_with(|| {
                let observed = *source.lock().expect("operation state poisoned");
                ForkedProgress {
                    source: Arc::downgrade(source),
                    observed,
                    branch: Arc::new(Mutex::new(observed)),
                }
            })
            .branch
            .clone()
    }

    /// Service callbacks can mutate operations without touching the owning
    /// session. A cached prediction is valid only while these inputs match the
    /// values observed when it was forked; changes inside the branch don't count.
    pub fn sources_unchanged(&self) -> bool {
        self.states.values().all(|state| {
            state.source.upgrade().is_some_and(|source| {
                *source.lock().expect("operation state poisoned") == state.observed
            })
        }) && self.flags.iter().all(|(source, observed, _)| {
            source
                .upgrade()
                .is_some_and(|source| source.load(Ordering::Acquire) == *observed)
        })
    }

    /// Snapshot asynchronous voice completion, preserving branch-local aliases.
    pub fn fork_flag(&mut self, source: &Arc<AtomicBool>) -> Arc<AtomicBool> {
        if let Some((_, _, branch)) = self
            .flags
            .iter()
            .find(|(weak, _, _)| weak.as_ptr() == Arc::as_ptr(source))
        {
            return branch.clone();
        }
        let observed = source.load(Ordering::Acquire);
        let branch = Arc::new(AtomicBool::new(observed));
        self.flags
            .push((Arc::downgrade(source), observed, branch.clone()));
        branch
    }

    pub fn fork(&mut self, source: &Operation) -> Operation {
        self.ids
            .fetch_max(source.id.saturating_add(1), Ordering::Relaxed);
        Operation {
            id: source.id,
            state: self.state(&source.state),
        }
    }
}
impl Drop for OperationScope {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Wait {
    /// A satisfied service wait resumes its script on the following update.
    Service {
        condition: Box<Wait>,
        ready_at: Option<u32>,
    },
    Voice,
    SkitMedia {
        id: Option<u32>,
        position: Option<u32>,
    },
    Tick(u32),
    ControlHandoff(u32),
    /// Input is released; the caller resumes at the next dispatcher update.
    ControlReleased,
    Camera {
        after: u32,
    },
    CameraPath {
        after: u32,
        channel: u8,
    },
    ActorMotion(i32),
    DespawnAfterMotion(i32),
    ActorHeading(i32),
    FaceAfterMotion {
        actor: i32,
        heading: f32,
    },
    ActorAnimation(i32),
    ActorAnimationFrame(i32, i32),
    Complete(Operation),
    Result(Operation),
    Choice {
        result: Operation,
        window: Box<Wait>,
    },
    Menu(Operation),
    Battle(Operation),
    Ready(Operation),
    Position(Operation, u32),
}
impl Wait {
    pub(crate) fn fork(&self, fork: &mut OperationFork) -> Self {
        match self {
            Self::Service {
                condition,
                ready_at,
            } => Self::Service {
                condition: Box::new(condition.fork(fork)),
                ready_at: *ready_at,
            },
            Self::Choice { result, window } => Self::Choice {
                result: fork.fork(result),
                window: Box::new(window.fork(fork)),
            },
            Self::Complete(op) => Self::Complete(fork.fork(op)),
            Self::Result(op) => Self::Result(fork.fork(op)),
            Self::Menu(op) => Self::Menu(fork.fork(op)),
            Self::Battle(op) => Self::Battle(fork.fork(op)),
            Self::Ready(op) => Self::Ready(fork.fork(op)),
            Self::Position(op, position) => Self::Position(fork.fork(op), *position),
            Self::Voice
            | Self::SkitMedia { .. }
            | Self::Tick(_)
            | Self::ControlHandoff(_)
            | Self::ControlReleased
            | Self::Camera { .. }
            | Self::CameraPath { .. }
            | Self::ActorMotion(_)
            | Self::DespawnAfterMotion(_)
            | Self::ActorHeading(_)
            | Self::FaceAfterMotion { .. }
            | Self::ActorAnimation(_)
            | Self::ActorAnimationFrame(_, _) => self.clone(),
        }
    }

    pub fn track(&self, scope: &mut OperationScope) -> Result<(), String> {
        match self {
            Self::Service { condition, .. } => condition.track(scope),
            Self::Choice { result, window } => {
                scope.track(result)?;
                window.track(scope)
            }
            Self::Complete(op)
            | Self::Result(op)
            | Self::Battle(op)
            | Self::Menu(op)
            | Self::Ready(op)
            | Self::Position(op, _) => scope.track(op),
            _ => Ok(()),
        }
    }
    pub fn poll(&mut self, world: &mut crate::GameWorld) -> Result<bool, String> {
        let operation = match self {
            Self::Service {
                condition,
                ready_at,
            } => {
                if let Some(ready) = *ready_at {
                    return Ok(world.tick > ready);
                }
                if condition.poll(world)? {
                    *ready_at = Some(world.tick);
                }
                return Ok(false);
            }
            Self::ControlReleased => return Ok(true),
            Self::SkitMedia { id, position } => {
                let scene = world
                    .skit
                    .as_ref()
                    .ok_or("skit media wait outside a skit")?;
                return Ok(if let Some(id) = id {
                    scene.media.as_ref().is_some_and(|m| m.id == *id)
                } else {
                    scene.media.as_ref().is_none_or(|m| {
                        m.finished(world.tick)
                            || position.is_some_and(|p| m.position(world.tick) >= p)
                    })
                });
            }
            Self::Voice => {
                return Ok(world
                    .voice
                    .as_ref()
                    .is_none_or(|v| world.tick >= v.end_tick));
            }
            Self::ActorAnimation(id) => {
                return Ok(world
                    .actors
                    .get(id)
                    .and_then(|a| a.animation.as_ref())
                    .is_none_or(|a| {
                        let frame = a.elapsed(world.tick, 0);
                        if a.script_rate() < 0. {
                            frame <= 0.
                        } else {
                            frame >= a.duration_ticks as f32
                        }
                    }));
            }
            Self::ActorAnimationFrame(id, frame) => {
                // Native model time is in 30-Hz authored frames; cooked poses
                // use 60-Hz ticks. This wait tests the integer frame, including
                // a paused final frame, rather than waiting for clip completion.
                return Ok(world
                    .actors
                    .get(id)
                    .and_then(|a| a.animation.as_ref())
                    .is_some_and(|a| {
                        (a.sample(world.tick, 0, a.duration_ticks as f32) / 2.) as i32 == *frame
                    }));
            }
            Self::ActorMotion(id) => {
                return Ok(world.actors.get(id).is_none_or(|a| a.motion.is_none()));
            }
            Self::DespawnAfterMotion(id) => {
                if world.actors.get(id).is_some_and(|a| a.motion.is_some()) {
                    return Ok(false);
                }
                world.remove_actor(*id);
                return Ok(true);
            }
            Self::FaceAfterMotion { actor, heading } => {
                let Some(actor) = world.actors.get_mut(actor) else {
                    return Ok(true);
                };
                if actor.motion.is_some() {
                    return Ok(false);
                }
                actor.target_heading = *heading;
                return Ok((actor.heading - *heading + 180.).rem_euclid(360.) - 180. == 0.);
            }
            Self::ActorHeading(id) => {
                return Ok(world
                    .actors
                    .get(id)
                    .is_none_or(|a| (a.target_heading - a.heading).abs() < 0.01));
            }
            Self::Tick(wake) | Self::ControlHandoff(wake) => {
                return Ok(world.tick >= *wake);
            }
            Self::Camera { after } => {
                return Ok(world.tick > *after
                    && world
                        .field_camera
                        .as_ref()
                        .is_none_or(|camera| camera.settled()));
            }
            Self::CameraPath { after, channel } => {
                return Ok(world.tick > *after
                    && world
                        .field_camera
                        .as_ref()
                        .and_then(|rig| rig.motion.as_ref())
                        .is_none_or(|m| m.settled(*channel)));
            }
            Self::Choice { result, .. } => result,
            Self::Complete(op)
            | Self::Result(op)
            | Self::Battle(op)
            | Self::Menu(op)
            | Self::Ready(op)
            | Self::Position(op, _) => op,
        };
        let progress = operation.progress();
        match progress.outcome {
            Some(Outcome::Cancelled) => Err(format!("operation {} was cancelled", operation.id())),
            Some(Outcome::Completed(_)) => match self {
                Self::Choice { window, .. } => window.poll(world),
                _ => Ok(true),
            },
            None => Ok(match self {
                Self::Complete(_)
                | Self::Result(_)
                | Self::Choice { .. }
                | Self::Menu(_)
                | Self::Battle(_) => false,
                Self::Ready(_) => progress.ready,
                Self::Position(_, target) => progress.ready && progress.position >= *target,
                _ => unreachable!("non-operation waits returned above"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_acknowledgements_are_isolated_and_invalidate_the_cache() {
        let live = Arc::new(AtomicBool::new(false));
        let mut fork = OperationFork::default();
        let branch = fork.fork_flag(&live);
        assert!(Arc::ptr_eq(&branch, &fork.fork_flag(&live)));
        branch.store(true, Ordering::Release);
        assert!(!live.load(Ordering::Acquire));
        assert!(fork.sources_unchanged());
        live.store(true, Ordering::Release);
        assert!(!fork.sources_unchanged());
    }

    #[test]
    fn prediction_preserves_aliases_without_completing_or_cancelling_live_operations() {
        let mut live = OperationScope::default();
        let source = live.begin().unwrap();
        source.advance(7).unwrap();
        let mut fork = OperationFork::default();
        let mut branch_scope = live.fork(&mut fork);
        let first = fork.fork(&source);
        let alias = fork.fork(&source.clone());
        drop(fork);
        first.complete(Some(42)).unwrap();
        assert_eq!(alias.progress().outcome, Some(Outcome::Completed(Some(42))));
        assert_eq!(source.progress().outcome, None);
        assert_eq!(source.progress().position, 7);
        let created = branch_scope.begin().unwrap();
        drop(branch_scope);
        assert_eq!(created.progress().outcome, Some(Outcome::Cancelled));
        assert!(source.is_pending());
        drop(live);
        assert_eq!(source.progress().outcome, Some(Outcome::Cancelled));
    }

    #[test]
    fn cached_forks_detect_service_updates_even_without_a_simulation_step() {
        let mut scope = OperationScope::default();
        let source = scope.begin().unwrap();
        for position in [0, 4, 8] {
            let mut fork = OperationFork::default();
            let copy = fork.fork(&source);
            assert!(fork.sources_unchanged());
            copy.cancel();
            assert!(fork.sources_unchanged());
            source.advance(position).unwrap();
            assert!(!fork.sources_unchanged());
        }
        let mut fork = OperationFork::default();
        let _copy = fork.fork(&source);
        source.complete(None).unwrap();
        assert!(!fork.sources_unchanged());

        let pending = scope.begin().unwrap();
        let mut fork = OperationFork::default();
        let _copy = fork.fork(&pending);
        drop(scope);
        assert!(!fork.sources_unchanged());
    }

    #[test]
    fn cached_fork_does_not_keep_a_live_operation_pending() {
        let mut scope = OperationScope::default();
        let source = scope.begin().unwrap();
        let mut fork = OperationFork::default();
        let _copy = fork.fork(&source);
        drop(source);
        assert_eq!(scope.active[0].strong_count(), 0);
        assert!(!fork.sources_unchanged());
    }

    #[test]
    fn discarded_prediction_does_not_cancel_a_pending_live_wait() {
        let mut live = OperationScope::default();
        let source = live.begin().unwrap();
        let mut fork = OperationFork::default();
        let scope = live.fork(&mut fork);
        let copy = fork.fork(&source);
        drop(fork);
        drop(scope);
        assert_eq!(copy.progress().outcome, Some(Outcome::Cancelled));
        assert!(source.is_pending());
    }
    #[test]
    fn deferred_facing_preserves_movement_then_waits_for_the_turn() {
        let mut world = crate::GameWorld::default();
        let mut actor = crate::Actor::new(1, [0.; 3]);
        actor.motion = Some(crate::ActorMotion {
            target: [10., 0., 0.],
            speed: 2.,
        });
        world.actors.insert(1, actor);
        let mut wait = Wait::FaceAfterMotion {
            actor: 1,
            heading: 135.,
        };
        assert!(!wait.poll(&mut world).unwrap());
        assert_eq!(world.actors[&1].target_heading, 0.);
        world.actors.get_mut(&1).unwrap().motion = None;
        assert!(!wait.poll(&mut world).unwrap());
        assert_eq!(world.actors[&1].target_heading, 135.);
        world.actors.get_mut(&1).unwrap().heading = 135.;
        assert!(wait.poll(&mut world).unwrap());
    }
    #[test]
    fn observed_service_completion_survives_a_later_state_change() {
        let mut world = crate::GameWorld::default();
        let mut wait = Wait::Service {
            condition: Box::new(Wait::Voice),
            ready_at: None,
        };
        assert!(!wait.poll(&mut world).unwrap());
        // Another caller can start a voice after this caller's wait cleared.
        // That must not re-block an already completed service command.
        world.voice = Some(crate::VoicePlayback {
            resource: 7,
            end_tick: 100,
        });
        assert!(!wait.poll(&mut world).unwrap());
        world.tick += 1;
        assert!(wait.poll(&mut world).unwrap());
    }

    #[test]
    fn scene_exit_cancels_live_callbacks_without_reusing_tokens() {
        let mut first = OperationScope::default();
        let old = first.begin().unwrap();
        old.advance(4).unwrap();
        drop(first);
        assert!(old.complete(None).is_err());
        let mut second = OperationScope::default();
        let new = second.begin().unwrap();
        assert_ne!(old.id(), new.id());
        assert_eq!(new.progress().outcome, None);
        assert!(
            Wait::Ready(old)
                .poll(&mut crate::GameWorld::default())
                .is_err()
        );
        new.complete(Some(7)).unwrap();
        assert!(new.complete(None).is_err());
        assert_eq!(new.progress().outcome, Some(Outcome::Completed(Some(7))));
    }
}
