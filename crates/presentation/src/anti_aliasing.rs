//! Spatial scene antialiasing. UI, movies and final presentation never run SMAA.
use bevy::render::{
    render_resource::{TextureFormat, TextureFormatFeatureFlags, TextureUsages},
    renderer::{RenderAdapter, RenderDevice},
    settings::WgpuFeatures,
};
use bevy::{camera::RenderTarget, prelude::*};
use bevy_post_process::msaa_writeback::MsaaWritebackPlugin;
use resonance_content::menu_data::AntiAliasing;
mod smaa;
use smaa::Smaa;

/// Choices valid for all scene color formats and sampled refraction depth.
/// Created once the render device is available, before the first scene frame.
#[derive(Resource, Debug)]
pub struct AntiAliasingCapabilities(Vec<AntiAliasing>);
impl AntiAliasingCapabilities {
    pub fn available_modes(&self) -> &[AntiAliasing] {
        &self.0
    }
    pub fn supports(&self, mode: AntiAliasing) -> bool {
        self.0.contains(&mode)
    }
    pub fn effective(&self, mode: AntiAliasing) -> AntiAliasing {
        mode.supported_or_fallback(&self.0)
    }
    fn detect(adapter: &RenderAdapter, device: &RenderDevice) -> Self {
        let formats = [
            TextureFormat::Bgra8Unorm,
            TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Rgba8Unorm,
            TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Depth32Float,
        ];
        let features = formats.map(|format| {
            (
                format,
                if device
                    .features()
                    .contains(WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
                {
                    adapter.get_texture_format_features(format)
                } else {
                    format.guaranteed_format_features(device.features())
                },
            )
        });
        Self(
            AntiAliasing::ALL
                .into_iter()
                .filter(|mode| {
                    features.iter().all(|(format, features)| {
                        features.flags.sample_count_supported(mode.samples())
                            && features.allowed_usages.contains(
                                TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                            )
                            && (mode.samples() == 1
                                || *format == TextureFormat::Depth32Float
                                || features
                                    .flags
                                    .contains(TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
                    })
                })
                .collect(),
        )
    }
}
fn detect_capabilities(
    mut commands: Commands,
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    preferences: Option<ResMut<crate::config::Preferences>>,
) {
    let caps = AntiAliasingCapabilities::detect(&adapter, &device);
    if let Some(mut preferences) = preferences {
        let effective = caps.effective(preferences.settings.anti_aliasing);
        if preferences.settings.anti_aliasing != effective {
            preferences.settings.anti_aliasing = effective;
        }
    }
    info!("Available antialiasing: {:?}", caps.available_modes());
    commands.insert_resource(caps);
}
fn constrain_menu_choices(
    caps: Option<Res<AntiAliasingCapabilities>>,
    title: Option<ResMut<crate::saves::title::TitleMenu>>,
    session: Option<ResMut<crate::new_game::Session>>,
) {
    let Some(caps) = caps else {
        return;
    };
    let choices = caps.available_modes();
    if let Some(mut title) = title
        && title.0.customize.anti_aliasing_choices() != choices
    {
        title.0.customize.set_anti_aliasing_choices(choices);
    }
    if let Some(mut session) = session
        && session
            .menu()
            .is_some_and(|menu| menu.customize.anti_aliasing_choices() != choices)
        && let Some(menu) = session.menu_mut()
    {
        menu.customize.set_anti_aliasing_choices(choices);
    }
}
/// Active renderer choice. Diagnostics default to exact, unfiltered rendering.
#[derive(Resource, PartialEq, Eq)]
pub struct AntiAliasingSettings(pub AntiAliasing);
impl Default for AntiAliasingSettings {
    fn default() -> Self {
        Self(AntiAliasing::Off)
    }
}

/// Installs only spatial SMAA; no temporal AA plugin, history or jitter is used.
pub struct AntiAliasingPlugin;
impl Plugin for AntiAliasingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AntiAliasingSettings>()
            // Writeback preserves the filtered scene when a later MSAA UI
            // camera loads it. No aggregate AA or temporal plugin is installed.
            .add_plugins(MsaaWritebackPlugin)
            .add_systems(Startup, detect_capabilities)
            .add_systems(First, constrain_menu_choices)
            .add_systems(Last, configure_cameras);
        smaa::install(app);
    }
}

fn configure_cameras(
    mut commands: Commands,
    settings: Res<AntiAliasingSettings>,
    caps: Res<AntiAliasingCapabilities>,
    primary: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut cameras: Query<(
        Entity,
        &RenderTarget,
        Has<Camera3d>,
        &mut Msaa,
        Option<&Smaa>,
    )>,
) {
    let choice = caps.effective(settings.0);
    let primary = primary.single().ok();
    let scene_targets: Vec<_> = cameras
        .iter()
        .filter(|(_, _, scene, _, _)| *scene)
        .filter_map(|(_, target, _, _, _)| target.normalize(primary))
        .collect();
    for (entity, target, scene, mut msaa, current_smaa) in &mut cameras {
        // Bevy shares scene/UI attachments only when sample counts agree. UI
        // cameras retain the resolved scene through Bevy's MSAA writeback pass.
        let desired = if choice.samples() > 1
            && (scene
                || target
                    .normalize(primary)
                    .is_some_and(|t| scene_targets.contains(&t)))
        {
            match choice.samples() {
                2 => Msaa::Sample2,
                4 => Msaa::Sample4,
                8 => Msaa::Sample8,
                _ => Msaa::Off,
            }
        } else {
            Msaa::Off
        };
        if *msaa != desired {
            *msaa = desired;
        }
        let desired_smaa = scene.then(|| Smaa::new(choice)).flatten();
        if current_smaa.copied() != desired_smaa {
            if let Some(smaa) = desired_smaa {
                commands.entity(entity).insert(smaa);
            } else {
                commands.entity(entity).remove::<Smaa>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_gpu_modes_are_hidden_and_fall_back_safely() {
        let caps = AntiAliasingCapabilities(
            AntiAliasing::ALL
                .into_iter()
                .filter(|mode| matches!(mode.samples(), 1 | 4))
                .collect(),
        );
        assert_eq!(
            caps.available_modes(),
            vec![
                AntiAliasing::Off,
                AntiAliasing::SmaaLow,
                AntiAliasing::SmaaMedium,
                AntiAliasing::SmaaHigh,
                AntiAliasing::SmaaUltra,
                AntiAliasing::Msaa4
            ]
        );
        assert_eq!(caps.effective(AntiAliasing::Msaa8), AntiAliasing::Msaa4);
        assert_eq!(caps.effective(AntiAliasing::Msaa2), AntiAliasing::Msaa4);
        assert_eq!(
            caps.effective(AntiAliasing::SmaaS2x),
            AntiAliasing::SmaaUltra
        );
    }
    #[test]
    #[ignore = "requires current cooked assets and a GPU; renders the production field/refraction pipeline"]
    fn spatial_aa_renders_a_cooked_field_with_refraction_depth() -> anyhow::Result<()> {
        use resonance_content::{diagnostics::Diagnostics, library::Library, prepared::Files};
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .expect("set RESONANCE_TEST_ASSETS");
        let mut snapshot = Library::open(&root, &[])?.snapshot();
        let diagnostics = Diagnostics::new(true);
        resonance_content::preparation::gameplay(&mut snapshot, &diagnostics, &|| false)?;
        let files = Files::from_snapshot(snapshot, || false, diagnostics)?;
        let checkpoint = crate::test_support::field_checkpoint(&files)?;
        let work = tempfile::tempdir()?;
        let save = work.path().join("checkpoint.json");
        crate::saves::prepare_checkpoint_fixture(&root, checkpoint, &save)?;
        let output = std::env::var_os("RESONANCE_AA_CAPTURE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| work.path().join("msaa-field.png"));
        let mut options = crate::test_support::run_options(root);
        options.capture = Some(output.clone());
        options.saves.directory = Some(work.path().join("slots"));
        options.saves.load = Some(save);
        let (mut app, _) = crate::build_app_with_display(options, crate::Resolution::default())?;
        let selected = std::env::var("RESONANCE_AA_MODE")
            .ok()
            .map(|mode| serde_json::from_value(serde_json::Value::String(mode)))
            .transpose()?
            .unwrap_or(AntiAliasing::Msaa4);
        app.world_mut().resource_mut::<AntiAliasingSettings>().0 = selected;
        // Software GPUs can spend more than the scenario recorder's 60-second
        // budget compiling the full field. Use a bounded test-only warmup and
        // the same production readiness/readback, without changing player limits.
        let began = std::time::Instant::now();
        while app.plugins_state() == bevy::app::PluginsState::Adding {
            anyhow::ensure!(began.elapsed().as_secs() < 180, "renderer setup timed out");
            bevy::tasks::tick_global_task_pools_on_main_thread();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        app.finish();
        app.cleanup();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ));
        loop {
            anyhow::ensure!(began.elapsed().as_secs() < 180, "AA field warmup timed out");
            app.update();
            crate::playthrough::check_exit(&app)?;
            let field = app
                .world()
                .get_resource::<crate::new_game::Session>()
                .is_some_and(|s| s.is_field() && s.ready_for_field);
            if field
                && app
                    .world()
                    .resource::<crate::loading::Resident>()
                    .active
                    .load(std::sync::atomic::Ordering::Acquire)
                && crate::new_game::scene_ready(app.world_mut())
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        anyhow::ensure!(
            app.world()
                .resource::<AntiAliasingCapabilities>()
                .supports(selected),
            "requested AA mode {selected:?} is not supported by the selected adapter"
        );
        let failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let written = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        crate::new_game_capture::screenshot_held(
            &mut app,
            output.clone(),
            failed.clone(),
            written.clone(),
        )?;
        assert!(!failed.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(written.load(std::sync::atomic::Ordering::Acquire), 1);
        assert!(output.is_file());
        Ok(())
    }

    #[test]
    fn live_choices_cover_new_cameras_and_preserve_ui_composition_without_temporal_jitter() {
        let mut app = App::new();
        app.init_resource::<AntiAliasingSettings>()
            .insert_resource(AntiAliasingCapabilities(AntiAliasing::ALL.to_vec()))
            .add_systems(Last, configure_cameras);
        let target = RenderTarget::Image(Handle::<Image>::default().into());
        let scene = app
            .world_mut()
            .spawn((Camera3d::default(), target.clone(), Msaa::Off))
            .id();
        let ui = app
            .world_mut()
            .spawn((Camera2d, target.clone(), Msaa::Off))
            .id();
        let output = app.world_mut().spawn((Camera2d, Msaa::Off)).id();
        for choice in AntiAliasing::ALL.into_iter().chain([AntiAliasing::Off]) {
            app.world_mut().resource_mut::<AntiAliasingSettings>().0 = choice;
            app.update();
            assert_eq!(
                app.world().get::<Msaa>(scene).unwrap().samples(),
                choice.samples()
            );
            assert_eq!(app.world().get::<Msaa>(ui), app.world().get::<Msaa>(scene));
            assert_eq!(app.world().get::<Msaa>(output), Some(&Msaa::Off));
            assert!(app.world().get::<Smaa>(scene).copied() == Smaa::new(choice));
            assert!(app.world().get::<Smaa>(ui).is_none());
            assert!(app.world().get::<Smaa>(output).is_none());
            assert!(
                app.world()
                    .get::<bevy::render::camera::TemporalJitter>(scene)
                    .is_none()
            );
        }
        app.world_mut().resource_mut::<AntiAliasingSettings>().0 = AntiAliasing::Msaa4;
        let preview = app.world_mut().spawn((Camera3d::default(), Msaa::Off)).id();
        app.update();
        assert!(app.world().get::<Smaa>(preview).is_none());
        assert_eq!(app.world().get::<Msaa>(preview), Some(&Msaa::Sample4));
    }
}
