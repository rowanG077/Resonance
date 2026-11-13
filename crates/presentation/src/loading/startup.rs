//! One admission gate over shared CPU inputs and concurrent GPU preparations.
use super::*;
use resonance_content::{library::Library, preparation};

pub(crate) struct Common {
    pub files: Arc<Files>,
    pub data: Arc<resonance_content::session::SessionData>,
    pub text: Arc<resonance_content::session::GameText>,
    pub skits: Arc<resonance_content::skit::SkitCatalog>,
    pub skit_voices: Arc<crate::field_audio::SkitVoices>,
    pub skit_programs: Arc<std::collections::BTreeMap<u16, resonance_game::skit::Prepared>>,
    pub services: Arc<resonance_game::authored::FieldServices>,
    pub fields: std::collections::BTreeSet<u32>,
    pub battle: Option<Arc<resonance_game::battle::encounter::Common>>,
    pub battle_ui: resonance_content::battle_ui::Art,
    pub field: Option<resonance_content::field::document::Common>,
    pub cutins: Vec<resonance_content::font::UiTexture>,
    pub world: Option<crate::overworld::Package>,
    _sounds: Vec<Arc<resonance_audio::effect::Prepared>>,
    _voices: Vec<Arc<crate::field_audio::Clip>>,
    _music: Vec<Arc<crate::audio::Recording>>,
}

