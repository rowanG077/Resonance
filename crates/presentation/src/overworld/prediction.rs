use super::*;

#[allow(clippy::too_many_arguments)] // Scene endpoints and independent Bevy presentation resources.
pub(super) fn update(
    owner: State,
    art: Option<Res<Art>>,
    controls: Res<Controls>,
    menu_controls: Res<crate::field_view::Controls>,
    resident: Res<crate::loading::Resident>,
    parts: Query<&Instance>,
    mut prediction: ResMut<crate::field_view::Prediction>,
) {
    if prediction.alpha <= 0.
        || resident.battle.load(Ordering::Acquire)
        || owner.live.as_ref().is_some_and(|s| s.audio.is_some())
        || !resident.active.load(Ordering::Acquire)
        || art.is_none_or(|a| !a.ready || !a.spawned)
        || parts.iter().any(|part| !part.prepared)
    {
        return;
    }
    let Some(scene) = owner.get().filter(|_| owner.capture.is_none()) else {
        return;
    };
    if scene.session.events.battle_pending() {
        return;
    }
    let mut input = controls.0;
    input.menu = menu_controls.current();
    let key = crate::field_view::prediction::Key::World(
        owner.live.as_ref().unwrap().last_changed(),
        input,
    );
    if prediction.reuse(key) {
        return;
    }
    let _timer = crate::performance::smoothing_profile::Timer::start(0);
    let result = prediction
        .execute(|| {
            let mut fork = resonance_events::OperationFork::default();
            let mut next = Scene {
                session: scene.session.fork_for_prediction(&mut fork),
                package: scene.package.clone(),
                models: scene.models.clone(),
                landmarks: scene.landmarks.clone(),
                animation: scene.animation.clone(),
                effects: scene.effects.clone(),
            };
            next.step(input)?;
            Ok::<_, anyhow::Error>((
                (!next.session.events.battle_pending()
                    && next.session.events.world.field_transition.is_none()
                    && next.session.events.world.world_transition.is_none()
                    && next.session.world_destination().is_none())
                .then_some(next),
                fork,
            ))
        })
        .and_then(|r| r);
    match result {
        Ok((next, sources)) => prediction.publish(
            key,
            match next {
                Some(scene) => crate::field_view::prediction::Endpoint::World(scene, sources),
                None => crate::field_view::prediction::Endpoint::Hold(sources),
            },
        ),
        Err(error) => prediction.report_error(&error),
    }
}
