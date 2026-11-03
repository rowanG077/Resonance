//! Adapt Bevy's native renderer to wgpu's EGL/OpenGL restrictions.
//! Initialize EGL on the window display and use normalized scene targets to clamp
//! blending between draws. SMAA handles color conversion explicitly on these targets.
use bevy::{
    pbr::{ShadowView, ViewShadowBindings, prepare_lights},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        camera::CameraMainPassTextureFormats,
        render_resource::{
            Extent3d, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
            TextureView, TextureViewDescriptor, TextureViewDimension,
        },
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::{RenderCreation, WgpuSettings, WgpuSettingsPriority},
        texture::TextureCache,
        view::ExtractedView,
    },
    window::{PrimaryWindow, RawHandleWrapperHolder},
    winit::DisplayHandleWrapper,
};

pub(super) const COLOR_FORMAT: TextureFormat = TextureFormat::Rgba16Unorm;

// Normalized attachments preserve per-draw clamping without the alternate
// sRGB views Bevy requests for 8-bit targets. SMAA converts explicitly on GL.
fn install_opengl_compatibility(app: &mut App) {
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render.add_systems(
        Render,
        (
            compatible_targets
                .after(RenderSystems::ExtractCommands)
                .before(RenderSystems::PrepareAssets),
            compatible_empty_shadows
                .after(prepare_lights)
                .in_set(RenderSystems::PrepareResources),
        ),
    );
}

// Resonance uses authored lighting, but Bevy still binds empty shadow maps for
// every 3D view. GL infers the texture target from its layer count: six means
// Cube and one means D2, whereas the native PBR layout requires arrays. Two
// empty layers/cubes make those targets unambiguous. Keep real shadow maps.
fn compatible_empty_shadows(
    adapter: Res<RenderAdapterInfo>,
    device: Res<RenderDevice>,
    shadows: Query<(), With<ShadowView>>,
    mut views: Query<&mut ViewShadowBindings>,
    mut cache: ResMut<TextureCache>,
) {
    if adapter.backend != wgpu::Backend::Gl || !shadows.is_empty() || views.is_empty() {
        return;
    }
    let (point, point_view) =
        shadow_array(&device, &mut cache, 12, TextureViewDimension::CubeArray);
    let (directional, directional_view) =
        shadow_array(&device, &mut cache, 2, TextureViewDimension::D2Array);
    for mut view in &mut views {
        view.point_light_depth_texture = point.clone();
        view.point_light_depth_texture_view = point_view.clone();
        view.directional_light_depth_texture = directional.clone();
        view.directional_light_depth_texture_view = directional_view.clone();
    }
}

