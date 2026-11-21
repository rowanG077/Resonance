//! Helpers shared by presentation tests.
use bevy::{
    prelude::{App, Handle, Image, On, ResMut, Resource},
    shader::{ShaderCacheError, ShaderCacheSource},
};
use resonance_battle::{Actor, PreparedBattle, Side};

/// Expected world position of an inertial cloth joint under a turning animated pin.
pub(crate) fn inertial_joint(
    position: bevy::math::Vec3,
    pin: impl Fn(f32) -> bevy::math::Vec3,
    t: f32,
) -> bevy::math::Vec3 {
    position + (pin(t) - pin(0.).lerp(pin(1.), t))
}

/// CPU cost and allocator isolation for cooked field and battle predictions.
pub(crate) struct PredictionMeasurements {
    storage: resonance_game::field::PredictionBuffer,
    milliseconds: Vec<f64>,
    peak_bytes: usize,
}

impl PredictionMeasurements {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            storage: resonance_game::field::PredictionBuffer::new()?,
            milliseconds: Vec::new(),
            peak_bytes: 0,
        })
    }

    pub fn measure<T>(
        &mut self,
        run: impl FnOnce(&mut resonance_game::field::PredictionBuffer) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let calls = resonance_simulation_memory::system_calls();
        let start = std::time::Instant::now();
        let value = run(&mut self.storage)?;
        let ms = start.elapsed().as_secs_f64() * 1000.;
        assert_eq!(resonance_simulation_memory::system_calls(), calls);
        self.milliseconds.push(ms);
        self.peak_bytes = self
            .peak_bytes
            .max(self.storage.used_bytes().into_iter().max().unwrap());
        Ok(value)
    }

    pub fn report(mut self, scene: &str) {
        self.milliseconds.sort_by(f64::total_cmp);
        let n = self.milliseconds.len();
        eprintln!(
            "{scene}, {n} predictions: median {:.3} ms, p95 {:.3} ms, max {:.3} ms (CPU only); peak arena {:.2} MiB; zero system allocator calls",
            self.milliseconds[n / 2],
            self.milliseconds[n * 95 / 100],
            self.milliseconds[n - 1],
            self.peak_bytes as f64 / (1024. * 1024.)
        );
    }
}

/// Independent editable copies of a complete current library. Fixtures use the
/// same content reader as production and never translate an old cooked tree.
fn named_fixture() -> anyhow::Result<tempfile::TempDir> {
    use anyhow::{Context as ContextExt, ensure};
    let source = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map(std::path::PathBuf::from)
        .context("set RESONANCE_TEST_ASSETS to a complete current named library")?;
    resonance_content::library::Library::open(&source, &[])?;
    let work = tempfile::tempdir()?;
    let mut pending = vec![std::path::PathBuf::new()];
    while let Some(relative) = pending.pop() {
        std::fs::create_dir_all(work.path().join(&relative))?;
        for entry in std::fs::read_dir(source.join(&relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                ensure!(
                    kind.is_file(),
                    "fixture contains a non-regular file: {}",
                    path.display()
                );
                // Independent copies allow in-place edits without modifying base
                // content. Do not use hard links for these authoring fixtures.
                std::fs::copy(entry.path(), work.path().join(path))?;
            }
        }
    }
    Ok(work)
}

pub(crate) fn named_opening_fixture() -> anyhow::Result<tempfile::TempDir> {
    named_fixture()
}

pub(crate) fn named_world_fixture() -> anyhow::Result<tempfile::TempDir> {
    named_fixture()
}

pub(crate) fn await_preparation<T: Send + 'static>(
    pending: &crate::loading::Task<T>,
) -> anyhow::Result<T> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        if let Some(result) = pending.poll()? {
            return result;
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "scene preparation timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub(crate) fn run_options(assets: impl Into<std::path::PathBuf>) -> crate::RunOptions {
    crate::RunOptions {
        speculative: false,
        assets: assets.into(),
        mods: Vec::new(),
        script_root: None,
        saves: Default::default(),
        presentation_start: None,
        replay: None,
        capture_at: None,
        capture: None,
        reveal: false,
        silent: true,
        paranoid: true,
        skip_intro: true,
        skip_battles: false,
        allow_incomplete_scripts: false,
        record_playthrough: None,
        record_title_ticks: 0,
    }
}

/// A save beside Iselia's save point, after the classroom sequence.
/// Host tests restore it directly; the game integration test owns story playback.
pub(crate) fn field_checkpoint(
    files: &resonance_content::prepared::Files,
) -> anyhow::Result<resonance_game::field::FieldCheckpoint> {
    use resonance_events::{
        SavedProgress,
        camera::{CameraRig, EntryCamera},
        party::Party,
    };
    let data = resonance_content::session::SessionData::load(files)?;
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![1, 2, 3];
    let leader = i32::from(party.field_leader);
    let mut progress = SavedProgress::new(party);
    // The field content stores its story counter in global word 16.
    progress.script_globals[16] = 2500;
    progress.event_flags.extend([31, 520]);
    let mut camera = CameraRig::default();
    *camera.current_mut() = EntryCamera::following(leader).camera;
    Ok(resonance_game::field::FieldCheckpoint {
        map_id: 332,
        position: [1968., 1005., 0.],
        heading: 276.,
        camera: Some(camera.settings(leader).map_err(anyhow::Error::msg)?),
        allow_incomplete_scripts: false,
        progress,
        played_ticks: 0,
    })
}

