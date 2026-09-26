//! Host ownership regression, without a window, GPU or audio device. Original
//! resources and native functions are used, but an initially KO party
//! candidate deliberately supplies the fatal boundary; this is not a natural
//! opening combat or image/audio-fidelity test.
use super::*;
use crate::{field_audio::validation, game_over, loading, new_game, saves};
use resonance_game::{
    field::{FieldCheckpoint, FieldInput},
    menu::SlotFocus,
};
use resonance_persistence::{Header, Kind, SlotId, Store};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    app: App,
    audio: validation::Playback,
    request: Request,
    saved: FieldCheckpoint,
    store: Store,
    _directory: Directory,
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

fn finish_battle(task: loading::BattlePending) -> Result<Package> {
    let start = Instant::now();
    let mut ready = None;
    loop {
        // This fatal-boundary fixture installs its own prepared scene/mixer;
        // consume the real worker handoff without starting a second owner.
        if let Some(audio) = task.poll_audio() {
            assert!(ready.replace(audio).is_none());
        }
        if let Some(result) = task.poll()? {
            let package = result?;
            let audio = ready.context("battle package preceded audio readiness")?;
            assert!(Arc::ptr_eq(&audio.assets, &package.audio));
            assert_eq!(audio.track, package.prepared.music);
            return Ok(package);
        }
        ensure!(
            start.elapsed() < Duration::from_secs(120),
            "fatal fixture battle preparation timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

impl Fixture {
    fn new(label: &str) -> Result<Self> {
        let root = root();
        let mut saved: FieldCheckpoint = serde_json::from_slice(&std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/battle-rewrite/stage3-opening-checkpoint.json"),
        )?)?;
        // The paired original savestate cde34b48… records previous formation 0;
        // see the entry-voice-history-01 audit. This is fixture supplementation,
        // not a default for legacy saves with unknown battle history.
        assert_eq!(saved.progress.party.battles.previous_formation, None);
        saved.progress.party.battles.previous_formation = Some(0);
        let resident = loading::Resident::default();
        let identity = new_game::Session::identity(&root)?;
        let bytes = resonance_persistence::encode(
            &Header {
                identity,
                label: "Fatal host fixture".into(),
                location: "Iselia".into(),
                played_ticks: saved.played_ticks(),
                saved_unix_seconds: 0,
            },
            &saved,
        )?;
        let mut session = finish(loading::Pending::start(
            root.clone(),
            None,
            Some(bytes.clone()),
            &resident,
        )?)?;
        let mut audio = validation::Playback::new(
            session.audio.take().context("missing field audio")?,
            &mut session.field,
        );
        ensure!(
            session.field.events.trigger(2002, false)?,
            "opening event did not start"
        );
        for tick in 0..6_000 {
            if session.field.events.world.battle_request.is_some() {
                break;
            }
            session.field.step(FieldInput {
                interact: tick % 12 == 0,
                ..Default::default()
            })?;
            audio.step(&mut session.field)?;
        }
        let request = session
            .field
            .events
            .world
            .battle_request
            .take()
            .context("opening event did not request battle")?;
        assert_eq!(
            request.setup.defeat,
            resonance_events::battle::DefeatPolicy::GameOver
        );
        let field_tick = session.field.events.tick();
        let party = serde_json::to_value(&session.field.events.world.party)?;
        let mut fatal_party = session.field.events.world.party.clone().unwrap();
        // Explicit policy fixture; preparation still performs every real check.
        // The original retained field party remains untouched. Keep its full
        // roster: normal preparation also verifies all potential group voices.
        for &character in &fatal_party.formation {
            fatal_party.members[usize::from(character - 1)].hp = 0;
            fatal_party.members[usize::from(character - 1)].conditions |= 0x8000_0000;
        }
        let package = finish_battle(loading::BattlePending::battle(
            root.clone(),
            Entry {
                files: session.files(),
                party: fatal_party,
                setup: request.setup,
                options: encounter::PrepareOptions {
                    random_seed: 1,
                    map: 332,
                    world_music: session
                        .field
                        .events
                        .memory()
                        .read(0x50, symphonia_script::Width::S32)?,
                    story: session.field.story_progress()?,
                    overlimit_boost: false,
                },
                data: session.data.clone(),
                libc_seed: session.field.events.world.random_state,
            },
            &resident,
        )?)?;
        let directory = Directory(
            std::env::temp_dir().join(format!("resonance-fatal-{label}-{}", std::process::id())),
        );
        std::fs::create_dir_all(&directory.0)?;
        let store = Store::new(directory.0.clone());
        store.write(Kind::Save, &SlotId::new("a-001")?, &bytes)?;
        let manifest: resonance_content::TitleAssets =
            serde_json::from_slice(&std::fs::read(root.join("title.json"))?)?;
        manifest.validate()?;
        ensure!(
            manifest.scene.is_some(),
            "fatal fixture requires original title scene"
        );
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..Default::default()
            },
        ))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<WorldAsset>()
        .init_asset::<crate::sparse_animation::Clip>()
        .init_asset::<crate::field_ui::Surface>()
        .init_asset::<crate::materials::TitleOutput>()
        .init_asset::<crate::audio::GameAudio>()
        .init_asset::<crate::field_audio::FieldSource>()
        .init_resource::<crate::sparse_animation::Prepared>()
        .init_resource::<crate::field_view::Controls>()
        .init_resource::<input::Controls>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::PendingInput>()
        .init_resource::<crate::PresentationPause>()
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
            assets: root,
            script_root: None,
            saves: Default::default(),
            tick: None,
            capture: None,
            reveal: false,
            selected: 0,
            silent: true,
            paranoid: true,
            replay: None,
            movie_frame: None,
            boot_frame: None,
            skip_intro: true,
            presentation_start: None,
            record_playthrough: None,
            record_title_ticks: 0,
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
                directory: Some(directory.0.clone()),
                ..Default::default()
            },
        )?;
        game_over::install(&mut app)?;
        app.world_mut()
            .spawn((Camera::default(), Projection::default(), crate::FieldCamera));
        app.world_mut()
            .spawn((Camera::default(), crate::FieldOverlayCamera));
        let scene = prepared_scene(app.world_mut(), package)?;
        *resident.files.write().unwrap() =
            Some(app.world().resource::<new_game::Session>().files());
        resident.active.store(true, Ordering::Release);
        resident.battle.store(true, Ordering::Release);
        app.insert_resource(Owner {
            request: request.clone(),
            phase: Phase::Scene(Box::new(scene)),
            input_tick: 0,
            music_paused: false,
            entry: None,
        });
        Ok(Self {
            app,
            audio,
            request,
            saved,
            store,
            _directory: directory,
            field_tick,
            party,
            completions: 0,
        })
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
        self.audio.sample_battle(534)?;
        Ok(())
    }

    fn enter_game_over(&mut self) -> Result<()> {
        for tick in 0..900 {
            self.step((tick % 12 == 0).then_some(KeyCode::Enter))?;
            let session = self.app.world().resource::<new_game::Session>();
            assert_eq!(session.field.events.tick(), self.field_tick);
            assert_eq!(
                serde_json::to_value(&session.field.events.world.party)?,
                self.party
            );
            assert!(self.request.is_pending());
            if self.app.world().contains_resource::<game_over::Active>() {
                break;
            }
        }
        assert!(
            self.app.world().contains_resource::<game_over::Active>(),
            "fatal transfer did not finish"
        );
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
            self.audio.sample_battle(1)?,
            (true, Some(96)),
            "fatal transfer stopped defeat music"
        );
        let played = self
            .app
            .world()
            .resource::<new_game::Session>()
            .field
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
                .field
                .events
                .tick(),
            self.field_tick
        );
        assert_eq!(
            self.app
                .world()
                .resource::<new_game::Session>()
                .field
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
            self.audio.sample_battle(1)?.1,
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
        while self.app.world().resource::<saves::title::LoadMenu>().0.busy {
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
        self.step(Some(key))?;
        self.step(None)
    }
    fn assert_title(&mut self) -> Result<()> {
        let world = self.app.world_mut();
        assert!(!world.contains_resource::<game_over::Active>());
        assert!(!world.contains_resource::<saves::title::LoadMenu>());
        assert!(!world.contains_resource::<crate::battle_audio::Playback>());
        assert!(!world.contains_resource::<new_game::Session>());
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
        assert_eq!(self.audio.sample_battle(1)?, (false, None));
        Ok(())
    }
}

