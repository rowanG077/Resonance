//! Saved backend selection and discovery of native, WebGPU-capable adapters.
use bevy::{
    prelude::*,
    render::{RenderPlugin, renderer::RenderAdapterInfo, settings::WgpuSettings},
};
use resonance_content::menu_data::GraphicsBackend;

#[derive(Resource)]
struct Candidates(Vec<wgpu::AdapterInfo>);

#[derive(Resource)]
struct Capabilities {
    choices: Vec<GraphicsBackend>,
    active: GraphicsBackend,
    overridden: bool,
}

fn native_default() -> GraphicsBackend {
    if cfg!(target_os = "windows") {
        GraphicsBackend::Dx12
    } else if cfg!(target_os = "macos") {
        GraphicsBackend::Metal
    } else {
        GraphicsBackend::Vulkan
    }
}

fn backends(backend: GraphicsBackend) -> wgpu::Backends {
    match backend {
        GraphicsBackend::Automatic => wgpu::Backends::PRIMARY,
        GraphicsBackend::Dx12 => wgpu::Backends::DX12,
        GraphicsBackend::Vulkan => wgpu::Backends::VULKAN,
        GraphicsBackend::Metal => wgpu::Backends::METAL,
    }
}

fn from_wgpu(backend: wgpu::Backend) -> Option<GraphicsBackend> {
    match backend {
        wgpu::Backend::Dx12 => Some(GraphicsBackend::Dx12),
        wgpu::Backend::Vulkan => Some(GraphicsBackend::Vulkan),
        wgpu::Backend::Metal => Some(GraphicsBackend::Metal),
        _ => None,
    }
}

fn resolve(
    preference: GraphicsBackend,
    default: GraphicsBackend,
    available: &[GraphicsBackend],
) -> GraphicsBackend {
    if preference != GraphicsBackend::Automatic && available.contains(&preference) {
        preference
    } else if available.contains(&default) {
        default
    } else {
        available.first().copied().unwrap_or(default)
    }
}

fn default_settings() -> WgpuSettings {
    WgpuSettings {
        #[cfg(target_os = "windows")]
        backends: Some(wgpu::Backends::DX12.with_env()),
        ..default()
    }
}

fn filter_adapters(adapters: &mut Vec<wgpu::AdapterInfo>, name: Option<&str>, fallback: bool) {
    // Match Bevy: a matching name wins; an unmatched name falls back to normal
    // selection. Hardware takes precedence over software unless fallback is
    // requested, so Automatic never chooses WARP over a working Vulkan GPU.
    if let Some(name) = name.map(str::to_lowercase)
        && adapters
            .iter()
            .any(|info| info.name.to_lowercase().contains(&name))
    {
        adapters.retain(|info| info.name.to_lowercase().contains(&name));
    } else if fallback {
        adapters.retain(|info| info.device_type == wgpu::DeviceType::Cpu);
    } else if adapters
        .iter()
        .any(|info| info.device_type != wgpu::DeviceType::Cpu)
    {
        adapters.retain(|info| info.device_type != wgpu::DeviceType::Cpu);
    }
}

pub(crate) fn plugin(app: &mut App, interactive: bool) -> RenderPlugin {
    let mut settings = default_settings();
    if interactive {
        // GL/ANGLE is not shipped. Discover native backends once for the player
        // settings menu; headless captures keep Bevy's normal initialization.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: settings.instance_flags,
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: settings.dx12_shader_compiler.clone(),
                    ..default()
                },
                ..default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let mut candidates: Vec<_> =
            bevy::tasks::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY))
                .into_iter()
                .filter(|adapter| {
                    adapter.features().contains(settings.features)
                        && settings.limits.check_limits(&adapter.limits())
                        && adapter.get_downlevel_capabilities().is_webgpu_compliant()
                })
                .map(|adapter| adapter.get_info())
                .collect();
        let adapter_name = std::env::var("WGPU_ADAPTER_NAME").ok();
        let fallback = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER")
            .map_or(settings.force_fallback_adapter, |value| {
                !(value.is_empty() || value == "0" || value == "false")
            });
        filter_adapters(&mut candidates, adapter_name.as_deref(), fallback);
        let available: Vec<_> = candidates
            .iter()
            .filter_map(|info| from_wgpu(info.backend))
            .collect();
        let preference = app
            .world()
            .resource::<crate::config::Preferences>()
            .settings
            .graphics_backend;
        let selected = resolve(preference, native_default(), &available);
        settings.backends = Some(backends(selected).with_env());
        app.insert_resource(Candidates(candidates))
            .add_systems(Startup, detect)
            .add_systems(First, constrain);
    }
    RenderPlugin {
        render_creation: settings.into(),
        ..default()
    }
}

fn choices(candidates: &[wgpu::AdapterInfo], adapter: &wgpu::AdapterInfo) -> Vec<GraphicsBackend> {
    let mut choices = vec![GraphicsBackend::Automatic];
    // Do not offer support found only on a different GPU or software adapter.
    for backend in [
        native_default(),
        GraphicsBackend::Dx12,
        GraphicsBackend::Vulkan,
        GraphicsBackend::Metal,
    ] {
        if (from_wgpu(adapter.backend) == Some(backend)
            || candidates.iter().any(|info| {
                info.vendor == adapter.vendor
                    && info.device == adapter.device
                    && info.device_type == adapter.device_type
                    && from_wgpu(info.backend) == Some(backend)
            }))
            && !choices.contains(&backend)
        {
            choices.push(backend);
        }
    }
    choices
}