fn shadow_array(
    device: &RenderDevice,
    cache: &mut TextureCache,
    layers: u32,
    dimension: TextureViewDimension,
) -> (Texture, TextureView) {
    let texture = cache
        .get(
            device,
            TextureDescriptor {
                label: Some("OpenGL empty shadow array"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Depth32Float,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
        )
        .texture;
    let view = texture.create_view(&TextureViewDescriptor {
        dimension: Some(dimension),
        ..default()
    });
    (texture, view)
}

pub(super) fn compatible_color_target(adapter: &wgpu::Adapter) -> bool {
    let format = adapter.get_texture_format_features(COLOR_FORMAT);
    adapter
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
        && format
            .allowed_usages
            .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
        && format.flags.contains(
            wgpu::TextureFormatFeatureFlags::BLENDABLE
                | wgpu::TextureFormatFeatureFlags::FILTERABLE,
        )
}

fn compatible_targets(
    adapter: Res<RenderAdapterInfo>,
    mut views: Query<(Entity, &mut ExtractedView)>,
    mut formats: ResMut<CameraMainPassTextureFormats>,
) {
    if adapter.backend != wgpu::Backend::Gl {
        return;
    }
    for (entity, mut view) in &mut views {
        if matches!(
            view.target_format,
            TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm
        ) {
            view.target_format = COLOR_FORMAT;
            formats.insert(entity, view.target_format);
        }
    }
}

/// Insert before (and disable) the default RenderPlugin in DefaultPlugins.
/// Initializes OpenGL with the window display, then installs compatibility targets.
pub(super) struct NativeRenderPlugin(pub RenderPlugin);

impl Plugin for NativeRenderPlugin {
    fn build(&self, app: &mut App) {
        let creation = match &self.0.render_creation {
            RenderCreation::Automatic(settings) => {
                if settings
                    .backends
                    .is_some_and(|backends| backends.contains(wgpu::Backends::GL))
                {
                    // Bevy 0.20 initializes wgpu without a display handle. EGL
                    // needs the event loop's display to present on Wayland/X11.
                    let window = app
                        .world_mut()
                        .query_filtered::<&RawHandleWrapperHolder, With<PrimaryWindow>>()
                        .single(app.world())
                        .ok()
                        .cloned();
                    create_renderer(
                        settings,
                        app.world().get_resource::<DisplayHandleWrapper>(),
                        window.as_ref(),
                    )
                    .into()
                } else {
                    RenderCreation::Automatic(settings.clone())
                }
            }
            RenderCreation::Manual(resources) => RenderCreation::Manual(resources.clone()),
        };
        app.add_plugins(RenderPlugin {
            render_creation: creation,
            synchronous_pipeline_compilation: self.0.synchronous_pipeline_compilation,
            debug_flags: self.0.debug_flags,
        });
        install_opengl_compatibility(app);
    }
}

fn create_renderer(
    settings: &WgpuSettings,
    display: Option<&DisplayHandleWrapper>,
    window: Option<&RawHandleWrapperHolder>,
) -> bevy::render::settings::RenderResources {
    bevy::tasks::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: settings.backends.unwrap(),
            flags: settings.instance_flags,
            memory_budget_thresholds: settings.instance_memory_budget_thresholds,
            backend_options: wgpu::BackendOptions {
                gl: wgpu::GlBackendOptions {
                    gles_minor_version: settings.gles3_minor_version,
                    ..default()
                },
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: settings.dx12_shader_compiler.clone(),
                    ..wgpu::Dx12BackendOptions::from_env_or_default()
                },
                ..default()
            },
            display: display.map(|display| Box::new(display.0.clone()) as _),
        });
        let surface = window.and_then(|window| {
            let window = window.0.lock().expect("window handle lock poisoned");
            window.as_ref().map(|window| {
                // SAFETY: Plugin initialization runs on the main thread.
                instance
                    .create_surface(unsafe { window.get_handle() })
                    .expect("Failed to create the window surface")
            })
        });
        let name = std::env::var("WGPU_ADAPTER_NAME")
            .ok()
            .or_else(|| settings.adapter_name.clone());
        let named = if let Some(name) = name {
            instance
                .enumerate_adapters(settings.backends.unwrap())
                .await
                .into_iter()
                .find(|adapter| {
                    surface
                        .as_ref()
                        .is_none_or(|surface| adapter.is_surface_supported(surface))
                        && adapter
                            .get_info()
                            .name
                            .to_lowercase()
                            .contains(&name.to_lowercase())
                })
        } else {
            None
        };
        let adapter = if let Some(adapter) = named {
            adapter
        } else {
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: settings.power_preference,
                    force_fallback_adapter: super::backend::force_fallback(settings),
                    compatible_surface: surface.as_ref(),
                    apply_limit_buckets: false,
                })
                .await
                .expect("No compatible graphics adapter on the window display")
        };
        let info = adapter.get_info();
        info!("Graphics adapter: {info:?}");
        let (mut features, mut limits) =
            if matches!(settings.priority, WgpuSettingsPriority::Functionality) {
                (adapter.features(), adapter.limits())
            } else {
                (wgpu::Features::empty(), settings.limits.clone())
            };
        if info.device_type == wgpu::DeviceType::DiscreteGpu {
            features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        }
        features.remove(settings.disabled_features.unwrap_or_default());
        features |= settings.features;
        if let Some(constrained) = &settings.constrained_limits {
            limits = limits.or_worse_values_from(constrained);
        }
        // Bevy eagerly compiles SSAO when this limit exceeds four, even without
        // an SSAO camera. Its depth-gather shader translates to an invalid GLSL
        // shadow-sampler call in wgpu 30. Our authored lighting doesn't use SSAO;
        // retain all other adapter limits and features (including compression).
        if info.backend == wgpu::Backend::Gl {
            assert!(
                compatible_color_target(&adapter),
                "OpenGL requires a blendable, filterable RGBA16 normalized target"
            );
            features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
            limits.max_storage_textures_per_shader_stage =
                limits.max_storage_textures_per_shader_stage.min(4);
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: settings.device_label.as_deref(),
                required_features: features,
                required_limits: limits,
                // Match Bevy's native renderer when a mixed backend mask
                // selects a native adapter with experimental features.
                experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
                memory_hints: settings.memory_hints.clone(),
                ..default()
            })
            .await
            .expect("Failed to create the render device");
        bevy::render::settings::RenderResources(
            RenderDevice::new(device),
            RenderQueue::new(queue),
            RenderAdapterInfo::new(info),
            RenderAdapter::new(adapter),
            RenderInstance::new(instance),
        )
    })
}
