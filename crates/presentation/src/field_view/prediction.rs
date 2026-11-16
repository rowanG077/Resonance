//! Retain the next endpoint while its committed source and inputs are unchanged.
//! It is never promoted, dispatched to services, or used as simulation input.
use crate::sparse_animation::affine::PoseEndpoint;
use bevy::prelude::*;
use resonance_events::camera::CameraRig;
use resonance_game::field::FieldPrediction;

#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "Battle input is compared on every render frame; keep it inline to avoid a per-frame allocation."
)]
pub(crate) enum Key {
    Battle {
        request: u64,
        revision: u64,
        tick: u32,
        input: crate::battle::input::Frame,
        last_page: bool,
        voice_busy: resonance_battle::ActorSet,
        entry_voice_ready: bool,
    },
    Field(
        bevy::ecs::change_detection::Tick,
        resonance_game::field::FieldInput,
    ),
    World(
        bevy::ecs::change_detection::Tick,
        resonance_game::overworld::Input,
    ),
    Menu(
        bevy::ecs::change_detection::Tick,
        resonance_game::field::FieldInput,
    ),
    Title {
        events: bevy::ecs::change_detection::Tick,
        menu: bevy::ecs::change_detection::Tick,
        clock: u32,
        input: resonance_game::MenuInput,
    },
}

/// Exactly one scene owns the cached branch and its operation dependencies.
#[allow(clippy::large_enum_variant)] // Keep scene ownership inline; nested data already uses arenas.
pub(crate) enum Endpoint {
    Field(FieldPrediction),
    Battle(
        crate::battle::prediction::Endpoint,
        resonance_events::OperationFork,
    ),
    Hold(resonance_events::OperationFork),
    World(crate::overworld::Scene, resonance_events::OperationFork),
    Title(
        (resonance_game::TitleState, resonance_events::EventRuntime),
        resonance_events::OperationFork,
    ),
    Menu(resonance_game::menu::Menu),
}

impl Endpoint {
    fn sources_unchanged(&self) -> bool {
        match self {
            Self::Field(next) => next.sources_unchanged(),
            Self::Battle(_, sources)
            | Self::Hold(sources)
            | Self::World(_, sources)
            | Self::Title(_, sources) => sources.sources_unchanged(),
            Self::Menu(_) => true,
        }
    }

