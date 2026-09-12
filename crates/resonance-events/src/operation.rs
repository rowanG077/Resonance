//! Completion tokens shared with presentation and other game services.
//! Dropping a scene cancels its outstanding work; late callbacks cannot complete
//! an operation belonging to a different scene or a reused dialogue slot.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed(Option<i32>),
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default)]
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
}
impl OperationScope {
    pub fn begin(&mut self) -> Result<Operation, String> {
        self.active.retain(|task| task.strong_count() != 0);
        if self.active.len() >= 4096 {
            return Err("too many retained game operations".into());
        }
        let id = NEXT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "operation ID exhausted")?;
        let state = Arc::new(Mutex::new(Progress::default()));
        self.active.push(Arc::downgrade(&state));
        Ok(Operation { id, state })
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
    ActorHeading(i32),
    ActorAnimation(i32),
    Complete(Operation),
    Choice {
        result: Operation,
        window: Box<Wait>,
    },
    Menu(Operation),
    Ready(Operation),
    Position(Operation, u32),
}
impl Wait {
    pub fn poll(&mut self, world: &crate::GameWorld) -> Result<bool, String> {
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
                    .is_none_or(|a| a.elapsed(world.tick, 0) >= a.duration_ticks as f32));
            }
            Self::ActorMotion(id) => {
                return Ok(world.actors.get(id).is_none_or(|a| a.motion.is_none()));
            }
            Self::ActorHeading(id) => {
                return Ok(world
                    .actors
                    .get(id)
                    .is_none_or(|a| (a.target_heading - a.heading).abs() < 0.01));
            }
            Self::Tick(wake) | Self::ControlHandoff(wake) => return Ok(world.tick >= *wake),
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
            Self::Complete(op) | Self::Menu(op) | Self::Ready(op) | Self::Position(op, _) => op,
        };
        let progress = operation.progress();
        match progress.outcome {
            Some(Outcome::Cancelled) => Err(format!("operation {} was cancelled", operation.id())),
            Some(Outcome::Completed(_)) => match self {
                Self::Choice { window, .. } => window.poll(world),
                _ => Ok(true),
            },
            None => Ok(match self {
                Self::Complete(_) | Self::Choice { .. } | Self::Menu(_) => false,
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
    fn observed_service_completion_survives_a_later_state_change() {
        let mut world = crate::GameWorld::default();
        let mut wait = Wait::Service {
            condition: Box::new(Wait::Voice),
            ready_at: None,
        };
        assert!(!wait.poll(&world).unwrap());
        // Another caller can start a voice after this caller's wait cleared.
        // That must not re-block an already completed service command.
        world.voice = Some(crate::VoicePlayback {
            resource: 7,
            end_tick: 100,
        });
        assert!(!wait.poll(&world).unwrap());
        world.tick += 1;
        assert!(wait.poll(&world).unwrap());
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
        assert!(Wait::Ready(old).poll(&crate::GameWorld::default()).is_err());
        new.complete(Some(7)).unwrap();
        assert!(new.complete(None).is_err());
        assert_eq!(new.progress().outcome, Some(Outcome::Completed(Some(7))));
    }
}