impl Common {
    fn capture(
        library: Library,
        diagnostics: resonance_content::diagnostics::Diagnostics,
        stop: Arc<AtomicBool>,
    ) -> Result<(Arc<Self>, crate::field_audio::Cache)> {
        let mut snapshot = library.snapshot();
        let cancelled = || stop.load(Ordering::Relaxed);
        let has_world = diagnostics
            .preload("startup world", &cancelled, || {
                resonance_content::overworld::document::present(&mut snapshot)
            })?
            .unwrap_or(false);
        let capture = snapshot.concurrent();
        std::thread::scope(|scope| -> Result<_> {
            let mut snapshot = capture.view();
            let world_diagnostics = diagnostics.clone();
            let world = scope.spawn(move || -> Result<_> {
                profile_span!("startup world capture");
                if !has_world {
                    return Ok(None);
                }
                world_diagnostics.preload("startup world", &cancelled, || {
                    crate::overworld::Inputs::capture(
                        &mut snapshot,
                        &mut crate::field_audio::Cache::default(),
                        &cancelled,
                        &world_diagnostics,
                    )
                })
            });
            let mut snapshot = capture.view();
            let sources = preparation::gameplay(&mut snapshot, &diagnostics, &cancelled)?;
            preparation::ui(&mut snapshot, &diagnostics, &cancelled)?;
            let mut battle_view = capture.view();
            let mut field_view = capture.view();
            let battle_diagnostics = diagnostics.clone();
            let battle = scope.spawn(move || -> Result<_> {
                profile_span!("startup battle capture");
                battle_diagnostics.preload("startup battle", &cancelled, || {
                    resonance_game::battle::encounter::Common::capture(
                        &mut battle_view,
                        &battle_diagnostics,
                        &cancelled,
                    )
                    .map(Arc::new)
                })
            });
            let field_diagnostics = diagnostics.clone();
            let field = scope.spawn(move || {
                profile_span!("startup field capture");
                field_diagnostics.preload("startup field", &cancelled, || {
                    resonance_content::field::document::Common::capture(
                        &mut field_view,
                        &field_diagnostics,
                        &cancelled,
                    )
                })
            });
            let ui = Self::capture_ui(capture.view(), &diagnostics, &cancelled);
            let battle = battle
                .join()
                .map_err(|_| anyhow::anyhow!("startup battle preparation panicked"));
            let field = field
                .join()
                .map_err(|_| anyhow::anyhow!("startup field preparation panicked"));
            let (battle, field, (skits, cutins, programs)) = (battle??, field??, ui?);
            // Shared HUD artwork is required independently of speculative
            // battle rules/models. A failed preload stays a cold encounter.
            let battle_ui = match &battle {
                Some(battle) => battle.ui.clone(),
                None => resonance_content::battle_ui::document::capture(
                    &mut snapshot,
                    &diagnostics,
                    &cancelled,
                )?,
            };
            profile_span!(startup_profile = "startup audio warm");
            let skit_voices = Arc::new(crate::field_audio::Assets::prepare_skits(
                &skits,
                &mut snapshot,
                diagnostics.clone(),
                &cancelled,
            )?);
            let mut audio = crate::field_audio::Cache::default();
            let (sounds, voices) = match &battle {
                Some(battle) => (
                    audio.warm_effects(battle, &mut snapshot, &cancelled)?,
                    audio.warm_voices(battle, &mut snapshot, &cancelled)?,
                ),
                None => (Vec::new(), Vec::new()),
            };
            let music = match &battle {
                Some(battle) => audio.warm_music(battle, &mut snapshot, &cancelled)?,
                None => Vec::new(),
            };
            profile_span!(end startup_profile);
            profile_span!(startup_profile = "startup service scripts");
            let scripts = crate::new_game::FieldPackage::prepare_from_sources(
                0,
                &sources,
                &mut Cache::default(),
                |path| {
                    anyhow::ensure!(!cancelled(), "shared field preparation cancelled");
                    snapshot.read(path, preparation::ASSET_LIMIT)
                },
            )?;
            anyhow::ensure!(
                scripts.authored.is_none(),
                "common services cannot own field event state"
            );
            profile_span!(end startup_profile);
            profile_span!(startup_profile = "startup session and skit programs");
            let fields = crate::new_game::capture_fields(&mut snapshot, has_world)?;
            let world = world
                .join()
                .map_err(|_| anyhow::anyhow!("startup world preparation panicked"))??;
            anyhow::ensure!(!cancelled(), "startup preparation cancelled");
            drop(snapshot);
            let files = Arc::new(Files::from_snapshot(
                capture.finish()?,
                cancelled,
                diagnostics.clone(),
            )?);
            let world = match world {
                Some(inputs) => diagnostics.preload("startup world", &cancelled, || {
                    inputs.prepare(files.clone(), skits.clone(), programs.clone(), &cancelled)
                })?,
                None => None,
            };
            // Share the assembled session resources with fields too.
            let (data, text, skit_programs) = match &world {
                Some(world) => (
                    world
                        .world
                        .resources
                        .session_data
                        .as_ref()
                        .context("startup world data missing")?
                        .clone(),
                    world.world.resources.text.clone(),
                    world.world.skits.clone(),
                ),
                None => {
                    let data = Arc::new(resonance_content::session::SessionData::load(&files)?);
                    let text = Arc::new(resonance_content::session::GameText::load(&files)?);
                    let skit_programs = Arc::new(resonance_game::skit::Prepared::load(
                        skits.clone(),
                        &files,
                        &resonance_events::ResourceLibrary {
                            text: text.clone(),
                            session_data: Some(data.clone()),
                            ..Default::default()
                        },
                        &programs,
                    )?);
                    (data, text, skit_programs)
                }
            };
            let world = world.map(|mut world| {
                Arc::make_mut(&mut world.audio).use_skits(&skit_voices);
                world
            });
            let common = Self {
                files,
                data,
                text,
                skits,
                skit_voices,
                skit_programs,
                services: scripts.services,
                fields,
                battle,
                battle_ui,
                field,
                cutins,
                world,
                _sounds: sounds,
                _voices: voices,
                _music: music,
            };
            profile_span!(end startup_profile);
            Ok((Arc::new(common), audio))
        })
    }
    fn capture_ui(
        mut snapshot: resonance_content::library::Snapshot,
        diagnostics: &resonance_content::diagnostics::Diagnostics,
        cancelled: &(impl Fn() -> bool + Sync),
    ) -> Result<(
        Arc<resonance_content::skit::SkitCatalog>,
        Vec<resonance_content::font::UiTexture>,
        resonance_game::screen_resources::Programs,
    )> {
        profile_span!("startup skit and cutin capture");
        let skits = Arc::new(resonance_content::skit::document::capture(
            &mut snapshot,
            cancelled,
        )?);
        let programs = resonance_game::screen_resources::capture(
            &mut snapshot,
            skits.resources.values().map(|skit| skit.script.as_str()),
            diagnostics,
            cancelled,
        )?;
        let cutins = capture_cutins(&mut snapshot, diagnostics, cancelled)?;
        Ok((skits, cutins, programs))
    }
}

