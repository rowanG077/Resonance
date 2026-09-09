//! Fail closed when the presentation layer drops a live visual request.
use super::field_view::{Art, State};
use anyhow::{Result, ensure};
use bevy::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Request {
    Actor(i32, usize),
    Animation {
        actor: i32,
        part: usize,
        resource: u32,
        slot: u16,
    },
    Attachment(i32, usize),
    Bone(i32, usize, u8),
    Mouth(i32),
    SecondaryMotion(i32, usize),
    Shadow(i32),
    Emote(i32),
    Billboard(i32),
    Particle(i32),
    Dialogue(u8),
    Choice(u8),
    Overlay(i32),
    Camera,
    Fade,
}

#[derive(Resource, Default)]
pub(super) struct Applied {
    pub requests: BTreeSet<Request>,
    loading: BTreeMap<Request, Instant>,
    armed: bool,
    expected: BTreeSet<Request>,
}
impl Applied {
    pub fn ack(&mut self, request: Request) {
        self.loading.remove(&request);
        self.requests.insert(request);
    }
    pub fn loading(&mut self, request: Request) {
        self.loading.entry(request).or_insert_with(Instant::now);
    }
}
pub(super) fn begin(state: State, art: Res<Art>, mut applied: ResMut<Applied>) {
    applied.requests.clear();
    applied.armed = art.ready
        && state
            .live
            .as_ref()
            .is_none_or(|s| s.ready_for_field && s.assets.map_id == art.map);
    applied.expected = expected(&state.get().events.world, |resource| {
        art.models.get(&resource).map_or(1, Vec::len)
    });
    for (&id, actor) in &state.get().events.world.actors {
        if actor.visible && !actor.appearance.model_hidden {
            for part in art.secondary_parts(actor.resource) {
                applied.expected.insert(Request::SecondaryMotion(id, part));
            }
            if actor.casts_shadow && state.get().ground_surface(actor.position).is_some() {
                applied.expected.insert(Request::Shadow(id));
            }
        }
        if actor.visible
            && !actor.appearance.model_hidden
            && (actor.appearance.mouth.is_some() || state.get().talking.contains_key(&id))
            && art.has_mouth(actor.resource)
        {
            applied.expected.insert(Request::Mouth(id));
        }
    }
    let expected = applied.expected.clone();
    applied
        .loading
        .retain(|request, _| expected.contains(request));
}
pub(super) fn actor_requests(
    id: i32,
    part: usize,
    actor: &resonance_events::Actor,
) -> impl Iterator<Item = Request> + '_ {
    std::iter::once(Request::Actor(id, part))
        .chain(actor.animation.iter().map(move |a| Request::Animation {
            actor: id,
            part,
            resource: a.resource,
            slot: a.slot,
        }))
        .chain(
            actor
                .attachment
                .iter()
                .map(move |_| Request::Attachment(id, part)),
        )
        .chain(
            actor
                .appearance
                .bone_adjustments
                .keys()
                .map(move |slot| Request::Bone(id, part, *slot)),
        )
}

