//! Host ownership regression without a window, GPU, or audio device. Prepared
//! resources and runtime functions are used. Tests start at the handoff boundary;
//! combat damage and field-script execution have separate integration coverage.
//! These tests make no image/audio-fidelity claim.
use super::*;
use crate::test_support::field_checkpoint;
use crate::{field_audio::validation, game_over, loading, new_game, saves};
use resonance_game::{field::FieldCheckpoint, menu::SlotFocus};
use resonance_persistence::{Header, Kind, SlotId, Store};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, Weak},
    time::{Duration, Instant},
};

struct SharedAudio(Arc<Mutex<validation::Playback>>);
impl Iterator for SharedAudio {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.0.lock().unwrap().next_output_sample()
    }
}
impl resonance_playback::Source for SharedAudio {
    fn channels(&self) -> resonance_playback::ChannelCount {
        2.try_into().unwrap()
    }
    fn sample_rate(&self) -> resonance_playback::SampleRate {
        resonance_playback::SOURCE_RATE.try_into().unwrap()
    }
}

struct Fixture {
    app: App,
    audio: Weak<Mutex<validation::Playback>>,
    mixer: resonance_playback::Offline,
    audio_entity: Entity,
    audio_handle: resonance_playback::Handle,
    title_root: Entity,
    request: Request,
    saved: FieldCheckpoint,
    store: Store,
    _directory: tempfile::TempDir,
    _output: Handle<crate::materials::TitleOutput>,
    field_tick: u32,
    party: serde_json::Value,
    completions: u8,
}