// Warmup failures follow the same policy as optional encounter artwork. Keep
// cancellation outside recovery so it cannot publish a partially warmed set.
fn capture_cutins(
    snapshot: &mut resonance_content::library::Snapshot,
    diagnostics: &resonance_content::diagnostics::Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<resonance_content::font::UiTexture>> {
    use resonance_content::battle_scene::document::{Definition, Kind};
    let mut cutins = Vec::new();
    for kind in [Kind::Technique, Kind::Skill] {
        let ids = diagnostics.preload(kind.mapping(), cancelled, || {
            if snapshot
                .read_optional(kind.mapping(), preparation::JSON_LIMIT)?
                .is_none()
            {
                return Ok(Vec::new());
            }
            Ok(snapshot
                .mappings(kind.mapping())?
                .iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>())
        })?;
        let Some(ids) = ids else {
            continue;
        };
        for id in ids {
            let image = diagnostics.preload(
                &format!("common battle cut-in {}:{id}", kind.mapping()),
                cancelled,
                || -> Result<_> {
                    let path = snapshot.resolve(kind.mapping(), id, Some("scene.json"))?;
                    let scene: Definition = snapshot.json(&path, preparation::JSON_LIMIT)?;
                    let Some(cutin) = scene.cutin else {
                        return Ok(None);
                    };
                    anyhow::ensure!(cutin.images.len() == 1, "skill cut-in requires one image");
                    let mut image = cutin.images.into_iter().next().unwrap();
                    [image.width, image.height] = resonance_asset_writer::texture::dimensions(
                        &snapshot.read(&image.path, preparation::ASSET_LIMIT)?,
                    )?;
                    image.validate()?;
                    Ok(Some(image))
                },
            )?;
            if let Some(image) = image.flatten() {
                cutins.push(image);
            }
        }
    }
    Ok(cutins)
}

#[derive(Resource)]
enum Startup {
    Cpu(Task<(Arc<Common>, crate::field_audio::Cache)>),
    Gpu {
        ui: Option<crate::field_ui::resident::Warm>,
        field: Option<super::field::Warm>,
        battle: Option<crate::battle_view::startup::Warm>,
        worlds: Option<crate::overworld::warm::Warm>,
    },
    Ready,
    Failed,
}

impl Startup {
    fn ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Status<'w> {
    startup: Option<Res<'w, Startup>>,
}
impl Status<'_> {
    pub(crate) fn ready(&self) -> bool {
        self.startup.as_ref().is_none_or(|state| state.ready())
    }
}

pub(crate) fn install(app: &mut App) -> Result<()> {
    let resident = app.world().resource::<Resident>();
    let library = resident.library(&app.world().resource::<crate::RunOptions>().assets)?;
    let diagnostics = resident.diagnostics.clone();
    app.insert_resource(Startup::Cpu(Task::spawn(
        "common startup resources",
        move |stop| Common::capture(library, diagnostics, stop),
    )?));
    super::party::install(app);
    crate::battle_view::startup::install(app);
    app.add_systems(
        Update,
        update
            .before(crate::new_game::enter)
            .before(crate::timing::prepare),
    );
    Ok(())
}

pub(crate) fn ready(world: &World) -> bool {
    world.get_resource::<Startup>().is_none_or(Startup::ready)
}

#[cfg(all(test, feature = "preparation-trace"))]
pub(crate) fn pipelines_building(world: &World) -> bool {
    matches!(world.get_resource::<Startup>(), Some(Startup::Gpu { battle: Some(battle), .. }) if battle.pipelines_building())
}

