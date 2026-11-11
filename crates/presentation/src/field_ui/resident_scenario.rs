//! Shared driver for the residency regression and the optional diagnostic tool.
use crate::transition_metrics::{Place, Trace};
use anyhow::{Context as ContextExt, Result, ensure};
use bevy::{prelude::*, time::TimeUpdateStrategy};
use std::time::{Duration, Instant};

pub(super) struct Scenario {
    pub app: App,
    #[cfg(feature = "preparation-trace")]
    pub boot_ms: f64,
    #[cfg(feature = "preparation-trace")]
    pub preparation_overlapped: bool,
    mixer: resonance_playback::Control,
    audio: resonance_playback::Offline,
    transition_sequence: u64,
}

impl Scenario {
    pub fn boot(resolution: crate::Resolution, configure: impl FnOnce(&mut App)) -> Result<Self> {
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
        Self::boot_with_options(
            crate::test_support::run_options(std::path::PathBuf::from(root)),
            resolution,
            configure,
        )
    }

    pub fn boot_with_options(
        mut options: crate::RunOptions,
        resolution: crate::Resolution,
        configure: impl FnOnce(&mut App),
    ) -> Result<Self> {
        options.capture = Some("unused-residency-test.png".into());
        let (mut app, _) = crate::build_app_with_display(options, resolution)?;
        let began = Instant::now();
        while app.plugins_state() == bevy::app::PluginsState::Adding {
            ensure!(began.elapsed().as_secs() < 30, "GPU setup timed out");
            bevy::tasks::tick_global_task_pools_on_main_thread();
            std::thread::sleep(Duration::from_millis(1));
        }
        configure(&mut app);
        app.finish();
        crate::battle_view::ResidentModels::install_upload_audit(&mut app);
        app.cleanup();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        #[cfg(feature = "preparation-trace")]
        let mut preparation_overlapped = false;
        loop {
            app.update();
            app.world()
                .resource::<crate::diagnostics::FatalErrors>()
                .finish(crate::playthrough::check_exit(&app))?;
            #[cfg(feature = "preparation-trace")]
            {
                preparation_overlapped |= crate::loading::startup::pipelines_building(app.world());
            }
            if crate::loading::startup::ready(app.world()) {
                break;
            }
            ensure!(
                !app.world().resource::<crate::timing::Ready>().0
                    && app
                        .world()
                        .resource::<Assets<crate::materials::TitleOutput>>()
                        .iter()
                        .all(|(_, output)| output.brightness.z == 1.),
                "cache preparation exposed or advanced startup playback"
            );
            ensure!(began.elapsed().as_secs() < 60, "residency boot timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        let (mixer, audio) = resonance_playback::Offline::new();
        Ok(Self {
            app,
            #[cfg(feature = "preparation-trace")]
            boot_ms: began.elapsed().as_secs_f64() * 1000.,
            #[cfg(feature = "preparation-trace")]
            preparation_overlapped,
            mixer,
            audio,
            transition_sequence: 0,
        })
    }

    pub fn update(&mut self) -> Result<()> {
        let held = self
            .app
            .world()
            .contains_resource::<crate::new_game::scene_fade::Fade>()
            && matches!(self.app.world().resource::<TimeUpdateStrategy>(), TimeUpdateStrategy::ManualDuration(duration) if duration.is_zero());
        if held {
            self.app.insert_resource(TimeUpdateStrategy::ManualDuration(
                resonance_game::clock::UPDATE_STEP,
            ));
        }
        self.app.update();
        if held {
            self.app
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        }
        crate::playthrough::check_exit(&self.app)?;
        crate::playthrough::attach::<crate::field_audio::FieldSource>(
            self.app.world_mut(),
            &self.mixer,
        )?;
        for _ in 0..1600 {
            self.audio.next().context("offline audio stopped")?;
        }
        Ok(())
    }

    pub fn wait(&mut self, message: &str, mut ready: impl FnMut(&mut App) -> bool) -> Result<()> {
        let since = Instant::now();
        loop {
            self.update()?;
            if ready(&mut self.app) {
                return Ok(());
            }
            ensure!(since.elapsed().as_secs() < 30, "{message}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn enter(&mut self, map: u32) -> Result<()> {
        self.request_scene(map)?;
        self.wait_for_scene(map)
    }

    pub fn request_scene(&mut self, map: u32) -> Result<()> {
        self.app
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        if self
            .app
            .world()
            .contains_resource::<crate::new_game::Session>()
        {
            self.app
                .world_mut()
                .resource_mut::<crate::new_game::Session>()
                .events_mut()
                .world
                .request_destination(resonance_events::SceneDestination {
                    map,
                    position: if map >= 3000 {
                        [2., 0., 0.]
                    } else {
                        [-100., -100., 0.]
                    },
                    heading: 0.,
                })
                .map_err(anyhow::Error::msg)?;
        } else {
            ensure!(map == 332, "the residency fixture starts in field 332");
            let common = self
                .app
                .world()
                .resource::<crate::loading::Resident>()
                .common
                .get()
                .context("startup inputs missing")?;
            let work = tempfile::tempdir()?;
            let save = work.path().join("checkpoint.json");
            crate::saves::prepare_checkpoint_fixture(
                &self.app.world().resource::<crate::RunOptions>().assets,
                crate::test_support::field_checkpoint(&common.files)?,
                &save,
            )?;
            crate::transition_metrics::begin(self.app.world_mut(), Place::Title, Place::Field(map));
            self.app
                .insert_resource(crate::new_game::Request(Some(std::fs::read(save)?)));
        }
        Ok(())
    }

    pub fn wait_for_scene(&mut self, map: u32) -> Result<()> {
        self.wait("scene admission timed out", |app| {
            app.world()
                .get_resource::<crate::new_game::Session>()
                .is_some_and(|session| {
                    session.map_id() == map
                        && session.events().world.field_transition.is_none()
                        && session.events().world.world_transition.is_none()
                })
                && scene_ready(app)
        })?;
        self.wait_for_transition(if map >= 3000 {
            Place::World
        } else {
            Place::Field(map)
        })?;
        self.wait("scene fade did not finish", |app| {
            !app.world()
                .contains_resource::<crate::new_game::scene_fade::Fade>()
        })
    }

    fn wait_for_transition(&mut self, destination: Place) -> Result<()> {
        let sequence = self.transition_sequence;
        self.wait("scene frame submission timed out", |app| {
            app.world()
                .resource::<Trace>()
                .completed_since(sequence, destination)
                .is_some()
        })?;
        self.transition_sequence = self
            .app
            .world()
            .resource::<Trace>()
            .last
            .as_ref()
            .unwrap()
            .sequence;
        Ok(())
    }

    pub fn battle(&mut self) -> Result<resonance_events::battle::Request> {
        self.battle_with_clock(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ))
    }

    #[cfg(feature = "preparation-trace")]
    pub fn battle_realtime(&mut self) -> Result<resonance_events::battle::Request> {
        // Held/manual frames leave Real's instant behind (or ahead of) wall time.
        // Rebase it before Automatic so entry cannot inherit a catch-up delta.
        self.app
            .world_mut()
            .resource_mut::<Time<Real>>()
            .update_with_instant(Instant::now());
        self.battle_with_clock(TimeUpdateStrategy::Automatic)
    }

    fn battle_with_clock(
        &mut self,
        clock: TimeUpdateStrategy,
    ) -> Result<resonance_events::battle::Request> {
        let realtime = matches!(clock, TimeUpdateStrategy::Automatic);
        self.app.insert_resource(clock);
        let request = self
            .app
            .world_mut()
            .resource_mut::<crate::new_game::Session>()
            .events_mut()
            .world
            .request_battle(resonance_events::battle::Setup {
                encounter: resonance_events::battle::Encounter::Formation(1),
                arena: 13,
                route: [0; 5],
                music: None,
                defeat: resonance_events::battle::DefeatPolicy::ResumeEvent,
            })
            .map_err(anyhow::Error::msg)?;
        let mut observed_shatter = false;
        let mut last_entry_update = None;
        let mut animated_entry = false;
        self.wait("battle admission timed out", |app| {
            let (presenting, transition) = crate::battle::entry_status(app.world());
            observed_shatter |= transition;
            if transition && presenting {
                let (_, update, _) = app
                    .world()
                    .resource::<crate::battle::Owner>()
                    .capture_clock()
                    .unwrap();
                animated_entry |= last_entry_update.is_some_and(|previous| update > previous);
                last_entry_update = Some(update);
            }
            presenting && !transition
        })?;
        ensure!(
            realtime || animated_entry,
            "battle opening stayed frozen beneath the shatter"
        );
        ensure!(
            !realtime || observed_shatter,
            "battle benchmark requires a visible outgoing scene and shatter"
        );
        self.app
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        self.wait_for_transition(Place::Battle)?;
        Ok(request)
    }

    pub fn return_from_battle(&mut self, request: resonance_events::battle::Request) -> Result<()> {
        request
            .complete(resonance_events::battle::Outcome::Escaped)
            .map_err(anyhow::Error::msg)?;
        self.wait("battle return timed out", |app| {
            !app.world().contains_resource::<crate::battle::Owner>() && scene_ready(app)
        })?;
        let session = self.app.world().resource::<crate::new_game::Session>();
        self.wait_for_transition(if session.is_field() {
            Place::Field(session.map_id())
        } else {
            Place::World
        })
    }
}

pub(super) fn scene_ready(app: &mut App) -> bool {
    crate::new_game::scene_ready(app.world_mut())
        && app
            .world()
            .resource::<crate::loading::Resident>()
            .active
            .load(std::sync::atomic::Ordering::Acquire)
}
