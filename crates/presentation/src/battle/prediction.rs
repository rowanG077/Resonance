//! One isolated ordinary visit, cached until its source or latched input changes.
use super::*;
use crate::field_view::prediction::{Endpoint as CachedScene, Key, Prediction};

#[derive(Default)]
pub(crate) struct Endpoint {
    pub frame: BattleFrame,
    pub menu: Option<Box<resonance_game::battle::command::Frame>>,
    pub event: Option<Box<resonance_game::battle::event::Playback>>,
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
    playback: Option<Res<crate::battle_audio::Playback>>,
    transition: Option<Res<super::transition::State>>,
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
    let last_page = scene.hud.results_on_last_page();
    let voice_busy = playback.as_ref().map_or_default(|p| p.busy_voices());
    let entry_voice_ready = transition.as_ref().is_none_or(|t| t.entry_voice_ready());
    let key = Key::Battle {
        request: owner.request.id(),
        revision: scene.prediction_revision,
        tick: owner.input_tick,
        input: input.clone(),
        last_page,
        voice_busy,
        entry_voice_ready,
    };
    if prediction.reuse(&key) {
        return;
    }
    let _timer = crate::performance::smoothing_profile::Timer::start(0);
    prediction.predict(key, || {
        let (endpoint, sources) = predict(scene, &input, last_page, voice_busy, entry_voice_ready)?;
        Ok(match endpoint {
            Some(endpoint) => CachedScene::Battle(endpoint, sources),
            None => CachedScene::Hold(sources),
        })
    });
}

pub(super) fn predict(
    scene: &Scene,
    input: &input::Frame,
    last_page: bool,
    voice_busy: resonance_battle::ActorSet,
    entry_voice_ready: bool,
) -> Result<(Option<Endpoint>, resonance_events::OperationFork)> {
    let mut sources = resonance_events::OperationFork::default();
    let mut core = scene.core.clone();
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
    let had_event = lifecycle.event().is_some();
    let mut frame = Simulation {
        core: &mut core,
        lifecycle: &mut lifecycle,
        candidate: &mut candidate,
        feedback: &mut feedback,
        effects: &mut effects,
        event_models: event_models.as_mut(),
        particles: &mut particles,
    }
    .step(
        input,
        scene.frame.as_ref(),
        &mut Presentation { last_page },
        voice_busy,
        entry_voice_ready,
    )?;
    if frame.outcome.is_some() || lifecycle.event().is_some() != had_event {
        return Ok((None, sources));
    }
    if lifecycle.event().is_none() {
        let paused = frame.clock.feedback_paused();
        feedback.advance(&mut frame, paused, effects.actor_effect_ticks());
    }
    Ok((
        Some(Endpoint {
            menu: (lifecycle.command_input_kind()
                == resonance_game::battle::command::InputKind::SharedMenu
                && lifecycle.equipment_character() == scene.lifecycle.equipment_character())
            .then(|| lifecycle.command_frame())
            .flatten()
            .map(Box::new),
            event: lifecycle.into_event().map(Box::new),
            cover: battle_cover(&frame, core.transition()),
            frame,
            particles,
        }),
        sources,
    ))
}
