//! Scene fades run independently of destination preparation.
use crate::transition_metrics::Place;
use bevy::prelude::*;

const FADE_TICKS: u32 = 15;

#[derive(Resource)]
pub(crate) struct Fade {
    phase: Phase,
    white: bool,
    destination: Place,
}

enum Phase {
    Out(u32, f32),
    Covered,
    In(u32),
}

impl Fade {
    fn opacity(&self, tick: u32) -> f32 {
        let progress = |start| tick.wrapping_sub(start).min(FADE_TICKS) as f32 / FADE_TICKS as f32;
        match self.phase {
            Phase::Out(start, from) => from + (1. - from) * progress(start),
            Phase::Covered => 1.,
            Phase::In(start) => 1. - progress(start),
        }
    }
}

pub(super) fn begin(world: &mut World) {
    let session = world.resource::<super::Session>();
    if !session.is_field()
        || !session
            .events()
            .world
            .world_transition
            .as_ref()
            .is_some_and(|request| request.following.is_none())
    {
        return;
    }
    if world
        .get_resource::<Fade>()
        .is_some_and(|fade| fade.destination == Place::World && !matches!(fade.phase, Phase::In(_)))
    {
        return;
    }
    let tick = world.resource::<crate::Clock>().0.tick();
    let from = world
        .get_resource::<Fade>()
        .filter(|fade| fade.destination == Place::Field(session.map_id()))
        .map_or(0., |fade| fade.opacity(tick));
    let source = &session.events().world;
    let phase = if source.brightness() == 0.
        || from == 1.
        || !world
            .resource::<crate::loading::Resident>()
            .active
            .load(std::sync::atomic::Ordering::Acquire)
    {
        Phase::Covered
    } else {
        Phase::Out(tick, from)
    };
    let white = source.fade.as_ref().is_some_and(|fade| fade.white);
    world.insert_resource(Fade {
        phase,
        white,
        destination: Place::World,
    });
}

pub(crate) fn loaded(world: &mut World) {
    if let Some(destination) = Place::current(world) {
        world.insert_resource(Fade {
            phase: Phase::Covered,
            white: false,
            destination,
        });
    }
}

/// The source remains presentable while its captured destination prepares.
pub(crate) fn outgoing(world: &World) -> bool {
    world.get_resource::<Fade>().is_some_and(|fade| {
        matches!(fade.phase, Phase::Out(..))
            && fade.opacity(world.resource::<crate::Clock>().0.tick()) < 1.
    })
}

pub(super) fn cover(world: &mut World) {
    if let Some(mut fade) = world.get_resource_mut::<Fade>() {
        fade.phase = Phase::Covered;
    }
}

/// Apply after the scene and loading cover have published their output values.
pub(crate) fn update(world: &mut World) {
    let Some(mut fade) = world.remove_resource::<Fade>() else {
        return;
    };
    let arrived = Place::current(world) == Some(fade.destination);
    let cancelled = world
        .get_resource::<super::Session>()
        .is_none_or(|session| {
            if let Some(scene) = session.overworld() {
                !arrived || scene.session.cinematic.is_some() || scene.session.portal().is_some()
            } else {
                !arrived
                    && (fade.destination != Place::World
                        || session.events().world.world_transition.is_none())
            }
        })
        || world.contains_resource::<crate::battle::Owner>()
        || world.contains_resource::<super::TransitionFailure>()
        || world.contains_resource::<crate::loading::Pending>();
    if cancelled {
        return;
    }
    let tick = world.resource::<crate::Clock>().0.tick();
    if matches!(fade.phase, Phase::Covered)
        && arrived
        && world.resource::<super::Session>().audio.is_none()
        && crate::saves::scene_prepared(world)
    {
        fade.phase = Phase::In(tick);
    }
    let alpha = fade.opacity(tick);
    crate::materials::TitleOutput::update(
        &mut world.resource_mut::<Assets<crate::materials::TitleOutput>>(),
        |brightness| {
            brightness.x *= 1. - alpha;
            brightness.y = brightness.y * (1. - alpha) + f32::from(fade.white) * alpha;
            brightness.z = 0.;
        },
    );
    if !matches!(fade.phase, Phase::In(_)) || alpha != 0. {
        world.insert_resource(fade);
    }
}
