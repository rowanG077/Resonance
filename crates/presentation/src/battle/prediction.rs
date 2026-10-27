//! One isolated ordinary visit, cached until its source or latched input changes.
use super::*;
use crate::field_view::prediction::{Endpoint as CachedScene, Key, Prediction};

pub(crate) struct Endpoint {
    pub frame: BattleFrame,
    pub cover: Option<resonance_battle::TransitionFrame>,
    pub particles: Vec<resonance_battle::ParticleFrame>,
}

struct Presentation {
    last_page: bool,
}
impl results::Presentation for Presentation {
    fn results_on_last_page(&self) -> bool {
        self.last_page
    }
    fn request(
        &mut self,
        _: lifecycle::Request,
        _: Option<&results::Results>,
        _: &Battle,
    ) -> Result<()> {
        Ok(())
    }
}

pub(super) fn update(
    mut prediction: ResMut<Prediction>,
    owner: Option<Res<Owner>>,
    controls: Res<input::Controls>,
    testing: Option<Res<crate::testing::Controls>>,
) {
    let Some(owner) = owner.filter(|owner| owner.presenting()) else {
        return;
    };
    let Phase::Scene(scene) = &owner.phase else {
        return;
    };
    if prediction.alpha <= 0.
        || scene.state != SceneState::Active
        || scene.failure.is_some()
        || scene.completed.is_some()
        || scene.menu_backdrop.awaiting_draw()
        || !scene.menu_ready
        || scene.frame.is_none()
        || !scene.lifecycle.prediction_available()
    {
        return;
    }
    let Some(_) = &scene.candidate else {
        return;
    };
    let input = controls.clone().sample(&scene.settings);
    let instant_kill = testing.is_some_and(|testing| testing.instant_kill);
    let last_page = scene.hud.results_on_last_page();
    let key = Key::Battle {
        request: owner.request.id(),
        revision: scene.prediction_revision,
        tick: owner.input_tick,
        input,
        instant_kill,
        last_page,
    };
    if prediction.reuse(key) {
        return;
    }
    let _timer = crate::performance::smoothing_profile::Timer::start(0);
    let result = prediction
        .execute(|| predict(scene, &input, instant_kill, last_page))
        .and_then(|result| result);
    match result {
        Ok((endpoint, sources)) => prediction.publish(
            key,
            match endpoint {
                Some(endpoint) => CachedScene::Battle(endpoint, sources),
                None => CachedScene::Hold(sources),
            },
        ),
        Err(error) => prediction.report_error(&error),
    }
}

pub(super) fn predict(
    scene: &Scene,
    input: &input::Frame,
    instant_kill: bool,
    last_page: bool,
) -> Result<(Option<Endpoint>, resonance_events::OperationFork)> {
    let mut sources = resonance_events::OperationFork::default();
    let mut core = scene.core.clone();
    core.set_instant_kill(instant_kill);
    let mut lifecycle = scene.lifecycle.fork_for_prediction(&mut sources);
    let mut candidate = scene
        .candidate
        .as_ref()
        .context("battle result candidate was already consumed")?
        .clone();
    let mut feedback = scene.feedback.clone();
    let mut effects = scene.effects.clone();
    let mut event_models = scene.event_models.clone();
    let mut particles = Vec::with_capacity(scene.particles.len());
    let mut frame = Simulation {
        core: &mut core,
        lifecycle: &mut lifecycle,
        candidate: &mut candidate,
        feedback: &mut feedback,
        effects: &mut effects,
        event_models: event_models.as_mut(),
        particles: &mut particles,
    }
    .step(input, scene.frame.as_ref(), &mut Presentation { last_page })?;
    if frame.outcome.is_some() || lifecycle.event().is_some() != scene.lifecycle.event().is_some() {
        return Ok((None, sources));
    }
    if lifecycle.event().is_none() {
        let paused = frame.clock.feedback_paused();
        feedback.advance(&mut frame, paused, effects.actor_effect_ticks());
    }
    Ok((
        Some(Endpoint {
            cover: battle_cover(&frame, core.transition()),
            frame,
            particles,
        }),
        sources,
    ))
}