pub(crate) fn actor(side: Side, hp: i32, tp: u16) -> Actor {
    Actor::new(
        side,
        Default::default(),
        resonance_battle::EquipmentAttributes {
            normal_combo_limit: 3,
            ..resonance_battle::EquipmentAttributes::new(hp, tp, Default::default())
        },
    )
}

pub(crate) fn prepare_battle(
    actors: impl IntoIterator<Item = Actor>,
) -> anyhow::Result<PreparedBattle> {
    PreparedBattle::new(
        actors
            .into_iter()
            .map(|actor| (actor, Default::default()))
            .collect(),
        Default::default(),
        1,
    )
}

/// Representative authored startup used by timing and replay tests.
pub(crate) fn boot_logos() -> resonance_game::boot::Logos {
    let sequence = serde_json::from_value(serde_json::json!([
        {"image":"nintendo.ktx2","size":[7,5],"fade_out_at":180},
        {"image":"namco.ktx2","size":[7,5],"fade_out_at":240,"skip_after":120},
        {"image":"tales-studio.ktx2","size":[7,5],"fade_out_at":180},
        {"image":"adx.ktx2","size":[7,5],"fade_out_at":120}
    ]))
    .unwrap();
    resonance_game::boot::Logos::new(sequence).unwrap()
}

/// Compile every composed variant through Naga as the GPU pipeline does.
pub(crate) fn validate_shader(
    _: &(),
    source: ShaderCacheSource,
    _: &bevy::shader::ValidateShader,
) -> Result<(), ShaderCacheError> {
    let ShaderCacheSource::Wgsl(source) = source else {
        panic!("expected composed WGSL");
    };
    let module = wgpu::naga::front::wgsl::parse_str(&source)
        .map_err(|error| ShaderCacheError::ProcessShaderError(error.emit_to_string(&source)))?;
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|error| ShaderCacheError::ProcessShaderError(error.emit_to_string(&source)))?;
    Ok(())
}

/// Headless native renderer shared by GPU tests.
pub(crate) fn render_plugins() -> bevy::app::PluginGroupBuilder {
    render_plugins_with_features(wgpu::Features::empty())
}

/// Finish asynchronous plugin setup before a GPU test starts rendering.
pub(crate) fn finish_app(app: &mut App) -> anyhow::Result<()> {
    let began = std::time::Instant::now();
    while app.plugins_state() == bevy::app::PluginsState::Adding {
        anyhow::ensure!(began.elapsed().as_secs() < 180, "GPU setup timed out");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    Ok(())
}

pub(crate) fn render_until(
    app: &mut App,
    message: &str,
    ready: impl Fn(&App) -> bool,
) -> anyhow::Result<()> {
    let began = std::time::Instant::now();
    loop {
        app.update();
        crate::playthrough::check_exit(app)?;
        if ready(app) {
            return Ok(());
        }
        anyhow::ensure!(began.elapsed().as_secs() < 120, "{message}");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Capture the real rendered image; each call waits for its own GPU readback.
pub(crate) fn capture_image(app: &mut App, target: Handle<Image>) -> anyhow::Result<Image> {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
    #[derive(Resource, Default)]
    struct Captured(Option<Image>);

    app.init_resource::<Captured>();
    app.world_mut().spawn(Screenshot::image(target)).observe(
        |shot: On<ScreenshotCaptured>, mut captured: ResMut<Captured>| {
            captured.0 = Some(shot.image.clone());
        },
    );
    render_until(app, "GPU readback timed out", |app| {
        app.world().resource::<Captured>().0.is_some()
    })?;
    Ok(app.world_mut().resource_mut::<Captured>().0.take().unwrap())
}

/// The portable feature set plus `features`, so software adapters qualify.
pub(crate) fn render_plugins_with_features(
    features: wgpu::Features,
) -> bevy::app::PluginGroupBuilder {
    use bevy::{
        prelude::*,
        render::{
            RenderPlugin,
            settings::{Backends, WgpuSettings, WgpuSettingsPriority},
        },
    };
    let backends = (if cfg!(target_os = "macos") {
        Backends::METAL
    } else if cfg!(target_os = "windows") {
        Backends::DX12
    } else {
        Backends::VULKAN
    })
    .with_env();
    crate::renderer::render_plugins(RenderPlugin {
        render_creation: WgpuSettings {
            backends: Some(backends),
            features,
            priority: WgpuSettingsPriority::WebGPU,
            ..default()
        }
        .into(),
        synchronous_pipeline_compilation: true,
        ..default()
    })
    .set(WindowPlugin {
        primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit,
        ..default()
    })
    .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
    .disable::<bevy::winit::WinitPlugin>()
    .disable::<bevy::gilrs::GilrsPlugin>()
}

/// Install the native frame renderer and point it at `scene`: cameras drawing
/// into that image are drawn as the game's scene. The framebuffer is a small
/// image of its own that test cameras do not draw into. Call it before the app
/// finishes building.
pub(crate) fn native_scene(
    app: &mut bevy::app::App,
    scene: bevy::asset::Handle<bevy::image::Image>,
) {
    use bevy::{camera::RenderTarget, prelude::*, render::render_resource::TextureFormat};
    if !app
        .world()
        .contains_resource::<crate::materials::SurfaceRecords>()
    {
        crate::materials::install_surface_material(app);
    }
    if !app.is_plugin_added::<crate::anti_aliasing::AntiAliasingPlugin>() {
        app.add_plugins(crate::anti_aliasing::AntiAliasingPlugin);
    }
    let framebuffer =
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                1,
                1,
                TextureFormat::Bgra8UnormSrgb,
                None,
            ));
    crate::set_display_targets(
        app.world_mut(),
        scene,
        RenderTarget::Image(framebuffer.into()),
    );
}
