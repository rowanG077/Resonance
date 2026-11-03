//! Saved backend selection and discovery of compatible native adapters.
use bevy::{
    prelude::*,
    render::{RenderPlugin, renderer::RenderAdapterInfo, settings::WgpuSettings},
};
use resonance_content::menu_data::GraphicsBackend;

const NATIVE_BACKENDS: wgpu::Backends = wgpu::Backends::PRIMARY.union(wgpu::Backends::GL);

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
        GraphicsBackend::Automatic => NATIVE_BACKENDS,
        GraphicsBackend::Dx12 => wgpu::Backends::DX12,
        GraphicsBackend::Vulkan => wgpu::Backends::VULKAN,
        GraphicsBackend::Metal => wgpu::Backends::METAL,
        GraphicsBackend::OpenGl => wgpu::Backends::GL,
    }
}

fn from_wgpu(backend: wgpu::Backend) -> Option<GraphicsBackend> {
    match backend {
        wgpu::Backend::Dx12 => Some(GraphicsBackend::Dx12),
        wgpu::Backend::Vulkan => Some(GraphicsBackend::Vulkan),
        wgpu::Backend::Metal => Some(GraphicsBackend::Metal),
        wgpu::Backend::Gl => Some(GraphicsBackend::OpenGl),
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

fn compatible_downlevel(backend: wgpu::Backend, caps: &wgpu::DownlevelCapabilities) -> bool {
    if backend != wgpu::Backend::Gl {
        return caps.is_webgpu_compliant();
    }
    // GL cannot expose these WebGPU operations. Our forward renderer does not
    // use them; texture/MSAA support is checked separately by anti_aliasing.
    let unused = wgpu::DownlevelFlags::READ_ONLY_DEPTH_STENCIL
        | wgpu::DownlevelFlags::DEPTH_TEXTURE_AND_BUFFER_COPIES
        | wgpu::DownlevelFlags::WEBGPU_TEXTURE_FORMAT_SUPPORT
        // Authored meshes do not need the full u32 vertex-index range. GL
        // drivers may report a smaller maximum while supporting our meshes.
        | wgpu::DownlevelFlags::FULL_DRAW_INDEX_UINT32
        | wgpu::DownlevelFlags::DEPTH_BIAS_CLAMP
        | wgpu::DownlevelFlags::VIEW_FORMATS
        | wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS
        | wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE;
    caps.flags
        .contains(wgpu::DownlevelFlags::compliant() - unused)
        && caps.shader_model >= wgpu::ShaderModel::Sm5
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

pub(super) fn force_fallback(settings: &WgpuSettings) -> bool {
    std::env::var("WGPU_FORCE_FALLBACK_ADAPTER").map_or(settings.force_fallback_adapter, |value| {
        !(value.is_empty() || value == "0" || value == "false")
    })
}

pub(crate) fn plugin(app: &mut App, interactive: bool) -> RenderPlugin {
    info!(
        "Initializing graphics; interactive={interactive}, requested backend={:?}, adapter={:?}",
        std::env::var_os("WGPU_BACKEND"),
        std::env::var_os("WGPU_ADAPTER_NAME")
    );
    let mut settings = default_settings();
    if interactive {
        // Discover compatible backends once for the player settings menu;
        // headless captures keep Bevy's normal initialization.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: NATIVE_BACKENDS,
            flags: settings.instance_flags,
            backend_options: wgpu::BackendOptions {
                gl: wgpu::GlBackendOptions {
                    gles_minor_version: settings.gles3_minor_version,
                    ..default()
                },
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: settings.dx12_shader_compiler.clone(),
                    ..default()
                },
                ..default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let mut candidates: Vec<_> =
            bevy::tasks::block_on(instance.enumerate_adapters(NATIVE_BACKENDS))
                .into_iter()
                .filter(|adapter| {
                    (adapter.get_info().backend != wgpu::Backend::Gl
                        || super::opengl::compatible_color_target(adapter))
                        && adapter.features().contains(settings.features)
                        && settings.limits.check_limits(&adapter.limits())
                        && compatible_downlevel(
                            adapter.get_info().backend,
                            &adapter.get_downlevel_capabilities(),
                        )
                })
                .map(|adapter| adapter.get_info())
                .collect();
        let adapter_name = std::env::var("WGPU_ADAPTER_NAME").ok();
        let fallback = force_fallback(&settings);
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
        info!(
            "Graphics selection: saved={preference:?}, selected={selected:?}, adapters={candidates:?}, fallback={fallback}"
        );
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

fn same_gpu(a: &wgpu::AdapterInfo, b: &wgpu::AdapterInfo) -> bool {
    if a.backend != wgpu::Backend::Gl && b.backend != wgpu::Backend::Gl {
        return a.vendor == b.vendor && a.device == b.device && a.device_type == b.device_type;
    }
    // GL reports no PCI device ID and often classifies discrete GPUs as Other.
    // Compare renderer names, ignoring driver suffixes such as (RADV NAVI21)
    // and (radeonsi, ...). Never equate a software renderer with hardware.
    let name = |info: &wgpu::AdapterInfo| {
        let name = info.name.to_lowercase();
        name.strip_prefix("mesa ")
            .unwrap_or(&name)
            .split(" (")
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches("/pcie/sse2")
            .to_owned()
    };
    let a_name = name(a);
    !a_name.is_empty()
        && a_name == name(b)
        && (a.device_type == wgpu::DeviceType::Cpu) == (b.device_type == wgpu::DeviceType::Cpu)
}

fn choices(candidates: &[wgpu::AdapterInfo], adapter: &wgpu::AdapterInfo) -> Vec<GraphicsBackend> {
    let mut choices = vec![GraphicsBackend::Automatic];
    // Do not offer support found only on a different GPU or software adapter.
    for backend in [
        native_default(),
        GraphicsBackend::Dx12,
        GraphicsBackend::Vulkan,
        GraphicsBackend::Metal,
        GraphicsBackend::OpenGl,
    ] {
        if (from_wgpu(adapter.backend) == Some(backend)
            || candidates
                .iter()
                .any(|info| same_gpu(info, adapter) && from_wgpu(info.backend) == Some(backend)))
            && !choices.contains(&backend)
        {
            choices.push(backend);
        }
    }
    choices
}

fn detect(mut commands: Commands, candidates: Res<Candidates>, adapter: Res<RenderAdapterInfo>) {
    info!("Active graphics adapter: {:?}", **adapter);
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
            super::super::native_render_plugins(renderer)
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
    fn opengl_matches_renderer_names_without_pci_ids() {
        use GraphicsBackend::*;
        let mut vulkan = adapter(wgpu::Backend::Vulkan, 0x744c);
        vulkan.name = "AMD Radeon RX 7900 XTX (RADV NAVI31)".into();
        let mut gl = adapter(wgpu::Backend::Gl, 0);
        gl.device_type = wgpu::DeviceType::Other;
        gl.name = "AMD Radeon RX 7900 XTX (radeonsi, LLVM 21.1.0)".into();
        assert!(choices(&[gl.clone()], &vulkan).contains(&OpenGl));
        assert!(choices(&[vulkan.clone()], &gl).contains(&Vulkan));
        gl.name = "AMD Radeon RX 6800 XT (radeonsi)".into();
        assert!(!choices(&[gl.clone()], &vulkan).contains(&OpenGl));
        gl.name = vulkan.name.clone();
        gl.device_type = wgpu::DeviceType::Cpu;
        assert!(!choices(&[gl], &vulkan).contains(&OpenGl));

        vulkan.name = "NVIDIA GeForce RTX 3060".into();
        let mut gl = adapter(wgpu::Backend::Gl, 0);
        gl.device_type = wgpu::DeviceType::Other;
        gl.name = "NVIDIA GeForce RTX 3060/PCIe/SSE2".into();
        assert!(choices(&[gl.clone()], &vulkan).contains(&OpenGl));
        assert!(choices(&[vulkan.clone()], &gl).contains(&Vulkan));
        gl.name = "NVIDIA GeForce RTX 3070/PCIe/SSE2".into();
        assert!(!choices(&[gl], &vulkan).contains(&OpenGl));
    }

    #[test]
    fn opengl_requires_storage_and_compute_but_not_unused_webgpu_operations() {
        let mut caps = wgpu::DownlevelCapabilities::default();
        // Actual missing flags from Mesa on Apple M2 Max. In particular, the
        // index-range flag must not hide a working GL adapter from the menu.
        caps.flags.remove(
            wgpu::DownlevelFlags::READ_ONLY_DEPTH_STENCIL
                | wgpu::DownlevelFlags::DEPTH_TEXTURE_AND_BUFFER_COPIES
                | wgpu::DownlevelFlags::WEBGPU_TEXTURE_FORMAT_SUPPORT
                | wgpu::DownlevelFlags::FULL_DRAW_INDEX_UINT32
                | wgpu::DownlevelFlags::DEPTH_BIAS_CLAMP
                | wgpu::DownlevelFlags::VIEW_FORMATS
                | wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS,
        );
        assert!(compatible_downlevel(wgpu::Backend::Gl, &caps));
        assert!(!compatible_downlevel(wgpu::Backend::Vulkan, &caps));
        for required in [
            wgpu::DownlevelFlags::COMPUTE_SHADERS,
            wgpu::DownlevelFlags::VERTEX_STORAGE,
            wgpu::DownlevelFlags::FRAGMENT_STORAGE,
        ] {
            let mut missing = caps.clone();
            missing.flags.remove(required);
            assert!(!compatible_downlevel(wgpu::Backend::Gl, &missing));
        }
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
        assert_eq!(resolve(OpenGl, Dx12, &[Vulkan, OpenGl]), OpenGl);
        assert_eq!(resolve(Automatic, Vulkan, &[OpenGl]), OpenGl);
        assert_eq!(resolve(OpenGl, Vulkan, &[Vulkan]), Vulkan);
        assert_eq!(backends(OpenGl), wgpu::Backends::GL);
        assert_eq!(from_wgpu(wgpu::Backend::Gl), Some(OpenGl));
    }
}