/// Use verified HUD/menu data and actual scene owners, but only CPU placeholder
/// images. Their presence tests resource lifetime; it cannot prove GPU readiness.
fn prepared_scene(world: &mut World, package: Package) -> Result<Scene> {
    let Package {
        assets,
        prepared,
        audio,
        party,
        menus,
        data,
        catalogue,
        libc_seed,
        entry_seed,
    } = package;
    let enemies = enemy_hud(&assets, &prepared)?;
    let action_names = action_names(&prepared, &catalogue)?;
    let server = world.resource::<AssetServer>().clone();
    let (mut hud, mut art) = world.resource_scope(
        |world, mut surfaces: Mut<Assets<crate::field_ui::Surface>>| -> Result<_> {
            let hud = crate::field_ui::BattleHud::load(
                |path| Ok(assets.files.read(path)?.to_vec()),
                &enemies,
                &server,
                &mut surfaces,
                &mut world.resource_mut(),
            )?;
            let art = crate::field_ui::GameOverArt::load(
                assets.game_over.as_ref().context("fatal art missing")?,
                &server,
                &mut surfaces,
            )?;
            Ok((hud, art))
        },
    )?;
    world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        art.prepare(&mut world.commands(), &mut meshes);
    });
    world.flush();
    for handle in art.images() {
        world
            .resource_mut::<Assets<Image>>()
            .insert(handle.id(), Image::default())?;
    }
    let view = crate::battle_view::View::load(
        assets.stage.clone(),
        vec![],
        Handle::default(),
        vec![],
        vec![],
        &assets.ui,
        &server,
    )?;
    let settings = party.settings.preferences.clone();
    hud.settings(settings.clone());
    let candidate =
        results::Candidate::new(prepared.results, party, libc_seed, data, menus, catalogue)?;
    let playback = crate::battle_audio::Playback::begin(
        &mut world.resource_mut(),
        audio.clone(),
        crate::battle_audio::Settings {
            music: settings.volumes.music,
            effects: settings.volumes.effects,
            battle_effects: settings.volumes.battle_effects,
            voice: settings.volumes.battle_voice,
            stereo: settings.stereo,
        },
    )?;
    playback.music(Some(i16::try_from(prepared.music)?), 0)?;
    world.insert_resource(playback);
    Ok(Scene {
        view,
        hud,
        game_over: Some(art),
        audio,
        settings,
        core: Battle::new(prepared.core),
        lifecycle: prepared.lifecycle.start()?,
        candidate: Some(candidate),
        characters: prepared.characters,
        actors: prepared.actors,
        action_names,
        music: prepared.music,
        frame: None,
        camera: None,
        target: RenderTarget::default(),
        active: true,
        presenting: true,
        gpu_ready: true,
        dispatch: crate::battle_entry::Dispatch::Camera,
        transition: prepared.entry_transition,
        switched: true,
        failure: None,
        completed: None,
        requested_music: None,
        generation: 1,
        entry_seed,
        preparing_since: Instant::now(),
        diagnostics: resonance_content::diagnostics::Diagnostics::new(true),
    })
}