fn poll_optional<T>(
    world: &mut World,
    stage: &mut Option<T>,
    scope: &str,
    poll: impl FnOnce(&mut T, &mut World) -> Result<bool>,
    abort: impl FnOnce(&mut World, &mut T),
) -> Result<()> {
    let Some(warm) = stage.as_mut() else {
        return Ok(());
    };
    match poll(warm, world) {
        Ok(false) => return Ok(()),
        Ok(true) => {}
        Err(error) => {
            abort(world, warm);
            crate::diagnostics::policy(world).report(scope, error)?;
        }
    }
    *stage = None;
    Ok(())
}

fn update(world: &mut World) {
    let state = world.remove_resource::<Startup>().unwrap();
    let result = (|| -> Result<Startup> {
        match state {
            Startup::Cpu(task) => match task.poll()? {
                None => Ok(Startup::Cpu(task)),
                Some(result) => {
                    let (common, audio) = result?;
                    world
                        .resource::<Resident>()
                        .cache
                        .lock()
                        .unwrap()
                        .audio
                        .adopt_prepared(audio);
                    world
                        .resource::<Resident>()
                        .common
                        .set(common.clone())
                        .map_err(|_| anyhow::anyhow!("common resources already installed"))?;
                    super::graphics::begin(world, &common)?;
                    let ui = Some(crate::field_ui::resident::Warm::new(world, &common)?);
                    let battle = crate::diagnostics::policy(world)
                        .attempt(
                            "startup battle graphics",
                            crate::battle_view::startup::Warm::new(world),
                        )?
                        .flatten();
                    let field = match super::field::Warm::new(world, &common) {
                        Ok(warm) => warm,
                        Err(error) => {
                            super::field::abort(world);
                            crate::diagnostics::policy(world)
                                .report("startup field graphics", error)?;
                            None
                        }
                    };
                    let worlds = match crate::overworld::warm::Warm::new(world) {
                        Ok(warm) => warm,
                        Err(error) => {
                            crate::diagnostics::policy(world).report("startup world", error)?;
                            None
                        }
                    };
                    Ok(Startup::Gpu {
                        ui,
                        field,
                        battle,
                        worlds,
                    })
                }
            },
            Startup::Gpu {
                mut ui,
                mut field,
                mut battle,
                mut worlds,
            } => {
                let graphics_ready = super::graphics::update(world)?;
                poll_optional(
                    world,
                    &mut field,
                    "startup field graphics",
                    super::field::Warm::update,
                    |world, _| super::field::abort(world),
                )?;
                if ui
                    .as_mut()
                    .map(|ui| ui.update(world))
                    .transpose()?
                    .unwrap_or(true)
                {
                    ui = None;
                }
                poll_optional(
                    world,
                    &mut battle,
                    "startup battle graphics",
                    |battle, world| {
                        let ready = super::graphics::prepare_battle(world, battle)?;
                        if ready {
                            battle.retire(world);
                        }
                        Ok(ready)
                    },
                    |world, battle| battle.abort(world),
                )?;
                poll_optional(
                    world,
                    &mut worlds,
                    "startup world",
                    crate::overworld::warm::Warm::update,
                    |world, _| crate::overworld::warm::abort(world),
                )?;
                if ui.is_none()
                    && field.is_none()
                    && graphics_ready
                    && battle.is_none()
                    && worlds.is_none()
                {
                    info!(
                        "Common UI, field effects, world and GPU residency ready; gameplay admission released"
                    );
                    Ok(Startup::Ready)
                } else {
                    Ok(Startup::Gpu {
                        ui,
                        field,
                        battle,
                        worlds,
                    })
                }
            }
            state => Ok(state),
        }
    })();
    let state = match result {
        Ok(state) => state,
        Err(error) => {
            crate::diagnostics::fatal(world, "startup residency", error);
            Startup::Failed
        }
    };
    world.insert_resource(state);
}

