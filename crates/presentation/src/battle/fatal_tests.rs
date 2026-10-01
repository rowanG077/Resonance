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
        let root = root();
        let files = Files::load(
            &root,
            &["fields/map-332.preload.json"],
            &mut Default::default(),
            || false,
        )?;
        let saved = field_checkpoint(&files)?;
        let resident = loading::Resident::default();
        let identity = resonance_persistence::Identity::load(&files)?;
        let bytes = resonance_persistence::encode(
            &Header {
                identity,
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
        let entry = Entry::capture(&session, request.setup)?;
        let mut package = finish(loading::BattlePending::battle(
            root.clone(),
            entry,
            &resident,
        )?)?;
        configure(&mut package);
        let directory = tempfile::Builder::new()
            .prefix(&format!("resonance-host-{label}-"))
            .tempdir()?;
        let store = Store::new(directory.path());
        store.write(Kind::Save, &SlotId::new("a-001")?, &bytes)?;
        let manifest: resonance_content::TitleAssets =
            serde_json::from_slice(&std::fs::read(root.join("title.json"))?)?;
        manifest.validate()?;
        ensure!(
            manifest.scene.is_some(),
            "fatal fixture requires prepared title scene"
        );
        let mut app = App::new();
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
        .init_asset::<crate::materials::TitleOutput>()
        .init_asset::<crate::menu_backdrop::Material>()
        .init_asset::<crate::audio::GameAudio>()
        .init_asset::<crate::field_audio::FieldSource>()
        .init_resource::<crate::sparse_animation::Prepared>()
        .init_resource::<crate::field_view::Controls>()
        .init_resource::<input::Controls>()
        .insert_resource(MenuMemory {
            character: 2,
            unison_character: 3,
            ..Default::default()
        })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::PendingInput>()
        .init_resource::<crate::display::Display>()
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
            assets: root.clone(),
            script_root: None,
            saves: Default::default(),
            capture: None,
            capture_at: None,
            reveal: false,
            selected: 0,
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
        )?;
        game_over::install(&mut app)?;
        app.world_mut()
            .spawn((Camera::default(), Projection::default(), crate::FieldCamera));
        app.world_mut()
            .spawn((Camera::default(), crate::FieldOverlayCamera));
        let world = app.world_mut();
        let image = world.resource_mut::<Assets<Image>>().add(Image::default());
        let output = world
            .resource_mut::<Assets<crate::materials::TitleOutput>>()
            .add(crate::materials::TitleOutput {
                source: image.clone(),
                brightness: Vec4::ONE,
                screen_offset: Vec2::ZERO,
            });
        let mut scene = construct_scene(world, package, image, RenderTarget::default())?;
        let art = scene.game_over.as_mut().context("fatal art missing")?;
        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
            art.prepare(&mut world.commands(), &mut meshes);
        });
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
        while self.app.world().resource::<saves::title::LoadMenu>().0.busy
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
            .contains_resource::<saves::title::LoadMenu>()
        {
            self.wait_slots()?;
        }
        self.step(Some(key))?;
        self.step(None)
    }

    fn draw_menu(&mut self) -> Result<bool> {
        let began = Instant::now();
        loop {
            let world = self.app.world_mut();
            // Publish loaded images before borrowing the rendering resources.
            world.run_schedule(PreUpdate);
            let tick = world.resource::<crate::Clock>().0.tick();
            let mut owner = world.remove_resource::<Owner>().unwrap();
            let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
            let mut materials = world
                .remove_resource::<Assets<crate::menu_backdrop::Material>>()
                .unwrap();
            let camera = world
                .query_filtered::<Entity, With<crate::FieldOverlayCamera>>()
                .single(world)?;
            let Phase::Scene(scene) = &mut owner.phase else {
                anyhow::bail!("battle scene missing");
            };
            let mut queue = bevy::ecs::world::CommandQueue::default();
            scene
                .hud
                .prepare(&mut Commands::new(&mut queue, world), &mut meshes);
            queue.apply(world);
            let result = render_menu(
                scene,
                tick,
                &mut Commands::new(&mut queue, world),
                &mut meshes,
                world.resource::<Assets<Image>>(),
                world.resource::<AssetServer>(),
            )
            .and_then(|visible| {
                scene.menu_backdrop.show(
                    visible,
                    Some(camera),
                    &mut Commands::new(&mut queue, world),
                    &mut materials,
                )?;
                Ok(visible)
            });
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
            world.insert_resource(meshes);
            world.insert_resource(materials);
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
        let mut meshes = world.remove_resource::<Assets<Mesh>>().unwrap();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!()
        };
        scene
            .hud
            .prepare(&mut Commands::new(&mut queue, world), &mut meshes);
        // The ordinary HUD dismisses the shared page before drawing selectors.
        scene.hud.clear_menu(&mut Commands::new(&mut queue, world));
        queue.apply(world);
        let result =
            render_command_strip(scene, &mut Commands::new(&mut queue, world), &mut meshes);
        queue.apply(world);
        world.insert_resource(meshes);
        world.insert_resource(owner);
        result
    }

    fn assert_title(&mut self) -> Result<()> {
        let world = self.app.world_mut();
        assert!(!world.contains_resource::<game_over::Active>());
        assert!(!world.contains_resource::<saves::title::LoadMenu>());
        assert!(!world.contains_resource::<crate::battle_audio::Playback>());
        assert!(!world.contains_resource::<new_game::Session>());
        let memory = world.resource::<MenuMemory>();
        assert_eq!((memory.character, memory.unison_character), (2, 3));
        assert!(
            world.contains_resource::<crate::Events>(),
            "title script was not restarted"
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
        assert_eq!((menu.selected, menu.opacity, menu.tick), (1, 137, 0));
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
#[ignore = "requires current cooked opening/title assets; CPU menu publication and input only"]
fn failed_battle_page_hides_stale_geometry_and_returns_input_to_commands() -> Result<()> {
    use resonance_game::battle::command::{InputKind, View};
    for paranoid in [false, true] {
        let mut fixture = Fixture::new(if paranoid {
            "menu-paranoid"
        } else {
            "menu-tolerant"
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
        // Remove the live cursor mesh, then move selection so the actual page
        // upload fails after a successful draw retained visible geometry.
        let world = fixture.app.world_mut();
        let cursor = visible
            .iter()
            .copied()
            .max_by(|&a, &b| {
                world
                    .get::<Transform>(a)
                    .unwrap()
                    .translation
                    .z
                    .total_cmp(&world.get::<Transform>(b).unwrap().translation.z)
            })
            .unwrap();
        let mesh = world.get::<Mesh2d>(cursor).unwrap().0.clone();
        world
            .resource_mut::<Assets<Mesh>>()
            .remove(mesh.id())
            .unwrap();
        fixture.press(KeyCode::ArrowDown)?;
        let result = fixture.draw_menu();
        assert_eq!(result.is_err(), paranoid);
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
            .resource::<saves::title::LoadMenu>()
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
    assert!(!world.contains_resource::<saves::title::LoadMenu>());
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
        .resource::<saves::title::LoadMenu>()
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
            .resource::<saves::title::LoadMenu>()
            .0
            .busy
    );
    for tick in 0..120 {
        if !fixture.app.world().contains_resource::<game_over::Active>() {
            break;
        }
        fixture.step((tick % 12 == 0).then_some(KeyCode::Escape))?;
    }
    fixture.assert_title()
}

#[test]
#[ignore = "requires current cooked opening/title assets; no GPU or audio device"]
fn fatal_quit_cancels_the_request_and_restarts_title_once() -> Result<()> {
    let mut fixture = Fixture::new("title")?;
    fixture.enter_game_over()?;
    fixture.choose(true)?;
    fixture.assert_title()
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