fn detect(mut commands: Commands, candidates: Res<Candidates>, adapter: Res<RenderAdapterInfo>) {
    let Some(active) = from_wgpu(adapter.backend) else {
        return;
    };
    let choices = choices(&candidates.0, &adapter);
    info!("Available graphics backends: {choices:?}; active={active:?}");
    commands.remove_resource::<Candidates>();
    commands.insert_resource(Capabilities {
        choices,
        active,
        overridden: wgpu::Backends::from_env().is_some(),
    });
}

fn constrain(
    caps: Option<Res<Capabilities>>,
    mut title: Option<ResMut<crate::saves::title::TitleMenu>>,
    mut session: Option<ResMut<crate::new_game::Session>>,
) {
    let Some(caps) = caps else { return };
    let apply = |menu: &mut resonance_game::menu::Menu| {
        if menu.customize.backend_choices() != caps.choices
            || menu.customize.active_backend != Some(caps.active)
            || menu.customize.backend_override != caps.overridden
        {
            menu.customize
                .set_backend_choices(&caps.choices, caps.active, caps.overridden);
        }
    };
    if let Some(title) = &mut title {
        apply(&mut title.0);
    }
    if let Some(menu) = session.as_mut().and_then(|session| session.menu_mut()) {
        apply(menu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "opens a native graphics adapter; no game assets or window required"]
    fn native_backend_discovery_and_saved_backend_fallback() {
        let mut app = App::new();
        app.init_resource::<crate::config::Preferences>();
        // Exercise a saved backend that is unavailable on this platform.
        app.world_mut()
            .resource_mut::<crate::config::Preferences>()
            .settings
            .graphics_backend = if cfg!(target_os = "macos") {
            GraphicsBackend::Dx12
        } else {
            GraphicsBackend::Metal
        };
        let renderer = plugin(&mut app, true);
        app.add_plugins(
            DefaultPlugins
                .set(renderer)
                .set(WindowPlugin {
                    primary_window: None,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::gilrs::GilrsPlugin>(),
        );
        app.finish();
        app.cleanup();
        app.update();
        let caps = app.world().resource::<Capabilities>();
        assert!(caps.choices.contains(&GraphicsBackend::Automatic));
        assert!(caps.choices.contains(&caps.active));
        assert_ne!(caps.active, GraphicsBackend::Automatic);
    }

    fn adapter(backend: wgpu::Backend, device: u32) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: format!("GPU {device}"),
            vendor: 1,
            device,
            device_type: wgpu::DeviceType::DiscreteGpu,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend,
            subgroup_min_size: 32,
            subgroup_max_size: 32,
            transient_saves_memory: Some(false),
            limit_bucket: None,
        }
    }

    #[test]
    fn menu_backends_require_support_on_the_active_gpu() {
        use GraphicsBackend::*;
        let active = adapter(wgpu::Backend::Dx12, 1);
        let other_gpu = adapter(wgpu::Backend::Vulkan, 2);
        let mut software = adapter(wgpu::Backend::Vulkan, 1);
        software.device_type = wgpu::DeviceType::Cpu;
        assert_eq!(
            choices(&[other_gpu, software], &active),
            vec![Automatic, Dx12]
        );
        let supported = choices(&[adapter(wgpu::Backend::Vulkan, 1)], &active);
        assert_eq!(supported.len(), 3);
        assert!(supported.contains(&Vulkan));
    }

    #[test]
    fn discovery_respects_named_and_software_adapter_selection() {
        let hardware = adapter(wgpu::Backend::Dx12, 1);
        let mut software = adapter(wgpu::Backend::Vulkan, 2);
        software.device_type = wgpu::DeviceType::Cpu;
        let mut candidates = vec![hardware.clone(), software.clone()];
        filter_adapters(&mut candidates, None, true);
        assert_eq!(candidates, [software.clone()]);
        candidates = vec![hardware.clone(), software.clone()];
        filter_adapters(&mut candidates, Some("gPu 1"), true);
        assert_eq!(candidates, [hardware.clone()]);
        candidates = vec![hardware.clone(), software.clone()];
        filter_adapters(&mut candidates, Some("missing"), false);
        assert_eq!(candidates, [hardware]);

        // The native default must not select a software GPU over real hardware.
        let mut hardware = adapter(wgpu::Backend::Vulkan, 1);
        hardware.device_type = wgpu::DeviceType::IntegratedGpu;
        software.backend = wgpu::Backend::Dx12;
        candidates = vec![software, hardware];
        filter_adapters(&mut candidates, None, false);
        let available: Vec<_> = candidates
            .iter()
            .filter_map(|info| from_wgpu(info.backend))
            .collect();
        assert_eq!(
            resolve(
                GraphicsBackend::Automatic,
                GraphicsBackend::Dx12,
                &available
            ),
            GraphicsBackend::Vulkan
        );
    }

    #[test]
    fn headless_setup_needs_neither_preferences_nor_backend_discovery() {
        let mut app = App::new();
        plugin(&mut app, false);
        assert!(!app.world().contains_resource::<Candidates>());
    }

    #[test]
    fn selection_prefers_windows_native_but_honors_saved_choices_and_missing_drivers() {
        use GraphicsBackend::*;
        assert_eq!(resolve(Automatic, Dx12, &[Vulkan, Dx12]), Dx12);
        assert_eq!(resolve(Vulkan, Dx12, &[Vulkan, Dx12]), Vulkan);
        assert_eq!(resolve(Metal, Dx12, &[Vulkan, Dx12]), Dx12);
        assert_eq!(resolve(Automatic, Dx12, &[Vulkan]), Vulkan);
    }
}