    pub(crate) fn field(&self) -> Option<&resonance_game::field::FieldSession> {
        match self {
            Self::Field(next) => next.state.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn world(&self) -> Option<&crate::overworld::Scene> {
        match self {
            Self::World(next, _) => Some(next),
            _ => None,
        }
    }

    pub(crate) fn menu(&self) -> Option<&resonance_game::menu::Menu> {
        match self {
            Self::Field(next) => next.state.as_ref()?.menu.as_ref(),
            Self::World(next, _) => next.session.menu.as_ref(),
            Self::Menu(next) => Some(next),
            Self::Title(..) | Self::Battle(..) | Self::Hold(_) => None,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct Prediction {
    cached: Option<Cached>,
    active: bool,
    built: u64,
    reused: u64,
    storage: Option<resonance_game::field::PredictionBuffer>,
    pub alpha: f32,
    reported_error: bool,
}

/// The endpoint and its poses are valid for exactly this source and input.
struct Cached {
    key: Key,
    endpoint: Endpoint,
    poses: Option<Vec<PoseEndpoint>>,
}

// Persistent system queries avoid constructing allocating QueryStates per frame.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Scene<'w, 's> {
    checkpoint: Option<Res<'w, super::Session>>,
    resident: Option<Res<'w, crate::loading::Resident>>,
    art: Option<Res<'w, super::Art>>,
    ui: Option<Res<'w, crate::field_ui::Artwork>>,
    images: Option<Res<'w, Assets<Image>>>,
    session: Option<Res<'w, crate::new_game::Session>>,
    controls: Option<Res<'w, super::Controls>>,
    parts: Query<'w, 's, &'static super::ActorPart>,
    particles: Query<'w, 's, &'static crate::field_model_particles::Part>,
}

pub(super) fn reset(
    mut prediction: ResMut<Prediction>,
    options: Option<Res<crate::RunOptions>>,
    fixed: Res<Time<Fixed>>,
    testing: Option<Res<crate::testing::Controls>>,
    loading: Option<Res<crate::loading::Pending>>,
    preferences: Option<Res<crate::config::Preferences>>,
    (session, title): (
        Option<Res<crate::new_game::Session>>,
        Option<Res<crate::saves::title::TitleMenu>>,
    ),
) {
    // Shared enablement gate for every scene; scene systems only check readiness.
    // Readiness is rechecked by the owning scene before publishing the cache.
    // A preparation gap can rebuild render entities even if game time is frozen.
    if !prediction.active {
        prediction.clear();
    }
    prediction.active = false;
    let enabled = crate::input::active_menu(
        session.as_ref().and_then(|session| session.menu()),
        title.as_ref().map(|title| &title.0),
    )
    .and_then(|(_, menu)| menu.preferences())
    .or_else(|| {
        preferences
            .as_ref()
            .map(|preferences| &preferences.settings)
    })
    .is_some_and(|settings| settings.experimental_high_fps);
    prediction.alpha = if options.is_some_and(|o| {
        (o.speculative || enabled) && !o.headless() && o.replay.is_none() && o.capture_at.is_none()
    }) && testing.is_none_or(|t| !t.paused && !t.skipping)
        && loading.is_none()
        && !prediction.reported_error
    {
        fixed.overstep_fraction().clamp(0., 1.)
    } else {
        0.
    };
    if prediction.alpha <= 0. {
        prediction.clear();
    } else if prediction.storage.is_none()
        && let Err(error) = prediction.reserve()
    {
        prediction.report_error(&error);
    }
}

pub(super) fn update(mut prediction: ResMut<Prediction>, scene: Scene) {
    if prediction.alpha <= 0.
        || scene.checkpoint.is_some()
        || scene
            .resident
            .as_ref()
            .is_none_or(|r| !r.active.load(std::sync::atomic::Ordering::Acquire))
    {
        return;
    }
    let (Some(session), Some(art), Some(controls)) = (scene.session, scene.art, scene.controls)
    else {
        return;
    };
    if !session.ready_for_field
        || session.audio.is_some()
        || !session.is_field()
        || scene.ui.as_ref().is_none_or(|ui| {
            !ui.menu_published(session.field())
                || scene.images.as_ref().is_none_or(|images| !ui.ready(images))
        })
        || !art.ready
        || art.map != session.map_id()
        || session
            .field()
            .events
            .world
            .actors
            .iter()
            .any(|(id, actor)| {
                art.models.contains_key(&actor.model_resource()) && !art.instances.contains_key(id)
            })
        || scene.parts.iter().any(|part| !part.prepared)
        || scene
            .particles
            .iter()
            .filter(|part| part.prepared())
            .count()
            != session
                .field()
                .events
                .world
                .model_particles
                .values()
                .map(|p| art.models.get(&p.resource).map_or(1, Vec::len))
                .sum::<usize>()
    {
        return;
    }
    let key = Key::Field(session.last_changed(), controls.input);
    if prediction.reuse(&key) {
        return;
    }
    let _timer = crate::performance::smoothing_profile::Timer::start(0);
    let input = controls.input;
    prediction.predict(key, || {
        session
            .field()
            .capture_prediction()
            .advance(input)
            .map(Endpoint::Field)
    });
}

impl Prediction {
    fn clear(&mut self) {
        self.cached = None;
        self.active = false;
    }

    pub(crate) fn reuse(&mut self, key: &Key) -> bool {
        if self
            .cached
            .as_ref()
            .is_some_and(|cached| &cached.key == key && cached.endpoint.sources_unchanged())
        {
            self.active = true;
            self.reused += 1;
            return true;
        }
        self.clear();
        false
    }

    /// Capture the committed state and visit the next update in the spare arena.
    /// Committed frames also need an endpoint for their remaining fractional tick.
    pub(crate) fn predict(&mut self, key: Key, visit: impl FnOnce() -> anyhow::Result<Endpoint>) {
        let Some(storage) = self.storage.as_mut() else {
            return;
        };
        match storage.execute(visit).and_then(|result| result) {
            Ok(endpoint) => self.publish(key, endpoint),
            Err(error) => self.report_error(&error),
        }
    }

    pub(crate) fn publish(&mut self, key: Key, endpoint: Endpoint) {
        self.built += 1;
        self.active = true;
        self.cached = Some(Cached {
            key,
            endpoint,
            poses: None,
        });
    }

    pub(crate) fn counts(&self) -> [u64; 2] {
        [self.built, self.reused]
    }

    pub(crate) fn poses(&self) -> Option<&[PoseEndpoint]> {
        self.cached
            .as_ref()
            .filter(|_| self.active)?
            .poses
            .as_deref()
    }

    pub(crate) fn retain_poses(&mut self, poses: &[PoseEndpoint]) -> anyhow::Result<()> {
        let storage = self
            .storage
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("smoothing storage is unavailable"))?;
        let cached = self
            .cached
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("smoothing endpoint is unavailable"))?;
        cached.poses = Some(storage.retain(|| poses.to_vec())?);
        Ok(())
    }

    pub(crate) fn endpoint(&self) -> Option<&Endpoint> {
        Some(&self.cached.as_ref().filter(|_| self.active)?.endpoint)
    }

    pub(crate) fn battle(&self) -> Option<&crate::battle::prediction::Endpoint> {
        match self.endpoint()? {
            Endpoint::Battle(next, _) => Some(next),
            _ => None,
        }
    }

    pub(crate) fn world(&self) -> Option<&crate::overworld::Scene> {
        self.endpoint()?.world()
    }

    pub(crate) fn menu(&self) -> Option<&resonance_game::menu::Menu> {
        self.endpoint()?.menu()
    }

    pub(crate) fn title(
        &self,
    ) -> Option<&(resonance_game::TitleState, resonance_events::EventRuntime)> {
        match self.endpoint()? {
            Endpoint::Title(next, _) => Some(next),
            _ => None,
        }
    }

    /// Sample in the spare arena. Storage or drawing errors disable prediction
    /// and leave the committed drawing available to the caller.
    pub(crate) fn try_scratch<V, T>(
        &mut self,
        select: fn(&Endpoint) -> Option<&V>,
        work: impl FnOnce(&V, f32) -> anyhow::Result<T>,
    ) -> Option<T> {
        if !self.active {
            return None;
        }
        let next = select(&self.cached.as_ref()?.endpoint)?;
        let result = self.storage.as_mut()?.scratch(|| work(next, self.alpha));
        match result.and_then(|value| value) {
            Ok(value) => Some(value),
            Err(error) => {
                self.report_error(&error);
                None
            }
        }
    }

    pub(crate) fn scratch_scene<T>(&mut self, work: impl FnOnce(&Endpoint, f32) -> T) -> Option<T> {
        self.try_scratch(|scene| Some(scene), |scene, alpha| Ok(work(scene, alpha)))
    }

    #[cfg(test)]
    pub(crate) fn execute<T>(&mut self, work: impl FnOnce() -> T) -> anyhow::Result<T> {
        self.storage
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("smoothing storage is unavailable"))?
            .execute(work)
    }

    pub(crate) fn report_error(&mut self, error: &anyhow::Error) {
        self.clear();
        self.alpha = 0.;
        if !self.reported_error {
            warn!("Smoothing prediction failed; showing committed state: {error:#}");
            self.reported_error = true;
        }
    }
    pub fn field(&self) -> Option<&resonance_game::field::FieldSession> {
        self.endpoint()?.field()
    }

    pub fn scratch<T>(
        &mut self,
        work: impl FnOnce(&resonance_game::field::FieldSession, f32) -> T,
    ) -> Option<T> {
        self.try_scratch(Endpoint::field, |field, alpha| Ok(work(field, alpha)))
    }
    pub fn reserve(&mut self) -> anyhow::Result<()> {
        self.storage = Some(resonance_game::field::PredictionBuffer::new()?);
        Ok(())
    }
    pub(crate) fn camera(&self, current: &CameraRig) -> Option<&CameraRig> {
        let next = self.field()?.events.world.field_camera.as_ref()?;
        (next.selected == current.selected
            && Vec3::from_array(next.position).distance(Vec3::from_array(current.position)) <= 256.
            && Vec3::from_array(next.target).distance(Vec3::from_array(current.target)) <= 256.)
            .then_some(next)
    }

    pub fn camera_transform(&self, current: &CameraRig) -> Transform {
        let from = super::camera_transform(current);
        let Some(next) = self.camera(current) else {
            return from;
        };
        let to = super::camera_transform(next);
        Transform {
            translation: from.translation.lerp(to.translation, self.alpha),
            rotation: from.rotation.slerp(to.rotation, self.alpha),
            ..from
        }
    }

    pub fn camera_fov(&self, current: &CameraRig) -> f32 {
        let from = current.fov_degrees();
        self.camera(current)
            .map_or(from, |next| from + (next.fov_degrees() - from) * self.alpha)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn title(
    mut prediction: ResMut<Prediction>,
    menu: Res<crate::Menu>,
    events: Option<Res<crate::Events>>,
    pending: Res<crate::PendingInput>,
    controls: Res<super::Controls>,
    clock: Res<crate::Clock>,
    ready: Res<crate::timing::Ready>,
    movie: Res<crate::movie::Playback>,
    boot: Res<crate::boot::Playback>,
    session: Option<Res<crate::new_game::Session>>,
    loading: Option<Res<crate::loading::Pending>>,
    title_menu: Option<Res<crate::saves::title::TitleMenu>>,
    game_over: Option<Res<crate::session_screen::GameOver>>,
    overlay: Option<Res<crate::field_ui::MenuOverlay>>,
    images: Res<Assets<Image>>,
) {
    if prediction.alpha <= 0.
        || session.is_some()
        || loading.is_some()
        || game_over.is_some()
        || !ready.0
        || movie.active
        || boot.active()
    {
        return;
    }
    if let Some(menu) = title_menu {
        if overlay.is_none_or(|a| !a.ready(&images))
            || menu.0.page == resonance_game::menu::Page::Customize && menu.0.resources.is_none()
        {
            return;
        }
        let key = Key::Menu(menu.last_changed(), controls.input);
        if prediction.reuse(&key) {
            return;
        }
        prediction.predict(key, || {
            let mut next = menu.0.clone();
            next.step(controls.input);
            Ok(Endpoint::Menu(next))
        });
    } else if let Some(events) = events {
        let mut time = clock.0;
        time.advance();
        let input = (*pending).clone().consume(time);
        let key = Key::Title {
            events: events.last_changed(),
            menu: menu.last_changed(),
            clock: time.tick(),
            input,
        };
        if prediction.reuse(&key) {
            return;
        }
        prediction.predict(key, || {
            let mut fork = resonance_events::OperationFork::default();
            let mut next = events.0.fork_for_prediction(&mut fork);
            next.step()?;
            let mut state = menu.0.clone();
            let action = state.step(input);
            Ok(if action.is_none() {
                Endpoint::Title((state, next), fork)
            } else {
                Endpoint::Hold(fork)
            })
        });
    }
}

/// Follow short periodic scrolls. Callers must identify scroll channels first:
/// a small delta can also be an atlas-row change and must remain discrete.
pub(crate) fn uv_offsets(from: Vec4, to: Vec4, alpha: f32) -> Vec4 {
    let delta = (to - from)
        .to_array()
        .map(resonance_content::wrapped_scroll_delta);
    from + Vec4::from_array(delta) * alpha
}

pub(crate) fn uv_scrolls(from: Vec4, to: Vec4, scrolls: [bool; 2], alpha: f32) -> Vec4 {
    let smooth = uv_offsets(from, to, alpha);
    Vec4::from_array(std::array::from_fn(|i| {
        if scrolls[i / 2] { smooth[i] } else { from[i] }
    }))
}

/// Material, shadow and pose endpoints must agree about actor discontinuities.
pub(crate) fn continuous_actor(
    from: &resonance_events::Actor,
    to: &resonance_events::Actor,
) -> bool {
    from.instance == to.instance
        && from.model_resource() == to.model_resource()
        && from.field_model_pose().visible() == to.field_model_pose().visible()
        && from.attachment.as_ref().map(|a| (a.actor, &a.bone))
            == to.attachment.as_ref().map(|a| (a.actor, &a.bone))
        && Vec3::from_array(from.field_model_pose().position)
            .distance(Vec3::from_array(to.field_model_pose().position))
            <= 128.
}

pub(crate) fn continuous_pose(
    from: &resonance_events::Actor,
    to: &resonance_events::Actor,
) -> bool {
    from.field_model_pose().secondary_motion_disabled
        == to.field_model_pose().secondary_motion_disabled
        && continuous_actor(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_title_reuses_storage_but_input_ticks_and_readiness_invalidate_it() {
        use bevy::ecs::system::RunSystemOnce;
        use std::sync::Arc;
        let bytes: Vec<_> = [4u16, 0, 0, 0, 0x20ff]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect();
        let events = resonance_events::EventRuntime::new(
            Arc::new(symphonia_script::Program::decode(&bytes).unwrap()),
            Default::default(),
        )
        .unwrap();
        let mut world = World::new();
        world.insert_resource(crate::Events(events));
        world.insert_resource(crate::Menu(resonance_game::TitleState::default()));
        world.init_resource::<crate::Clock>();
        world.init_resource::<crate::PendingInput>();
        world.init_resource::<super::super::Controls>();
        world.insert_resource(crate::timing::Ready(true));
        world.init_resource::<crate::movie::Playback>();
        world.init_resource::<crate::boot::Playback>();
        world.init_resource::<Assets<Image>>();
        let mut prediction = Prediction::default();
        prediction.reserve().unwrap();
        world.insert_resource(prediction);
        let render = |world: &mut World, alpha| {
            let mut prediction = world.resource_mut::<Prediction>();
            prediction.alpha = alpha;
            prediction.active = false;
            world.run_system_once(title).unwrap();
        };
        render(&mut world, 0.1);
        let bytes = world
            .resource::<Prediction>()
            .storage
            .as_ref()
            .unwrap()
            .used_bytes();
        assert_eq!(world.resource::<Prediction>().counts(), [1, 0]);
        for alpha in [0.2, 0.5, 0.9] {
            render(&mut world, alpha);
            let prediction = world.resource::<Prediction>();
            assert!(prediction.title().is_some());
            assert_eq!(prediction.storage.as_ref().unwrap().used_bytes(), bytes);
        }
        assert_eq!(world.resource::<Prediction>().counts(), [1, 3]);
        world.resource_mut::<crate::PendingInput>().pressed.reveal = true;
        render(&mut world, 0.95);
        assert_eq!(world.resource::<Prediction>().counts(), [2, 3]);
        assert!(
            world.resource::<crate::PendingInput>().pressed.reveal,
            "prediction consumed live input"
        );
        render(&mut world, 0.99);
        assert_eq!(world.resource::<Prediction>().counts(), [2, 4]);
        assert_eq!(world.resource::<crate::Events>().0.tick(), 0);

        world.resource_mut::<crate::Clock>().0.advance();
        render(&mut world, 0.1);
        assert_eq!(world.resource::<Prediction>().counts(), [3, 4]);
        assert!(world.resource::<Prediction>().title().is_some());
        // Source edits at the same game tick must also invalidate the endpoint.
        world.resource_mut::<crate::Events>().0.world.random_state = 123;
        render(&mut world, 0.2);
        assert_eq!(world.resource::<Prediction>().counts(), [4, 4]);
        world.resource_mut::<crate::timing::Ready>().0 = false;
        render(&mut world, 0.3);
        assert!(world.resource::<Prediction>().title().is_none());
    }

    #[test]
    fn cached_scene_and_pose_endpoints_share_one_slot_and_release_before_rotation() {
        use bevy::ecs::change_detection::Tick;
        let mut prediction = Prediction::default();
        prediction.reserve().unwrap();
        let calls = resonance_simulation_memory::system_calls();
        for tick in 0..20 {
            let key = Key::Menu(Tick::new(tick), Default::default());
            assert!(!prediction.reuse(&key));
            let menu = prediction
                .execute(|| {
                    resonance_game::menu::Menu::new(
                        resonance_game::menu::Page::Slots(resonance_game::menu::Mode::Load),
                        None,
                        false,
                    )
                })
                .unwrap();
            prediction.publish(key.clone(), Endpoint::Menu(menu));
            prediction
                .retain_poses(&[PoseEndpoint {
                    entity: Entity::PLACEHOLDER,
                    poses: crate::sparse_animation::affine::PosePair::new(
                        Transform::IDENTITY.into(),
                        Transform::from_xyz(1., 2., 3.).into(),
                    ),
                    space: crate::sparse_animation::affine::PoseSpace::Local,
                }])
                .unwrap();
            let address = prediction.poses().unwrap().as_ptr();
            for _ in 0..10 {
                prediction.active = false;
                assert!(prediction.reuse(&key));
                assert_eq!(prediction.poses().unwrap().as_ptr(), address);
                // Retained poses must never occupy the transient scratch slot.
                prediction
                    .scratch_scene(|_, _| {
                        let scratch = vec![0u8; 1024];
                        assert_eq!(scratch.len(), 1024);
                    })
                    .unwrap();
            }
        }
        prediction.clear();
        assert_eq!(resonance_simulation_memory::system_calls(), calls);
    }

    #[test]
    fn escaped_scratch_disables_prediction_without_overwriting_live_storage() {
        let mut prediction = Prediction::default();
        prediction.reserve().unwrap();
        let menu = prediction
            .execute(|| {
                resonance_game::menu::Menu::new(
                    resonance_game::menu::Page::Slots(resonance_game::menu::Mode::Load),
                    None,
                    false,
                )
            })
            .unwrap();
        prediction.publish(
            Key::Menu(
                bevy::ecs::change_detection::Tick::new(1),
                Default::default(),
            ),
            Endpoint::Menu(menu),
        );
        prediction.alpha = 0.5;
        // Selecting another scene must not enter scratch memory or call the renderer.
        assert_eq!(prediction.scratch(|_, _| panic!("not a field")), None::<()>);
        let retained = prediction.scratch_scene(|_, _| vec![7u8; 128]).unwrap();
        assert_eq!(
            prediction.scratch_scene(|_, _| panic!("arena still in use")),
            None::<()>
        );
        assert!(prediction.reported_error);
        assert!(prediction.endpoint().is_none());
        assert_eq!(prediction.alpha, 0.);
        assert_eq!(retained, [7; 128]);
        drop(retained);
        assert_eq!(
            prediction.scratch_scene(|_, _| panic!("disabled")),
            None::<()>
        );
    }

    #[test]
    fn small_atlas_steps_never_sample_between_rows_while_scrolls_wrap() {
        assert_eq!(
            uv_scrolls(Vec4::ZERO, Vec4::splat(0.25), [true, false], 0.5),
            Vec4::new(0.125, 0.125, 0., 0.)
        );
        let from = Vec4::new(0., 30. / 32., 0.99, 2.02);
        let to = Vec4::new(0., 31. / 32., 0.01, 0.02);
        for alpha in [0., 0.25, 0.5, 0.75, 0.999] {
            let value = uv_scrolls(from, to, [false, true], alpha);
            assert_eq!(value.y, from.y);
            assert!((value.w - from.w).abs() < 0.00001);
            assert!((value.z - (0.99 + 0.02 * alpha)).abs() < 0.00001);
        }
    }
    #[test]
    fn held_predictions_reuse_dependencies_and_release_their_arena_on_invalidation() {
        use bevy::ecs::{change_detection::Tick, system::RunSystemOnce};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let key = Key::Menu(Tick::new(1), Default::default());
        let flag = Arc::new(AtomicBool::new(false));
        let mut prediction = Prediction::default();
        prediction.reserve().unwrap();
        let sources = prediction
            .execute(|| {
                let mut sources = resonance_events::OperationFork::default();
                sources.fork_flag(&flag);
                sources
            })
            .unwrap();
        prediction.publish(key.clone(), Endpoint::Hold(sources));
        let mut world = World::new();
        let mut options = crate::test_support::run_options("");
        options.speculative = true;
        world.insert_resource(options);
        let mut fixed = Time::<Fixed>::default();
        fixed.accumulate_overstep(std::time::Duration::from_millis(5));
        world.insert_resource(fixed);
        world.insert_resource(prediction);
        for _ in 0..30 {
            world.run_system_once(reset).unwrap();
            let mut p = world.resource_mut::<Prediction>();
            assert!(p.reuse(&key));
            assert!(
                p.field().is_none()
                    && p.battle().is_none()
                    && p.menu().is_none()
                    && p.title().is_none()
            );
        }
        assert_eq!(world.resource::<Prediction>().counts(), [1, 30]);
        // A callback can invalidate a held result without mutating the session.
        flag.store(true, Ordering::Release);
        {
            let mut p = world.resource_mut::<Prediction>();
            assert!(!p.reuse(&key));
            for tick in 2..5 {
                let sources = p.execute(resonance_events::OperationFork::default).unwrap();
                p.publish(key.clone(), Endpoint::Hold(sources));
                assert!(!p.reuse(&Key::Menu(Tick::new(tick), Default::default())));
            }
            let sources = p.execute(resonance_events::OperationFork::default).unwrap();
            p.publish(key.clone(), Endpoint::Hold(sources));
            let mut input = resonance_game::field::FieldInput::default();
            input.scroll_pressed[0] = true;
            assert!(!p.reuse(&Key::Menu(Tick::new(1), input)));
            p.publish(key.clone(), Endpoint::Hold(Default::default()));
        }
        // Readiness must still be checked each frame. A missed visit clears the cache.
        world.run_system_once(reset).unwrap();
        world.run_system_once(reset).unwrap();
        assert!(!world.resource_mut::<Prediction>().reuse(&key));
    }

    #[test]
    fn saved_smoothing_predicts_on_committed_frames_at_every_rate_and_disables_for_captures() {
        use bevy::ecs::{change_detection::Tick, system::RunSystemOnce};
        let mut world = World::new();
        world.insert_resource(crate::test_support::run_options(""));
        world.init_resource::<crate::config::Preferences>();
        world.init_resource::<Prediction>();
        let mut fixed = Time::<Fixed>::from_duration(resonance_game::clock::UPDATE_STEP);
        fixed.accumulate_overstep(std::time::Duration::from_millis(5));
        world.insert_resource(fixed);
        world.run_system_once(reset).unwrap();
        assert!(world.resource::<Prediction>().storage.is_none());
        world
            .resource_mut::<crate::config::Preferences>()
            .settings
            .experimental_high_fps = true;
        for frames in [1, 2, 4, 8] {
            let before = world.resource::<Prediction>().counts();
            for tick in 1..=3 {
                world
                    .resource_mut::<Time<Fixed>>()
                    .advance_by(resonance_game::clock::UPDATE_STEP);
                let key = Key::Menu(Tick::new(frames * 10 + tick), Default::default());
                for _ in 0..frames {
                    world.run_system_once(reset).unwrap();
                    let mut prediction = world.resource_mut::<Prediction>();
                    assert!(prediction.alpha > 0.);
                    if !prediction.reuse(&key) {
                        prediction.predict(key.clone(), || Ok(Endpoint::Hold(Default::default())));
                    }
                    assert!(prediction.endpoint().is_some());
                }
            }
            assert_eq!(
                world.resource::<Prediction>().counts(),
                [before[0] + 3, before[1] + 3 * u64::from(frames - 1)]
            );
        }
        world
            .resource_mut::<crate::config::Preferences>()
            .settings
            .experimental_high_fps = false;
        world.run_system_once(reset).unwrap();
        assert_eq!(world.resource::<Prediction>().alpha, 0.);
        assert!(world.resource::<Prediction>().cached.is_none());
        world
            .resource_mut::<crate::config::Preferences>()
            .settings
            .experimental_high_fps = true;
        for mode in 0..3 {
            let mut options = crate::test_support::run_options("");
            match mode {
                0 => options.capture = Some("capture".into()),
                1 => options.replay = Some("replay".into()),
                _ => options.capture_at = Some(crate::CaptureAt::TitleTick(1)),
            }
            world.insert_resource(options);
            world.run_system_once(reset).unwrap();
            assert_eq!(world.resource::<Prediction>().alpha, 0.);
        }
    }
}
