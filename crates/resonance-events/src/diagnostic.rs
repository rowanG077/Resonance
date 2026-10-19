//! Read-only snapshots for selected oracle captures. These are not loadable state.
use crate::operation::Wait;
use crate::{GameWorld, Operation, Outcome};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct EventInstanceObservation {
    pub slot: usize,
    pub handle: i32,
    pub key: Option<u32>,
    pub program_entry: u32,
    pub legacy_program: bool,
    pub pc: u32,
    pub legacy_return_stack: Vec<u32>,
    pub value_depth: usize,
    pub argument_depth: usize,
    pub expression: Option<i32>,
    pub join: Option<i32>,
    pub registers: [i32; 6],
    pub background: Option<BackgroundObservation>,
    pub wait: Option<WaitObservation>,
}

#[derive(Debug, Serialize)]
pub struct BackgroundObservation {
    pub paused: bool,
    pub require_control: bool,
}

#[derive(Debug, Serialize)]
pub struct OperationObservation {
    id: u64,
    ready: bool,
    position: u32,
    outcome: Option<OutcomeObservation>,
    /// Current retained dialogue owners with the same operation identity.
    /// This describes the observed wait rather than the encoded instruction operand.
    dialogue_slots: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OutcomeObservation {
    Completed { value: Option<i32> },
    Cancelled,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitObservation {
    Service {
        condition: Box<WaitObservation>,
        ready_at: Option<u32>,
    },
    Voice,
    SkitMedia {
        id: Option<u32>,
        position: Option<u32>,
    },
    Tick {
        wake: u32,
    },
    ControlHandoff {
        wake: u32,
    },
    ControlReleased,
    Camera {
        after: u32,
    },
    CameraPath {
        after: u32,
        channel: u8,
    },
    ActorMotion {
        actor: i32,
    },
    DespawnAfterMotion {
        actor: i32,
    },
    FaceAfterMotion {
        actor: i32,
        heading: f32,
    },
    ActorAnimationFrame {
        actor: i32,
        frame: i32,
    },
    ActorHeading {
        actor: i32,
    },
    ActorAnimation {
        actor: i32,
    },
    Complete {
        operation: OperationObservation,
    },
    Choice {
        result: OperationObservation,
        window: Box<WaitObservation>,
    },
    Menu {
        operation: OperationObservation,
    },
    Battle {
        operation: OperationObservation,
    },
    Ready {
        operation: OperationObservation,
    },
    Position {
        operation: OperationObservation,
        target: u32,
    },
}

fn operation(op: &Operation, world: &GameWorld) -> OperationObservation {
    let progress = op.progress();
    OperationObservation {
        id: op.id(),
        ready: progress.ready,
        position: progress.position,
        outcome: progress.outcome.map(|outcome| match outcome {
            Outcome::Completed(value) => OutcomeObservation::Completed { value },
            Outcome::Cancelled => OutcomeObservation::Cancelled,
        }),
        dialogue_slots: world
            .dialogue
            .iter()
            .filter_map(|(slot, dialogue)| (dialogue.operation.id() == op.id()).then_some(*slot))
            .collect(),
    }
}

/// Inspect the retained condition; deliberately never call Wait::poll.
pub(crate) fn wait(wait: &Wait, world: &GameWorld) -> WaitObservation {
    use WaitObservation as O;
    match wait {
        Wait::Service {
            condition,
            ready_at,
        } => O::Service {
            condition: Box::new(self::wait(condition, world)),
            ready_at: *ready_at,
        },
        Wait::Voice => O::Voice,
        Wait::SkitMedia { id, position } => O::SkitMedia {
            id: *id,
            position: *position,
        },
        Wait::Tick(wake) => O::Tick { wake: *wake },
        Wait::ControlHandoff(wake) => O::ControlHandoff { wake: *wake },
        Wait::ControlReleased => O::ControlReleased,
        Wait::Camera { after } => O::Camera { after: *after },
        Wait::CameraPath { after, channel } => O::CameraPath {
            after: *after,
            channel: *channel,
        },
        Wait::DespawnAfterMotion(actor) => O::DespawnAfterMotion { actor: *actor },
        Wait::FaceAfterMotion { actor, heading } => O::FaceAfterMotion {
            actor: *actor,
            heading: *heading,
        },
        Wait::ActorAnimationFrame(actor, frame) => O::ActorAnimationFrame {
            actor: *actor,
            frame: *frame,
        },
        Wait::ActorMotion(actor) => O::ActorMotion { actor: *actor },
        Wait::ActorHeading(actor) => O::ActorHeading { actor: *actor },
        Wait::ActorAnimation(actor) => O::ActorAnimation { actor: *actor },
        Wait::Complete(op) | Wait::Result(op) => O::Complete {
            operation: operation(op, world),
        },
        Wait::Choice { result, window } => O::Choice {
            result: operation(result, world),
            window: Box::new(self::wait(window, world)),
        },
        Wait::Menu(op) => O::Menu {
            operation: operation(op, world),
        },
        Wait::Battle(op) => O::Battle {
            operation: operation(op, world),
        },
        Wait::Ready(op) => O::Ready {
            operation: operation(op, world),
        },
        Wait::Position(op, target) => O::Position {
            operation: operation(op, world),
            target: *target,
        },
    }
}