fn root() -> PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
        Into::into,
    )
}
fn finish<T: Send + 'static>(task: loading::Task<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        if let Some(result) = task.poll()? {
            return result;
        }
        ensure!(
            start.elapsed() < Duration::from_secs(120),
            "fatal fixture preparation timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

impl Fixture {
    fn new(label: &str) -> Result<Self> {
        Self::with_package(label, |_| {})
    }

    fn with_package(label: &str, configure: impl FnOnce(&mut Package)) -> Result<Self> {
        Self::configured(label, |_| {}, |_, package| configure(package))
    }

    fn configured(
        label: &str,
        configure_session: impl FnOnce(&mut new_game::Session),
        configure_scene: impl FnOnce(&mut World, &mut Package),
    ) -> Result<Self> {
        let root = root();
        let mods = std::env::var_os("RESONANCE_NAMED_TITLE_FIELDS")
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>();
        let library = resonance_content::library::Library::open_paths(&root, &mods)?;
        let field = new_game::FieldPackage::capture(
            &library,
            332,
            &mut Default::default(),
            || false,
            resonance_content::diagnostics::Diagnostics::new(true),
        )?;
        let files = field.files;
        let saved = field_checkpoint(&files)?;
        let resident = loading::Resident::default();
        *resident.files.write().unwrap() = Some(files.clone());
        let bytes = resonance_persistence::encode(
            &Header {
                version: resonance_persistence::VERSION,
                label: "Fatal host fixture".into(),
                location: "Iselia".into(),
                played_ticks: saved.played_ticks,
                saved_unix_seconds: 0,
            },
            &saves::SceneCheckpoint::Field(saved.clone()),
        )?;
        let mut session = finish(loading::Pending::start(
            root.clone(),
            None,
            Some(bytes.clone()),
            None,
            &resident,
        )?)?;
        let audio = validation::Playback::new(
            session.audio.take().context("missing field audio")?,
            session.field_mut(),
        );
        configure_session(&mut session);
        session
            .field_mut()
            .events
            .world
            .request_battle(resonance_events::battle::Setup {
                route: [0; 5],
                encounter: resonance_events::battle::Encounter::Formation(1),
                arena: 13,
                defeat: resonance_events::battle::DefeatPolicy::GameOver,
                music: None,
            })
            .map_err(anyhow::Error::msg)?;
        let request = session
            .field_mut()
            .events
            .world
            .battle_request
            .take()
            .unwrap();
        let field_tick = session.field().events.tick();
        let party = serde_json::to_value(&session.field().events.world.party)?;
        let entry = Entry::capture(&session, &request, &resident)?;
        let mut package = finish(loading::BattlePending::battle(entry, &resident)?)?;
        let directory = tempfile::Builder::new()
            .prefix(&format!("resonance-host-{label}-"))
            .tempdir()?;
        let store = Store::new(directory.path());
        store.write(Kind::Save, &SlotId::new("a-001")?, &bytes)?;
        let title = crate::title_field::prepare(&library, false, resident.diagnostics.clone())?;
        let manifest = title.art;
        let title_scene = title.scene;
        let title_events = title.events;
        let mut app = App::new();
        crate::texture::decoded_files(&mut app, &root);
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..Default::default()
            },
        ))
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::NONE,
        ))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<WorldAsset>()
        .init_asset::<crate::sparse_animation::Clip>()
        .init_asset::<crate::field_ui::Surface>()
        .init_resource::<crate::field_ui::Resident>()
        .init_asset::<crate::materials::TitleSurface>()
        .init_asset::<crate::materials::TitleOutput>()
        .init_asset::<crate::audio::GameAudio>()
        .init_asset::<crate::field_audio::FieldSource>()
        .init_resource::<crate::scene::FieldAssets>()
        .init_resource::<crate::scene::SampledImages>()
        .insert_resource(title_scene)
        .insert_resource(crate::session_screen::Title {
            events: Some(title_events),
            audio: None,
            files: Some(title.files),
        })
        .init_resource::<crate::field_view::Controls>()
        .init_resource::<crate::field_view::Prediction>()
        .init_resource::<input::Controls>()
        .init_resource::<crate::input_icons::DeviceTracker>()
        .insert_resource(MenuMemory {
            character: 2,
            unison_character: 3,
            ..Default::default()
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::PendingInput>()
        .init_resource::<crate::display::Display>()
        .insert_resource(crate::display::OutputStage::Framebuffer)
        .init_resource::<crate::audio::MenuSounds>()
        .insert_resource(crate::Clock(Default::default()))
        .insert_resource(crate::timing::Ready(true))
        .insert_resource(crate::PendingAudio(None))
        .insert_resource(crate::Menu(resonance_game::TitleState {
            opacity: 137,
            ..Default::default()
        }))
        .insert_resource(crate::Art {
            manifest,
            images: vec![],
        })
        .insert_resource(crate::RunOptions {
            speculative: false,
            assets: root.clone(),
            mods,
            script_root: None,
            saves: Default::default(),
            capture: None,
            presentation_start: None,
            replay: None,
            capture_at: None,
            reveal: false,
            silent: true,
            paranoid: true,
            skip_intro: true,
            record_playthrough: None,
            record_title_ticks: 0,
            skip_battles: false,
            allow_incomplete_scripts: false,
        })
        .insert_resource(resident.clone())
        .insert_resource(audio.control())
        .insert_resource(session)
        .add_systems(
            PreUpdate,
            (input::gather, crate::field_view::gather_controls),
        )
        .add_systems(
            FixedUpdate,
            crate::field_view::advance_live.run_if(field_running),
        )
        .add_systems(FixedUpdate, advance.after(crate::field_view::advance_live));
        saves::install(
            &mut app,
            &saves::SaveOptions {
                directory: Some(directory.path().into()),
                ..Default::default()
            },
            false,
        )?;
        game_over::install(&mut app)?;
        app.world_mut()
            .spawn((Camera::default(), Projection::default(), crate::FieldCamera));
        app.world_mut()
            .spawn((Camera::default(), crate::FieldOverlayCamera));
        let title_root = app
            .world_mut()
            .spawn((crate::scene::PartRoot, Visibility::Hidden))
            .id();
        let world = app.world_mut();
        let image = world.resource_mut::<Assets<Image>>().add(Image::default());
        let output = world
            .resource_mut::<Assets<crate::materials::TitleOutput>>()
            .add(crate::materials::TitleOutput {
                overlay: None,
                source: image.clone(),
                brightness: Vec4::ONE,
            });
        configure_scene(world, &mut package);
        let mut scene = construct_scene(world, package, RenderTarget::default())?;
        let art = scene.game_over.as_mut().context("fatal art missing")?;
        art.prepare(&mut world.commands());
        world.flush();
        // Construction admits CPU images. Only final GPU activation is injected;
        // these lifecycle tests make no rendering claim.
        scene.state = SceneState::Active;
        begin_audio(world, &scene)?;
        *resident.files.write().unwrap() =
            Some(app.world().resource::<new_game::Session>().files());
        resident.active.store(true, Ordering::Release);
        resident.battle.store(true, Ordering::Release);
        app.insert_resource(Owner {
            request: request.clone(),
            phase: Phase::Scene(Box::new(scene)),
            input_tick: 0,
            publish_field: false,
        });
        // Transfer the running decoder to the real output owner. Observations
        // retain only a weak reference, so despawning its Sink releases it.
        let source = Arc::new(Mutex::new(audio));
        let audio = Arc::downgrade(&source);
        let (control, mixer) = resonance_playback::Offline::new();
        let audio_handle = control.play(false, move || Ok(Box::new(SharedAudio(source))))?;
        let audio_entity = app
            .world_mut()
            .spawn((
                crate::audio_output::Player::<crate::field_audio::FieldSource>(Handle::default()),
                crate::audio_output::Sink(audio_handle.clone()),
            ))
            .id();
        Ok(Self {
            app,
            audio,
            mixer,
            audio_entity,
            audio_handle,
            title_root,
            request,
            saved,
            store,
            _directory: directory,
            _output: output,
            field_tick,
            party,
            completions: 0,
        })
    }

    fn scene(&self) -> &Scene {
        let Phase::Scene(scene) = &self.app.world().resource::<Owner>().phase else {
            panic!("fixture has no battle scene");
        };
        scene
    }

    fn step(&mut self, key: Option<KeyCode>) -> Result<()> {
        let world = self.app.world_mut();
        let mut keys = ButtonInput::<KeyCode>::default();
        if let Some(key) = key {
            keys.press(key);
        }
        world.insert_resource(keys);
        world.resource_mut::<crate::Clock>().0.advance();
        world.run_schedule(PreUpdate);
        world.run_schedule(FixedUpdate);
        if let Some(owner) = world.get_resource::<Owner>()
            && let Phase::Scene(scene) = &owner.phase
            && let Some(completed) = &scene.completed
        {
            assert_eq!(completed.result, resonance_battle::BattleResult::Defeat);
            assert!(scene.candidate.is_none());
            self.completions += 1;
        }
        // The live schedule runs manage after field preparation, then the
        // game-over and save UI passes. No GPU preparation pass is simulated.
        manage(world);
        saves::update(world);
        world.run_schedule(Update);
        world.flush();
        // These CPU scenarios replace GPU completion, while images use the real loader.
        if let Some(draws) = world.get_resource::<crate::field_ui::MenuDraws>() {
            draws
                .0
                .lock()
                .unwrap()
                .completed
                .store(true, Ordering::Release);
        }
        ensure!(
            world
                .get_resource::<Owner>()
                .is_none_or(|owner| !owner.failed()),
            "fatal host entered failed phase"
        );
        ensure!(
            world
                .get_resource::<Messages<AppExit>>()
                .is_none_or(|messages| messages.is_empty()),
            "fatal host requested application exit"
        );
        self.advance_audio(534)?;
        Ok(())
    }

    fn advance_audio(&mut self, frames: u32) -> Result<()> {
        for _ in 0..frames * 2 {
            let sample = self.mixer.next().context("offline mixer ended")?;
            ensure!(sample.is_finite(), "nonfinite field audio output");
        }
        if let Some(audio) = self.audio.upgrade() {
            audio.lock().unwrap().battle_state()?;
        } else {
            ensure!(
                self.app.world().get_entity(self.audio_entity).is_err()
                    && self.audio_handle.empty(),
                "audio decoder stopped before its owner retired"
            );
        }
        Ok(())
    }

    fn sample_battle(&mut self, frames: u32) -> Result<(bool, Option<i16>)> {
        self.advance_audio(frames)?;
        self.audio
            .upgrade()
            .context("field audio owner retired")?
            .lock()
            .unwrap()
            .battle_state()
    }

    fn assert_audio_retired(&self) {
        assert!(self.app.world().get_entity(self.audio_entity).is_err());
        assert!(self.audio_handle.empty(), "retired audio sink did not stop");
        assert!(
            self.audio.upgrade().is_none(),
            "mixer retained the retired decoder"
        );
    }

    fn enter_combat(&mut self) -> Result<()> {
        for _ in 0..900 {
            if self.scene().core.phase() != resonance_battle::BattlePhase::Entry {
                break;
            }
            self.step(None)?;
        }
        ensure!(
            self.scene().core.phase() == resonance_battle::BattlePhase::Combat,
            "encounter did not complete entry"
        );
        Ok(())
    }

    fn open_commands(&mut self, paranoid: bool) -> Result<()> {
        self.enter_combat()?;
        let key = {
            let mut owner = self.app.world_mut().resource_mut::<Owner>();
            let Phase::Scene(scene) = &mut owner.phase else {
                unreachable!()
            };
            scene.diagnostics = resonance_content::diagnostics::Diagnostics::new(paranoid);
            let physical = scene
                .settings
                .button_map
                .iter()
                .position(|&action| action == 3)
                .unwrap();
            [
                KeyCode::Enter,
                KeyCode::Escape,
                KeyCode::KeyX,
                KeyCode::Tab,
                KeyCode::KeyQ,
                KeyCode::KeyE,
                KeyCode::KeyZ,
            ][physical]
        };
        self.press(key)
    }

    fn enter_game_over(&mut self) -> Result<()> {
        // The lifecycle test owns defeat recognition and completion. This host
        // starts with that result and live music, then exercises the real transfer.
        let world = self.app.world_mut();
        world
            .resource::<crate::battle_audio::Playback>()
            .music(Some(96), 0)?;
        let gameplay_random = world
            .resource::<new_game::Session>()
            .field()
            .events
            .world
            .gameplay_random;
        let mut owner = world.resource_mut::<Owner>();
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!()
        };
        let mut party = scene.candidate.take().unwrap().persistent_party().clone();
        for member in &mut party.members {
            member.hp = 0;
        }
        scene.completed = Some(results::Completed {
            party,
            gameplay_random,
            result: resonance_battle::BattleResult::Defeat,
        });
        self.step(None)?;
        assert!(
            self.app.world().contains_resource::<game_over::Active>(),
            "fatal transfer did not finish"
        );
        let session = self.app.world().resource::<new_game::Session>();
        assert_eq!(session.field().events.tick(), self.field_tick);
        assert_eq!(
            serde_json::to_value(&session.field().events.world.party)?,
            self.party
        );
        assert!(self.request.is_pending());
        assert!(!self.app.world().contains_resource::<Owner>());
        assert_eq!(self.completions, 1);
        assert!(
            self.app
                .world()
                .contains_resource::<crate::battle_audio::Playback>()
        );
        assert!(
            self.app
                .world()
                .resource::<loading::Resident>()
                .battle
                .load(Ordering::Acquire)
        );
        assert_eq!(
            self.sample_battle(1)?,
            (true, Some(96)),
            "fatal transfer stopped defeat music"
        );
        let played = self
            .app
            .world()
            .resource::<new_game::Session>()
            .field()
            .play_time
            .session();
        for _ in 0..40 {
            self.step(None)?;
        }
        assert!(self.request.is_pending());
        assert_eq!(
            self.app
                .world()
                .resource::<new_game::Session>()
                .field()
                .events
                .tick(),
            self.field_tick
        );
        assert_eq!(
            self.app
                .world()
                .resource::<new_game::Session>()
                .field()
                .play_time
                .session(),
            played
        );
        Ok(())
    }

    fn choose(&mut self, title: bool) -> Result<()> {
        if title {
            self.step(Some(KeyCode::ArrowDown))?;
            self.step(None)?;
        }
        self.step(Some(KeyCode::Enter))?;
        assert_eq!(
            self.sample_battle(1)?.1,
            None,
            "confirmation did not stop music"
        );
        for _ in 0..33 {
            self.step(None)?;
        }
        assert!(!self.request.is_pending());
        assert!(
            self.request
                .complete(resonance_events::battle::Outcome::Victory)
                .is_err()
        );
        assert!(!self.app.world().contains_resource::<new_game::Session>());
        Ok(())
    }

    fn wait_slots(&mut self) -> Result<()> {
        let start = Instant::now();
        while self
            .app
            .world()
            .resource::<saves::title::TitleMenu>()
            .0
            .busy
            || self
                .app
                .world()
                .get_resource::<crate::field_ui::MenuOverlay>()
                .is_none_or(|art| !art.ready(self.app.world().resource::<Assets<Image>>()))
        {
            ensure!(
                start.elapsed() < Duration::from_secs(30),
                "load slot scan timed out"
            );
            self.step(None)?;
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
    fn press(&mut self, key: KeyCode) -> Result<()> {
        if self
            .app
            .world()
            .contains_resource::<saves::title::TitleMenu>()
        {
            self.wait_slots()?;
        }
        self.step(Some(key))?;
        self.step(None)
    }

    fn draw_menu(&mut self) -> Result<bool> {
        self.draw_menu_frame(render_menu)
    }

    fn draw_hud_and_menu(&mut self) -> Result<bool> {
        self.draw_menu_frame(render_hud)
    }

    fn draw_menu_frame(
        &mut self,
        draw: impl Fn(
            &mut Scene,
            crate::field_ui::HudTime,
            &mut Commands,
            &Assets<Image>,
            &AssetServer,
        ) -> Result<bool>,
    ) -> Result<bool> {
        let began = Instant::now();
        loop {
            let world = self.app.world_mut();
            // Publish loaded images before borrowing the rendering resources.
            world.run_schedule(PreUpdate);
            let tick = world.resource::<crate::Clock>().0.tick();
            let mut owner = world.remove_resource::<Owner>().unwrap();
            let Phase::Scene(scene) = &mut owner.phase else {
                anyhow::bail!("battle scene missing");
            };
            let mut queue = bevy::ecs::world::CommandQueue::default();
            scene
                .hud
                .prepare(&mut Commands::new(&mut queue, world), Default::default());
            queue.apply(world);
            let result = draw(
                scene,
                crate::field_ui::HudTime {
                    tick,
                    ..Default::default()
                },
                &mut Commands::new(&mut queue, world),
                world.resource::<Assets<Image>>(),
                world.resource::<AssetServer>(),
            );
            queue.apply(world);
            if let Some(draws) = world.get_resource::<crate::field_ui::MenuDraws>() {
                draws
                    .0
                    .lock()
                    .unwrap()
                    .completed
                    .store(true, Ordering::Release);
            }
            let ready = scene.menu_ready;
            world.insert_resource(owner);
            let visible = result?;
            if ready || !visible {
                return Ok(visible);
            }
            ensure!(
                began.elapsed() < Duration::from_secs(30),
                "battle menu preparation timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn draw_commands(&mut self) -> Result<()> {
        let world = self.app.world_mut();
        let mut owner = world.remove_resource::<Owner>().unwrap();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!()
        };
        scene
            .hud
            .prepare(&mut Commands::new(&mut queue, world), Default::default());
        // This selector-only draw replaces any shared page.
        scene.hud.clear_menu(&mut Commands::new(&mut queue, world));
        queue.apply(world);
        let command = scene.lifecycle.command_frame();
        let result = render_command_strip(
            scene,
            command.as_ref(),
            Default::default(),
            &mut Commands::new(&mut queue, world),
        );
        queue.apply(world);
        world.insert_resource(owner);
        result
    }

    fn assert_title(&mut self, selected: usize) -> Result<()> {
        let world = self.app.world_mut();
        assert!(!world.contains_resource::<game_over::Active>());
        assert!(!world.contains_resource::<saves::title::TitleMenu>());
        assert!(!world.contains_resource::<crate::battle_audio::Playback>());
        assert!(!world.contains_resource::<new_game::Session>());
        let memory = world.resource::<MenuMemory>();
        assert_eq!((memory.character, memory.unison_character), (2, 3));
        assert!(
            world.contains_resource::<crate::Events>(),
            "title script was not restarted"
        );
        assert_eq!(world.resource::<crate::Events>().0.tick(), 0);
        assert_eq!(
            world.get::<Visibility>(self.title_root),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<crate::scene::PartRoot>>()
                .iter(world)
                .count(),
            1,
            "title return must reuse the retained scene"
        );
        assert!(world.contains_resource::<game_over::Returning>());
        assert!(
            !world
                .resource::<loading::Resident>()
                .battle
                .load(Ordering::Acquire)
        );
        assert!(!world.resource::<crate::timing::Ready>().0);
        let menu = &world.resource::<crate::Menu>().0;
        assert_eq!((menu.selected, menu.opacity, menu.tick), (selected, 137, 0));
        world.resource_mut::<crate::Menu>().0.tick = 17;
        for _ in 0..5 {
            self.step(None)?;
        }
        assert_eq!(
            self.app.world().resource::<crate::Menu>().0.tick,
            17,
            "title destination repeated"
        );
        self.assert_audio_retired();
        Ok(())
    }
}

#[test]
#[ignore = "requires current cooked battle assets; CPU menu composition and pause ownership only"]
fn translucent_battle_pages_preserve_the_hud_and_pause_without_a_capture() -> Result<()> {
    let mut fixture = Fixture::new("menu-underlay")?;
    fixture.enter_combat()?;
    assert!(!fixture.draw_hud_and_menu()?);
    let ordinary_hud: Vec<_> = fixture.scene().hud.entities().collect();
    // Record combat layers before opening commands adds blinking menu ornaments.
    let underlay: Vec<_> = ordinary_hud
        .iter()
        .copied()
        .filter(|&entity| {
            fixture.app.world().get::<Visibility>(entity) == Some(&Visibility::Inherited)
        })
        .collect();
    assert!(!underlay.is_empty(), "combat HUD must be visible");
    {
        let mut owner = fixture.app.world_mut().resource_mut::<Owner>();
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!()
        };
        let mut frame = scene.frame.clone().unwrap();
        let target = frame
            .actors
            .iter()
            .position(|actor| actor.side == Side::Enemy)
            .context("scan needs an enemy")?;
        frame.cues = vec![resonance_battle::Cue::EnemyScanned {
            actor: ActorId::from_index(target)?,
        }];
        scene.hud.advance(&frame, true)?;
    }
    assert!(!fixture.draw_hud_and_menu()?);
    // Scan backgrounds use the same windows as submenus. The no-menu pass
    // must preserve the panel just drawn by the ordinary HUD.
    let scan_panel: Vec<_> = fixture
        .scene()
        .hud
        .entities()
        .filter(|entity| !ordinary_hud.contains(entity))
        .collect();
    assert!(!scan_panel.is_empty());
    assert!(scan_panel.iter().all(|&entity| {
        fixture.app.world().get::<Visibility>(entity) == Some(&Visibility::Inherited)
    }));
    fixture.open_commands(true)?;
    for right_presses in [0, 2] {
        for _ in 0..right_presses {
            fixture.press(KeyCode::ArrowRight)?;
        }
        assert!(!fixture.draw_hud_and_menu()?);
        fixture.press(KeyCode::Enter)?;
        assert!(fixture.draw_hud_and_menu()?);
        let combat_tick = fixture.scene().core.ledger().combat_ticks;
        let actors = fixture.scene().frame.as_ref().unwrap().actors.clone();
        // Several fixed updates can run without drawing. No capture fence may
        // block input, and the paused battle poses remain the menu background.
        for _ in 0..60 {
            fixture.step(None)?;
        }
        assert!(fixture.draw_hud_and_menu()?);
        assert_eq!(fixture.scene().core.ledger().combat_ticks, combat_tick);
        assert_eq!(fixture.scene().frame.as_ref().unwrap().actors, actors);
        assert!(underlay.iter().all(|&entity| {
            fixture.app.world().get::<Visibility>(entity) == Some(&Visibility::Inherited)
        }));
        let menu_layers: Vec<_> = fixture
            .scene()
            .hud
            .entities()
            .filter(|entity| !ordinary_hud.contains(entity))
            .filter(|&entity| {
                fixture.app.world().get::<Visibility>(entity) == Some(&Visibility::Inherited)
            })
            .collect();
        assert!(
            !menu_layers.is_empty(),
            "shared page must have visible layers"
        );
        fixture.press(KeyCode::Escape)?;
        for _ in 0..40 {
            fixture.step(None)?;
        }
        assert!(
            !fixture.draw_hud_and_menu()?,
            "closing must hide the shared page"
        );
        assert!(
            menu_layers.iter().all(|&entity| {
                fixture.app.world().get::<Visibility>(entity) == Some(&Visibility::Hidden)
            }),
            "closing must hide the previously visible shared-page geometry"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires cooked battle assets; CPU battle recorder and host shortcut ownership"]
fn battle_arte_recorder_owns_function_key_down_and_release_before_host_actions() -> Result<()> {
    use crate::input::{HostInput, InspectionKey::Unavailable};
    use crate::test_support::send_key;
    use crate::testing::Controls as TestingControls;
    use bevy::input::{ButtonState, keyboard::KeyboardInput};
    use resonance_content::input::StandardAction;

    let mut fixture = Fixture::configured(
        "recorder-host-controls",
        |session| {
            // The shared fixture starts with no learned artes. Its remembered
            // Artes character is Genis; teach a valid entry before preparing
            // the battle so Confirm can open the recorder through normal UI.
            let technique = session.data().characters[2]
                .allowed_techniques
                .iter()
                .copied()
                .find(|&technique| technique != 0)
                .expect("Genis has an allowed arte");
            session.events_mut().world.party.as_mut().unwrap().members[2]
                .techniques
                .insert(technique);
        },
        |_, _| {},
    )?;
    let learned = *fixture
        .scene()
        .candidate
        .as_ref()
        .unwrap()
        .persistent_party()
        .members[2]
        .techniques
        .iter()
        .next()
        .expect("fixture taught an arte");
    fixture.open_commands(true)?;
    fixture.draw_hud_and_menu()?;
    fixture.press(KeyCode::Enter)?; // Open the real battle Artes page.
    fixture.draw_menu()?;
    let window = fixture
        .app
        .world_mut()
        .spawn((Window::default(), bevy::window::PrimaryWindow))
        .id();
    fixture
        .app
        .init_resource::<TestingControls>()
        .init_resource::<crate::boot::Playback>()
        .init_resource::<crate::movie::Playback>()
        .add_message::<KeyboardInput>()
        .configure_sets(PreUpdate, crate::input::HostActions.before(input::gather))
        .add_systems(
            PreUpdate,
            crate::timing::live_gameplay
                .pipe(crate::testing::controls)
                .in_set(crate::input::HostActions),
        );
    crate::input::install(&mut fixture.app);
    // Freeze on the open Artes page, then step the Confirm that opens the recorder.
    for key in [None, Some(KeyCode::Enter)] {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            send_key(fixture.app.world_mut(), window, KeyCode::F9, state, false);
        }
        fixture.step(key)?;
        assert!(fixture.app.world().resource::<TestingControls>().stepping());
    }
    let Some(resonance_game::battle::command::Frame {
        view: resonance_game::battle::command::View::Tech(recording),
        ..
    }) = fixture.scene().lifecycle.command_frame()
    else {
        anyhow::bail!("battle Artes page was not opened");
    };
    assert_eq!(recording.binding_technique, learned);

    for (index, key_code) in [
        KeyCode::F9,
        KeyCode::F1,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F10,
    ]
    .into_iter()
    .enumerate()
    {
        if index != 0 {
            fixture.press(KeyCode::Enter)?;
        }
        assert!(
            fixture
                .app
                .world()
                .resource::<Owner>()
                .capturing_arte_binding()
        );
        for state in [ButtonState::Pressed, ButtonState::Released] {
            send_key(fixture.app.world_mut(), window, key_code, state, false);
            fixture.step((state == ButtonState::Pressed).then_some(key_code))?;
            let controls = fixture.app.world().resource::<TestingControls>();
            assert!(
                !controls.paused
                    && controls.speed == resonance_playback::Speed::Normal
                    && !controls.skipping
            );
            assert!(!fixture.app.world().resource::<Time<Virtual>>().is_paused());
            let host = fixture.app.world().resource::<HostInput>();
            assert!(!host.slow_motion);
            assert_eq!(host.tick_step, Unavailable);
            assert!(
                StandardAction::HOST
                    .into_iter()
                    .all(|action| !host.pressed(action))
            );
        }
        assert!(
            !fixture
                .app
                .world()
                .resource::<Owner>()
                .capturing_arte_binding()
        );
        let Some(resonance_game::battle::command::Frame {
            view: resonance_game::battle::command::View::Tech(state),
            ..
        }) = fixture.scene().lifecycle.command_frame()
        else {
            anyhow::bail!("recorder lost the battle Artes page");
        };
        if matches!(key_code, KeyCode::F9 | KeyCode::F10) {
            assert_eq!(state.focus, resonance_game::menu::techniques::Focus::List);
            // The battle's candidate party owns this binding immediately after recording.
            for state in [ButtonState::Pressed, ButtonState::Released] {
                send_key(fixture.app.world_mut(), window, key_code, state, false);
                fixture.step((state == ButtonState::Pressed).then_some(key_code))?;
                let host = fixture.app.world().resource::<HostInput>();
                assert!(!host.slow_motion);
                if key_code == KeyCode::F9 {
                    assert_eq!(host.tick_step, Unavailable);
                }
            }
        } else {
            assert_eq!(
                state.focus,
                resonance_game::menu::techniques::Focus::BindingError
            );
            fixture.draw_menu()?;
            fixture.press(KeyCode::Enter)?; // Dismiss the protected-input error.
        }
        fixture.draw_menu()?;
    }
    // The guard ends with the recorder; the same production Pause handler then
    // pauses and resumes on new presses while the battle page remains open.
    for paused in [true, false] {
        send_key(
            fixture.app.world_mut(),
            window,
            KeyCode::F7,
            ButtonState::Pressed,
            false,
        );
        fixture.step(Some(KeyCode::F7))?;
        assert_eq!(
            fixture.app.world().resource::<TestingControls>().paused,
            paused
        );
        fixture.step(None)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets; CPU menu publication and input only"]
fn failed_battle_page_hides_stale_geometry_and_returns_input_to_commands() -> Result<()> {
    use resonance_game::battle::command::{InputKind, View};
    for paranoid in [false, true] {
        let mut fixture = Fixture::with_package(&format!("menu-{paranoid}"), |package| {
            for caption in Arc::make_mut(&mut package.menus)
                .presentation
                .strategy
                .as_mut()
                .unwrap()
                .groups
                .iter_mut()
                .flatten()
                .flatten()
            {
                caption.description = "Uncooked \u{10ffff}".into();
            }
        })?;
        fixture.open_commands(paranoid)?;
        fixture.press(KeyCode::ArrowRight)?;
        fixture.press(KeyCode::ArrowRight)?;
        fixture.press(KeyCode::Enter)?;
        assert!(fixture.draw_menu()?);
        for _ in 0..20 {
            fixture.step(None)?;
        }
        assert!(fixture.draw_menu()?);
        let (visible, before) = {
            let world = fixture.app.world();
            let scene = fixture.scene();
            assert!(matches!(
                scene.lifecycle.command_frame().unwrap().view,
                View::Strategy(_)
            ));
            let visible: Vec<_> = scene
                .hud
                .entities()
                .filter(|&entity| {
                    world
                        .get::<Visibility>(entity)
                        .is_some_and(|visibility| *visibility != Visibility::Hidden)
                })
                .collect();
            let before = serde_json::to_value(
                scene
                    .candidate
                    .as_ref()
                    .unwrap()
                    .strategy_page(&Default::default())
                    .party,
            )?;
            (visible, before)
        };
        assert!(!visible.is_empty());
        // Entering the selected character's settings reveals an uncooked
        // description after a successful page draw retained visible geometry.
        fixture.press(KeyCode::Enter)?;
        let result = fixture.draw_menu();
        assert_eq!(result.is_err(), paranoid);
        assert!(
            fixture.scene().diagnostics.entries().iter().any(|entry| {
                entry.scope == "battle menu page" && entry.message.contains("glyph")
            })
        );
        assert!(
            visible
                .iter()
                .all(|&entity| fixture.app.world().get::<Visibility>(entity)
                    == Some(&Visibility::Hidden))
        );
        if paranoid {
            continue;
        }
        assert!(!result?);
        {
            let scene = fixture.scene();
            assert!(matches!(
                scene.lifecycle.command_frame().unwrap().view,
                View::Strip
            ));
            assert_eq!(scene.lifecycle.command_input_kind(), InputKind::Command);
        }
        fixture.press(KeyCode::ArrowRight)?;
        fixture.press(KeyCode::Escape)?;
        let scene = fixture.scene();
        assert!(scene.lifecycle.command_frame().is_none());
        assert_eq!(
            serde_json::to_value(
                scene
                    .candidate
                    .as_ref()
                    .unwrap()
                    .strategy_page(&Default::default())
                    .party
            )?,
            before
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets; CPU selector geometry and input only"]
fn failed_item_selector_hides_previous_actor_and_recovers_input_by_policy() -> Result<()> {
    use resonance_game::battle::command::{InputKind, View};
    for paranoid in [false, true] {
        let mut fixture = Fixture::with_package(&format!("selector-{paranoid}"), |package| {
            let first = usize::from(package.party.formation[0] - 1);
            let second = usize::from(package.party.formation[1] - 1);
            package.party.members[first].name = Some("Actor A".into());
            package.party.members[second].name = Some("B\u{10ffff}".into());
            package.party.items = [(2, 5)].into();
        })?;
        fixture.open_commands(paranoid)?;
        for _ in 0..4 {
            fixture.press(KeyCode::ArrowRight)?;
        }
        fixture.press(KeyCode::Enter)?;
        fixture.press(KeyCode::Enter)?;
        assert!(fixture.draw_menu()?);
        for _ in 0..30 {
            fixture.step(None)?;
        }
        fixture.press(KeyCode::Enter)?;
        for _ in 0..30 {
            fixture.step(None)?;
        }
        fixture.draw_commands()?;
        let (selected, visible, before) = {
            let world = fixture.app.world();
            let scene = fixture.scene();
            let command = scene.lifecycle.command_frame().unwrap();
            let selection = match &command.view {
                View::Ally(selection) => selection,
                view => panic!("unexpected item selector: {view:?}"),
            };
            assert_eq!(selection.name, "Actor A");
            let visible: Vec<_> = scene
                .hud
                .entities()
                .filter(|&entity| {
                    world
                        .get::<Visibility>(entity)
                        .is_some_and(|visibility| *visibility != Visibility::Hidden)
                })
                .collect();
            (
                selection.actor,
                visible,
                serde_json::to_value(scene.candidate.as_ref().unwrap().persistent_party())?,
            )
        };
        assert!(!visible.is_empty(), "actor A must have published geometry");
        fixture.press(KeyCode::ArrowRight)?;
        {
            let scene = fixture.scene();
            let command = scene.lifecycle.command_frame().unwrap();
            let (View::User(selection) | View::Ally(selection)) = &command.view else {
                unreachable!()
            };
            assert_ne!(selection.actor, selected);
            assert_eq!(selection.name, "B\u{10ffff}");
        }
        let result = fixture.draw_commands();
        assert_eq!(result.is_err(), paranoid);
        assert!(
            visible
                .iter()
                .all(|&entity| fixture.app.world().get::<Visibility>(entity)
                    == Some(&Visibility::Hidden)),
            "failed actor B drawing retained actor A geometry"
        );
        {
            let scene = fixture.scene();
            assert_eq!(scene.diagnostics.entries().len(), 1);
            assert_eq!(scene.diagnostics.entries()[0].scope, "battle command strip");
            assert!(scene.core.pending_item().is_none());
            assert_eq!(
                serde_json::to_value(scene.candidate.as_ref().unwrap().persistent_party())?,
                before
            );
            if !paranoid {
                assert!(matches!(
                    scene.lifecycle.command_frame().unwrap().view,
                    View::Strip
                ));
                assert_eq!(scene.lifecycle.command_input_kind(), InputKind::Command);
            }
        }
        if paranoid {
            continue;
        }
        result?;
        fixture.draw_commands()?;
        // Confirmation reopens Item's user selection; it cannot consume the
        // stale target. A healthy actor then redraws and accepts input.
        fixture.press(KeyCode::Enter)?;
        {
            let scene = fixture.scene();
            assert!(matches!(
                scene.lifecycle.command_frame().unwrap().view,
                View::User(_)
            ));
            assert!(scene.core.pending_item().is_none());
        }
        fixture.press(KeyCode::ArrowLeft)?;
        fixture.draw_commands()?;
        fixture.press(KeyCode::Enter)?;
        let scene = fixture.scene();
        assert!(matches!(
            scene.lifecycle.command_frame().unwrap().view,
            View::Inventory(_)
        ));
        assert_eq!(
            serde_json::to_value(scene.candidate.as_ref().unwrap().persistent_party())?,
            before
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn fatal_completion_keeps_request_and_music_then_loads_a_new_field_once() -> Result<()> {
    let mut fixture = Fixture::new("load")?;
    fixture.enter_game_over()?;
    fixture.choose(false)?;
    fixture.wait_slots()?;
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<saves::title::TitleMenu>()
            .0
            .focus,
        SlotFocus::Bank
    );
    fixture.press(KeyCode::Enter)?;
    fixture.press(KeyCode::Enter)?;
    fixture.press(KeyCode::Enter)?;
    let start = Instant::now();
    while !fixture.app.world().contains_resource::<new_game::Session>() {
        ensure!(
            start.elapsed() < Duration::from_secs(120),
            "saved field load timed out"
        );
        fixture.step(None)?;
        std::thread::sleep(Duration::from_millis(1));
    }
    let world = fixture.app.world();
    assert!(!world.contains_resource::<game_over::Active>());
    assert!(!world.contains_resource::<saves::title::TitleMenu>());
    assert!(!world.contains_resource::<crate::battle_audio::Playback>());
    fixture.assert_audio_retired();
    assert!(
        !world
            .resource::<loading::Resident>()
            .battle
            .load(Ordering::Acquire)
    );
    let session = world.resource::<new_game::Session>();
    assert_eq!(session.map_id(), fixture.saved.map_id);
    ensure!(
        serde_json::to_value(&session.field().events.world.party)?
            == serde_json::to_value(Some(&fixture.saved.progress.party))?,
        "loaded party differs from saved party"
    );
    let tick = session.field().events.tick();
    for _ in 0..5 {
        fixture.step(None)?;
    }
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<new_game::Session>()
            .field()
            .events
            .tick(),
        tick
    );
    assert!(
        !fixture
            .app
            .world()
            .contains_resource::<game_over::Returning>()
    );
    assert!(!fixture.request.is_pending());
    let memory = fixture.app.world().resource::<MenuMemory>();
    assert_eq!((memory.character, memory.unison_character), (2, 3));
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn failed_fatal_load_stays_in_menu_and_cancel_restarts_title_once() -> Result<()> {
    let mut fixture = Fixture::new("failure")?;
    fixture.enter_game_over()?;
    fixture.choose(false)?;
    fixture.wait_slots()?;
    // The actual load re-reads and validates a file changed after slot scanning.
    std::fs::write(
        fixture.store.path(Kind::Save, &SlotId::new("a-001")?),
        b"invalid checkpoint",
    )?;
    fixture.press(KeyCode::Enter)?;
    fixture.press(KeyCode::Enter)?;
    fixture.press(KeyCode::Enter)?;
    let start = Instant::now();
    while fixture
        .app
        .world()
        .resource::<saves::title::TitleMenu>()
        .0
        .notice
        .is_none()
    {
        ensure!(
            start.elapsed() < Duration::from_secs(30),
            "invalid load did not report failure"
        );
        fixture.step(None)?;
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(fixture.app.world().contains_resource::<game_over::Active>());
    assert!(!fixture.app.world().contains_resource::<new_game::Session>());
    assert!(
        !fixture
            .app
            .world()
            .resource::<saves::title::TitleMenu>()
            .0
            .busy
    );
    for tick in 0..120 {
        if !fixture.app.world().contains_resource::<game_over::Active>() {
            break;
        }
        fixture.step((tick % 12 == 0).then_some(KeyCode::Escape))?;
    }
    fixture.assert_title(1)
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn fatal_quit_cancels_the_request_and_restarts_title_once() -> Result<()> {
    for saved in [true, false] {
        let mut fixture = Fixture::new("title")?;
        if !saved {
            std::fs::remove_file(fixture.store.path(Kind::Save, &SlotId::new("a-001")?))?;
        }
        fixture.enter_game_over()?;
        fixture.choose(true)?;
        fixture.assert_title(usize::from(saved))?;
    }
    Ok(())
}

fn completed_white_fade(session: &mut new_game::Session) {
    session.events_mut().world.fade = Some(resonance_events::Fade {
        start_tick: 0,
        duration: 0,
        from: 255.,
        to: 255.,
        white: true,
    });
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn battle_activation_retains_the_white_field_cover_without_a_shatter() -> Result<()> {
    let fixture = Fixture::configured("white-entry", completed_white_fade, |_, _| {})?;
    let scene = fixture.scene();
    let mut frame = scene.core.snapshot();
    scene.lifecycle.apply_view(&mut frame)?;
    let fade = frame
        .event_fade
        .context("battle activation lost the cover")?;
    assert_eq!((fade.color, fade.alpha), ([255; 3], 255));
    assert!(fixture.request.is_pending());
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn shatter_releases_the_camera_on_a_fixed_update_without_a_render() -> Result<()> {
    for ready_on in [3, 26] {
        check_camera_release(ready_on)?;
    }
    Ok(())
}

fn check_camera_release(ready_on: u32) -> Result<()> {
    let mut fixture = Fixture::configured("shatter-reveal", completed_white_fade, |world, _| {
        world.insert_resource(transition::State::captured_for_test());
    })?;
    transition::install(&mut fixture.app);
    fixture
        .app
        .init_resource::<crate::boot::Playback>()
        .init_resource::<crate::movie::Playback>()
        .add_systems(FixedUpdate, crate::timing::advance_clock.before(advance));
    let world = fixture.app.world_mut();
    world
        .resource::<loading::Resident>()
        .active
        .store(false, Ordering::Release);
    {
        let mut owner = world.resource_mut::<Owner>();
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!()
        };
        scene.state = SceneState::Activating;
        let mut frame = scene.core.snapshot();
        scene.lifecycle.apply_view(&mut frame)?;
        scene.frame = Some(frame);
        if scene.feedback.resources.entry_voice.is_none() {
            let taunt = scene.feedback.resources.actors[0]
                .taunt
                .context("fixture has no battle voice")?;
            scene.feedback.resources.entry_voice = Some((ActorId::from_index(0)?, taunt));
        }
    }
    let initial = world.resource::<crate::Clock>().0.tick();
    let played = world.resource::<new_game::Session>().play_time().total();
    // Slow preparation holds the stationary cracks, then runs the same opening.
    let extra_wait = ready_on.saturating_sub(23);
    let mut previous_focus = 0.;
    for tick in 1..=82 + extra_wait {
        let age = if tick < ready_on {
            tick.min(22)
        } else {
            tick - extra_wait
        };
        if tick == ready_on {
            world.resource_scope(|world, mut shatter: Mut<transition::State>| {
                let mut owner = world.resource_mut::<Owner>();
                let Phase::Scene(scene) = &mut owner.phase else {
                    unreachable!()
                };
                shatter.start_shatter(scene);
            });
        }
        world.run_schedule(FixedUpdate);
        assert_eq!(world.resource::<crate::Clock>().0.tick() - initial, age);
        assert_eq!(
            world.resource::<new_game::Session>().play_time().total() - played,
            u64::from(age)
        );
        let owner = world.resource::<Owner>();
        assert_eq!(owner.view_ready(), tick >= ready_on);
        assert_eq!(owner.presenting(), tick >= ready_on && age >= 22);
        let battle_update = age.saturating_sub(22);
        assert_eq!(owner.input_tick, battle_update);
        let Phase::Scene(scene) = &owner.phase else {
            unreachable!()
        };
        let frame = scene.frame.as_ref().unwrap();
        assert!(
            frame.event_fade.is_none(),
            "shatter age {age} has an entry cover"
        );
        assert_eq!(frame.update, u64::from(battle_update));
        let focus = frame.camera.context("opening frame has no camera")?.focus[0];
        if (1..=59).contains(&battle_update) {
            assert!(focus < previous_focus, "camera held at shatter age {age}");
        }
        previous_focus = focus;
        assert!(scene.failure.is_none());
        assert_eq!(
            scene.feedback.resources.entry_voice.is_some(),
            tick < ready_on
        );
        assert_eq!(
            world.resource::<transition::State>().entry_voice.is_some(),
            tick >= ready_on && age <= 56
        );
        assert_eq!(
            world.resource::<transition::State>().finished(),
            battle_update == 60
        );
        assert_eq!(
            scene.core.ledger().combat_ticks,
            u32::from(battle_update == 60)
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn invalid_battle_music_keeps_field_audio_and_state() -> Result<()> {
    let mut fixture = Fixture::new("music-failure")?;
    let random_before = fixture
        .app
        .world()
        .resource::<new_game::Session>()
        .field()
        .events
        .world
        .gameplay_random;
    let mut owner = fixture.app.world_mut().remove_resource::<Owner>().unwrap();
    let mut playback = fixture
        .app
        .world_mut()
        .remove_resource::<crate::battle_audio::Playback>()
        .unwrap();
    assert!(playback.finish(&mut fixture.app.world_mut().resource_mut(), true)?);
    let field_music = fixture.sample_battle(1)?;
    assert!(!field_music.0);
    let Phase::Scene(scene) = &mut owner.phase else {
        unreachable!()
    };
    scene.music = u16::MAX;
    assert!(begin_audio(fixture.app.world_mut(), scene).is_err());
    assert!(
        !fixture
            .app
            .world()
            .contains_resource::<crate::battle_audio::Playback>()
    );
    retire(fixture.app.world_mut(), owner, true);
    assert!(
        !fixture
            .app
            .world()
            .contains_resource::<crate::battle_audio::Playback>()
    );
    assert_eq!(fixture.sample_battle(1)?, field_music);
    let session = fixture.app.world().resource::<new_game::Session>();
    assert_eq!(session.field().events.tick(), fixture.field_tick);
    assert_eq!(session.field().events.world.gameplay_random, random_before);
    assert_eq!(
        serde_json::to_value(&session.field().events.world.party)?,
        fixture.party
    );
    assert!(fixture.request.is_pending());
    Ok(())
}

#[test]
#[ignore = "requires freshly cooked battle assets; CPU prediction and model/particle publication only"]
fn demon_fang_predicts_weapon_trails_without_changing_committed_battle() -> Result<()> {
    let mut fixture = Fixture::new("demon-fang-attachment")?;
    fixture.enter_combat()?;
    let actor = ActorId::from_index(0)?;
    {
        let mut owner = fixture.app.world_mut().resource_mut::<Owner>();
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!()
        };
        scene
            .core
            .record_technique_acquisition(actor, TechniqueId(1))?;
        let action = scene
            .core
            .prepared_technique(actor, TechniqueId(1))
            .context("Demon Fang was not prepared")?
            .action;
        scene
            .core
            .set_control_mode(actor, resonance_battle::Control::Auto)?;
        ensure!(
            scene.core.queue_technique(actor, action)?,
            "Demon Fang was not queued"
        );
    }
    let mut measurements = crate::test_support::PredictionMeasurements::new()?;
    let mut cache = interpolation::Cache::default();
    let initial_uses = fixture
        .scene()
        .core
        .technique_uses(actor, TechniqueId(1))
        .unwrap();
    let mut sampled_swing = false;
    for tick in 0..120 {
        let scene = fixture.scene();
        let input = fixture
            .app
            .world()
            .resource::<input::Controls>()
            .clone()
            .sample(&scene.settings);
        let before = scene.core.snapshot();
        let random = scene.core.random_state();
        let (predicted, sources) = measurements.measure(|storage| {
            storage.execute(|| {
                prediction::predict(
                    scene,
                    &input,
                    scene.hud.results_on_last_page(),
                    Default::default(),
                )
            })?
        })?;
        let predicted = predicted.context("Demon Fang prediction")?;
        assert_eq!(scene.core.snapshot(), before);
        assert_eq!(scene.core.random_state(), random);
        assert!(sources.sources_unchanged());
        let from = scene.frame.as_ref().unwrap();
        if let Some(slot) = from.cues.iter().find_map(|cue| match *cue {
            resonance_battle::Cue::WeaponTrail {
                actor: owner,
                slot,
                duration: 1..,
                ..
            } if owner == actor => Some(slot),
            _ => None,
        }) {
            let tip = |f: &BattleFrame| -> Result<Vec3> {
                let sword = f
                    .weapons
                    .iter()
                    .find(|w| w.owner == actor && w.slot == Some(slot) && w.visible)
                    .context("Demon Fang ribbon has no sword")?;
                ensure!(
                    sword.attachment.is_some(),
                    "Demon Fang sword lost its attachment"
                );
                let bone = sword.bones.last().context("Demon Fang sword has no tip")?;
                Ok(
                    (Mat4::from_cols_array_2d(&sword.world) * Mat4::from_cols_array_2d(bone))
                        .transform_point3(Vec3::ZERO),
                )
            };
            let (a, b) = (tip(from)?, tip(&predicted.frame)?);
            let travel = a.distance(b);
            if travel > 0.02 {
                let (frame, _) = cache.sample((tick, 1), from, &scene.particles, &predicted, 0.5);
                let mid = tip(frame)?;
                assert!(
                    [a, b]
                        .iter()
                        .all(|end| (0.005..travel).contains(&mid.distance(*end))),
                    "Demon Fang tip at the ribbon cue: {a:?}, {mid:?}, {b:?}"
                );
                sampled_swing = true;
            }
        }
        fixture.step(None)?;
        assert_eq!(predicted.frame, *fixture.scene().frame.as_ref().unwrap());
        assert_eq!(predicted.particles, fixture.scene().particles);
    }
    assert!(
        fixture
            .scene()
            .core
            .technique_uses(actor, TechniqueId(1))
            .unwrap()
            > initial_uses,
        "Demon Fang did not execute: clock {:?}, activity {:?}",
        fixture.scene().frame.as_ref().unwrap().clock,
        fixture.scene().core.activity(actor)
    );
    ensure!(
        sampled_swing,
        "Demon Fang did not sample a moving sword during its ribbon cue"
    );
    measurements.report("Battle arena 13");
    Ok(())
}