#[cfg(test)]
pub(crate) fn check_audio_reuse(world: &World) -> Result<()> {
    let resident = world.resource::<Resident>();
    let common = resident
        .common
        .get()
        .context("startup common inputs missing")?;
    anyhow::ensure!(
        !common._sounds.is_empty() && !common._voices.is_empty(),
        "startup player audio is empty"
    );
    let (models, programs, validations) = common
        .battle
        .as_ref()
        .context("startup battle inputs missing")?
        .factory_reuse();
    anyhow::ensure!(
        models > 0 && programs > 0 && validations > 0,
        "battle did not reuse prepared common model/program validation"
    );
    let cache = resident.cache.lock().unwrap();
    anyhow::ensure!(
        cache.audio.effect_cache_hits() > 0 && cache.audio.voice_cache_hits() > 0,
        "battle did not reuse startup audio"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::{diagnostics::Diagnostics, library};
    use serde_json::json;

    #[test]
    fn startup_holds_black_and_playback_until_all_caches_are_ready() -> Result<()> {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<Resident>()
            .init_resource::<Assets<crate::materials::TitleOutput>>()
            .init_resource::<crate::timing::Ready>()
            .init_resource::<crate::RenderReady>()
            .init_resource::<crate::FieldAssets>()
            .init_resource::<crate::boot::Playback>()
            .insert_resource(crate::Art {
                manifest: Vec::new(),
                images: Vec::new(),
            })
            .add_systems(
                Update,
                (crate::timing::prepare, super::super::black_hold).chain(),
            );
        app.world_mut().resource_mut::<crate::FieldAssets>().ready = true;
        app.world()
            .resource::<crate::RenderReady>()
            .0
            .store(true, Ordering::Release);
        let output = app
            .world_mut()
            .resource_mut::<Assets<crate::materials::TitleOutput>>()
            .add(crate::materials::TitleOutput {
                overlay: None,
                source: Handle::default(),
                brightness: Vec4::new(1., 1., 0., 0.),
            });
        for state in [
            Startup::Cpu(Task::spawn("startup hold test", |_| {
                anyhow::bail!("unused result")
            })?),
            Startup::Gpu {
                ui: None,
                field: None,
                battle: None,
                worlds: None,
            },
            Startup::Failed,
            Startup::Ready,
        ] {
            let ready = state.ready();
            app.insert_resource(state);
            app.update();
            assert_eq!(app.world().resource::<crate::timing::Ready>().0, ready);
            assert_eq!(
                app.world()
                    .resource::<Assets<crate::materials::TitleOutput>>()
                    .get(&output)
                    .unwrap()
                    .brightness
                    .z,
                f32::from(!ready)
            );
        }
        Ok(())
    }

    // One broken, unused startup input per shared package. The package is
    // omitted, healthy fields still load, and strict diagnostics still fail.
    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; no window or audio device"]
    fn unused_broken_preloads_keep_healthy_fields() -> Result<()> {
        use resonance_content::battle_scene::document::TECHNIQUES;
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
        for (broken, source, value) in [
            (
                "world/movement.json",
                "world/movement/collision/radius.json",
                json!(0),
            ),
            (
                "animations/bindings.json",
                "animations/bindings/banks/11.json",
                json!(null),
            ),
            (TECHNIQUES, "battle/scenes/mappings/201.json", json!(null)),
        ] {
            let overlay = tempfile::tempdir()?;
            // Keep the namespace valid so the preload, rather than library
            // assembly, rejects the selected document's contents.
            let path = overlay.path().join(source);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, serde_json::to_vec(&value)?)?;
            let library = Library::open_paths(Path::new(&root), &[overlay.path().to_owned()])?;
            let diagnostics = Diagnostics::default();
            let (common, _) = Common::capture(
                library.clone(),
                diagnostics.clone(),
                Arc::new(AtomicBool::new(false)),
            )?;
            assert!(diagnostics.has_errors());
            let field_bindings = broken == "animations/bindings.json";
            assert_eq!(
                (
                    common.world.is_none(),
                    common.field.is_none(),
                    common.battle.is_none()
                ),
                (
                    broken == "world/movement.json",
                    field_bindings,
                    broken == TECHNIQUES
                )
            );
            let mut cache = Cache::default();
            cache.common.set(common).ok().unwrap();
            let healthy = if field_bindings { 5 } else { 332 };
            let field = crate::new_game::FieldPackage::capture(
                &library,
                healthy,
                &mut cache,
                || false,
                diagnostics,
            )?;
            assert_eq!(field.assets.map_id, healthy);
            assert!(
                Common::capture(
                    library,
                    Diagnostics::new(true),
                    Arc::new(AtomicBool::new(false)),
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_ASSETS; no window or audio device"]
    fn battle_audio_preloads_preserve_the_diagnostic_policy() -> Result<()> {
        use resonance_game::battle::encounter::Common as BattleCommon;
        let root = std::env::var_os("RESONANCE_TEST_ASSETS").context("assets required")?;
        let library = Library::open(Path::new(&root), &[])?;
        let common =
            BattleCommon::capture(&mut library.snapshot(), &Diagnostics::new(true), &|| false)?;
        let sound = common
            .effect_sounds()
            .into_iter()
            .find_map(|id| {
                i16::try_from(id)
                    .ok()
                    .and_then(|id| common.audio.resolved.effects.get(&id))
            })
            .context("common battle sound missing")?;
        let voice = common
            .voice_streams()
            .into_iter()
            .filter_map(|id| common.audio.resolved.voices.get(&u32::from(id)))
            .find_map(|voice| voice.source.path())
            .context("common battle speech missing")?;
        let overlay = tempfile::tempdir()?;
        for path in [sound.as_str(), voice] {
            let path = overlay.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, b"invalid audio preload")?;
        }
        let library = Library::open_paths(Path::new(&root), &[overlay.path().to_owned()])?;
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut snapshot = library.snapshot();
            let common = BattleCommon::capture(&mut snapshot, &diagnostics, &|| false)?;
            let mut audio = crate::field_audio::Cache::default();
            assert_eq!(
                audio
                    .warm_effects(&common, &mut snapshot, &|| false)
                    .is_err(),
                paranoid
            );
            assert_eq!(
                audio
                    .warm_voices(&common, &mut snapshot, &|| false)
                    .is_err(),
                paranoid
            );
            for scope in ["common battle sound ", "common battle speech "] {
                assert!(
                    diagnostics
                        .entries()
                        .iter()
                        .any(|entry| entry.scope.starts_with(scope)),
                    "missing preload diagnostic: {scope}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn cutin_warmup_skips_bad_images_unless_strict() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("content.json"),
            serde_json::to_vec(&json!({"version": library::VERSION, "game": library::GAME}))?,
        )?;
        let scenes = root.path().join("battle/scenes");
        std::fs::create_dir_all(&scenes)?;
        let names = ["missing", "broken", "healthy"];
        let mappings: std::collections::BTreeMap<_, _> = names.iter().enumerate().collect();
        std::fs::write(scenes.join("mappings.json"), serde_json::to_vec(&mappings)?)?;
        for name in names {
            std::fs::create_dir(scenes.join(name))?;
            std::fs::write(
                scenes.join(name).join("scene.json"),
                serde_json::to_vec(&json!({
                    "effects": "unused.json", "duration": 1,
                    "cutin": {"images": [{"image":format!("{name}.ktx2"),"size":[1,1]}]}
                }))?,
            )?;
        }
        std::fs::write(root.path().join("broken.ktx2"), b"invalid image")?;
        std::fs::write(
            root.path().join("healthy.ktx2"),
            resonance_asset_writer::ktx2::encode_levels(1, 1, &[&[255; 4]])?,
        )?;
        let library = Library::open(root.path(), &[])?;
        let diagnostics = Diagnostics::default();
        let cutins = capture_cutins(&mut library.snapshot(), &diagnostics, &|| false)?;
        assert_eq!(cutins.len(), 1);
        assert_eq!(
            (&*cutins[0].path, cutins[0].width, cutins[0].height),
            ("healthy.ktx2", 1, 1)
        );
        assert_eq!(diagnostics.entries().len(), 2);
        assert!(
            capture_cutins(&mut library.snapshot(), &Diagnostics::new(true), &|| false).is_err()
        );
        Ok(())
    }
}