fn expected(
    world: &resonance_events::GameWorld,
    parts: impl Fn(u32) -> usize,
) -> BTreeSet<Request> {
    let mut expected = BTreeSet::new();
    for (&id, actor) in &world.actors {
        if actor.visible && !actor.appearance.model_hidden {
            for part in 0..parts(actor.resource).max(1) {
                expected.extend(actor_requests(id, part, actor));
            }
        }
    }
    expected.extend(world.emotes.keys().map(|id| Request::Emote(*id)));
    expected.extend(world.billboards.keys().map(|id| Request::Billboard(*id)));
    expected.extend(world.particles.iter().map(|p| Request::Particle(p.handle)));
    expected.extend(
        world
            .dialogue
            .iter()
            .filter(|(_, d)| d.operation.is_pending())
            .map(|(slot, _)| Request::Dialogue(*slot)),
    );
    expected.extend(
        world
            .choices
            .iter()
            .filter(|(_, c)| c.operation.is_pending())
            .map(|(slot, _)| Request::Choice(*slot)),
    );
    expected.extend(world.overlays.keys().map(|id| Request::Overlay(*id)));
    if world.field_camera.is_some() {
        expected.insert(Request::Camera);
    }
    if world.fade.is_some() {
        expected.insert(Request::Fade);
    }
    expected
}
fn validate(tick: u32, applied: &Applied, now: Instant) -> Result<()> {
    let missing: Vec<_> = applied
        .expected
        .difference(&applied.requests)
        .filter(|r| {
            applied
                .loading
                .get(*r)
                .is_none_or(|since| now.saturating_duration_since(*since) >= LOAD_TIMEOUT)
        })
        .collect();
    ensure!(
        missing.is_empty(),
        "unapplied VM presentation requests at tick {tick}: {missing:?}"
    );
    Ok(())
}
pub(super) fn check(state: State, applied: Res<Applied>) {
    if !cfg!(debug_assertions) || !applied.armed {
        return;
    }
    assert_applied(state.get().events.tick(), &applied);
}
fn assert_applied(tick: u32, applied: &Applied) {
    let result = validate(tick, applied, Instant::now());
    debug_assert!(
        result.is_ok(),
        "Field presentation contract failed: {}",
        result.unwrap_err()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "unapplied VM presentation requests at tick 549: [Billboard(8)]")]
    fn missing_effect_triggers_the_debug_assertion() {
        let mut applied = Applied::default();
        applied.expected.insert(Request::Billboard(8));
        assert_applied(549, &applied);
    }
    #[test]
    fn dropped_requests_fail_even_when_other_actors_are_present() {
        let mut world = resonance_events::GameWorld::default();
        world
            .actors
            .insert(1, resonance_events::Actor::new(1, [0.; 3]));
        world.emotes.insert(
            -100,
            resonance_events::Emote {
                actor: 1,
                kind: 4,
                offset: [0.; 3],
                start_tick: 0,
                duration: None,
            },
        );
        let mut applied = Applied {
            expected: expected(&world, |_| 1),
            ..Default::default()
        };
        applied.ack(Request::Actor(1, 0));
        assert!(
            validate(0, &applied, Instant::now())
                .unwrap_err()
                .to_string()
                .contains("Emote(-100)")
        );
        applied.requests.insert(Request::Emote(-100));
        validate(0, &applied, Instant::now()).unwrap();
        world.actors.get_mut(&1).unwrap().visible = false;
        applied.requests.remove(&Request::Actor(1, 0));
        applied.expected = expected(&world, |_| 1);
        validate(0, &applied, Instant::now()).unwrap();
    }
    #[test]
    fn loading_is_bounded_and_acknowledgements_expire_each_frame() {
        let mut applied = Applied::default();
        let request = Request::Attachment(1, 0);
        applied.expected.insert(request.clone());
        applied.loading(request.clone());
        let since = applied.loading[&request];
        validate(4, &applied, since).unwrap();
        assert!(validate(4, &applied, since + LOAD_TIMEOUT).is_err());
        applied.ack(request);
        validate(4, &applied, since + LOAD_TIMEOUT).unwrap();
        applied.requests.clear();
        assert!(validate(5, &applied, since + LOAD_TIMEOUT).is_err());
    }
    #[test]
    fn each_part_needs_its_requested_animation() {
        let mut world = resonance_events::GameWorld::default();
        let mut actor = resonance_events::Actor::new(7, [0.; 3]);
        actor.animation = Some(resonance_events::Animation {
            repeat: false,
            ..resonance_events::Animation::new(24, 6, 10, 0)
        });
        world.actors.insert(2, actor);
        let mut applied = Applied {
            expected: expected(&world, |_| 2),
            ..Default::default()
        };
        for part in 0..2 {
            applied.ack(Request::Actor(2, part));
        }
        applied.ack(Request::Animation {
            actor: 2,
            part: 0,
            resource: 24,
            slot: 6,
        });
        let error = validate(12, &applied, Instant::now())
            .unwrap_err()
            .to_string();
        assert!(error.contains("part: 1, resource: 24, slot: 6"), "{error}");
    }
}