#[test]
#[ignore = "requires current cooked opening/title assets and checkpoint; no GPU or audio device"]
fn fatal_completion_keeps_caller_and_music_then_loads_a_new_field_once() -> Result<()> {
    let mut fixture = Fixture::new("load")?;
    {
        let mut owner = fixture.app.world_mut().resource_mut::<Owner>();
        assert!(owner.capture_ready());
        let Phase::Scene(scene) = &mut owner.phase else {
            panic!("prepared battle scene");
        };
        scene.active = false;
        scene.dispatch = crate::battle_entry::Dispatch::Initialize;
        scene.gpu_ready = false;
        assert!(
            owner.presenting(),
            "entry presentation may precede GPU readiness"
        );
        assert!(
            !owner.capture_ready(),
            "readback must wait for stable GPU targets"
        );
        let Phase::Scene(scene) = &mut owner.phase else {
            unreachable!();
        };
        scene.active = true;
        scene.dispatch = crate::battle_entry::Dispatch::Camera;
        scene.gpu_ready = true;
    }
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
    assert!(
        !world
            .resource::<loading::Resident>()
            .battle
            .load(Ordering::Acquire)
    );
    let session = world.resource::<new_game::Session>();
    assert_eq!(session.assets.map_id, fixture.saved.map_id);
    // The captured checkpoint has no travel history. Normal field entry
    // records Iselia (location 2), including on a restored field.
    // Keep the entire party comparison so fatal-candidate changes cannot leak.
    let mut expected_party = fixture.saved.progress.party.clone();
    expected_party.travel.current_location = Some(2);
    expected_party.travel.visited_locations.insert(2);
    ensure!(
        serde_json::to_value(&session.field.events.world.party)?
            == serde_json::to_value(Some(&expected_party))?,
        "loaded party differs from checkpoint plus Iselia travel entry"
    );
    let tick = session.field.events.tick();
    for _ in 0..5 {
        fixture.step(None)?;
    }
    assert_eq!(
        fixture
            .app
            .world()
            .resource::<new_game::Session>()
            .field
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
    Ok(())
}

#[test]
#[ignore = "requires current cooked opening/title assets and checkpoint; no GPU or audio device"]
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
#[ignore = "requires current cooked opening/title assets and checkpoint; no GPU or audio device"]
fn fatal_quit_cancels_the_caller_and_restarts_title_once() -> Result<()> {
    let mut fixture = Fixture::new("title")?;
    fixture.enter_game_over()?;
    fixture.choose(true)?;
    fixture.assert_title()
}

#[test]
#[ignore = "requires current cooked opening/title assets and checkpoint; no GPU or audio device"]
fn early_audio_failure_returns_mixer_without_a_constructed_battle_scene() -> Result<()> {
    let mut fixture = Fixture::new("early-audio")?;
    let owner = fixture.app.world_mut().remove_resource::<Owner>().unwrap();
    let Phase::Scene(scene) = &owner.phase else {
        unreachable!()
    };
    let assets = scene.audio.clone();
    let preferences = scene.settings.clone();
    retire(fixture.app.world_mut(), owner, true);
    let field = fixture.audio.sample_battle(1)?;
    assert!(!field.0);
    assert!(field.1.is_some(), "fixture must retain a field song");
    fixture
        .app
        .world_mut()
        .resource_mut::<crate::field_audio::Control>()
        .prepare_battle_entry()?;
    assert_eq!(fixture.audio.sample_battle(1)?, field);
    // Cancellation before the bank is ready returns the retained player without
    // constructing Playback or changing field clocks/assets.
    retire(
        fixture.app.world_mut(),
        Owner {
            request: fixture.request.clone(),
            phase: Phase::Failed,
            input_tick: 0,
            music_paused: true,
            entry: None,
        },
        true,
    );
    assert_eq!(fixture.audio.sample_battle(1)?, field);
    fixture
        .app
        .world_mut()
        .resource_mut::<crate::field_audio::Control>()
        .prepare_battle_entry()?;
    assert_eq!(fixture.audio.sample_battle(1)?, field);
    let before = fixture
        .app
        .world()
        .resource::<new_game::Session>()
        .field
        .events
        .tick();
    begin_audio(
        fixture.app.world_mut(),
        loading::BattleAudio {
            assets: assets.clone(),
            settings: crate::battle_audio::Settings {
                music: preferences.volumes.music,
                effects: preferences.volumes.effects,
                battle_effects: preferences.volumes.battle_effects,
                voice: preferences.volumes.battle_voice,
                stereo: preferences.stereo,
            },
            track: 85,
        },
    )?;
    assert_eq!(fixture.audio.sample_battle(1)?, (true, Some(85)));
    // Preparation can fail after audio ownership, before a Scene exists.
    // The ordinary owner cleanup must still consume exactly that Playback.
    retire(
        fixture.app.world_mut(),
        Owner {
            request: fixture.request.clone(),
            phase: Phase::Failed,
            input_tick: 0,
            music_paused: false,
            entry: None,
        },
        false,
    );
    assert!(
        !fixture
            .app
            .world()
            .contains_resource::<crate::battle_audio::Playback>()
    );
    assert_eq!(fixture.audio.sample_battle(1)?, (false, None));
    // Begin has already entered the queue when converting a malformed music
    // request fails. The installed owner must still be available to retire it.
    assert!(
        begin_audio(
            fixture.app.world_mut(),
            loading::BattleAudio {
                assets,
                settings: crate::battle_audio::Settings {
                    music: preferences.volumes.music,
                    effects: preferences.volumes.effects,
                    battle_effects: preferences.volumes.battle_effects,
                    voice: preferences.volumes.battle_voice,
                    stereo: preferences.stereo,
                },
                track: u16::MAX,
            }
        )
        .is_err()
    );
    assert!(
        fixture
            .app
            .world()
            .contains_resource::<crate::battle_audio::Playback>()
    );
    retire(
        fixture.app.world_mut(),
        Owner {
            request: fixture.request.clone(),
            phase: Phase::Failed,
            input_tick: 0,
            music_paused: false,
            entry: None,
        },
        false,
    );
    assert_eq!(fixture.audio.sample_battle(1)?, (false, None));
    // Fatal/cancelled preparation has the same non-resuming semantics as End:
    // a later field cannot inherit an indefinitely paused retained track.
    fixture
        .app
        .world_mut()
        .resource_mut::<crate::field_audio::Control>()
        .prepare_battle_entry()?;
    retire(
        fixture.app.world_mut(),
        Owner {
            request: fixture.request.clone(),
            phase: Phase::Failed,
            input_tick: 0,
            music_paused: true,
            entry: None,
        },
        false,
    );
    assert_eq!(fixture.audio.sample_battle(1)?, (false, None));
    let session = fixture.app.world().resource::<new_game::Session>();
    assert_eq!(session.field.events.tick(), before);
    assert_eq!(
        serde_json::to_value(&session.field.events.world.party)?,
        fixture.party
    );
    assert!(fixture.request.is_pending());
    Ok(())
}
