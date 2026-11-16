//! Backend selection, scheduling and draw configuration for Resonance's authored shaders.
mod backend;
mod transforms;
pub(crate) use transforms::{SerialTransformPlugin, Stats as TransformStats};
mod uploads;
pub(super) use backend::plugin;
use bevy::{
    ecs::schedule::{ScheduleLabel, SingleThreadedExecutor},
    prelude::*,
    render::{
        Render, RenderApp,
        renderer::{RenderGraph, RenderGraphSystems},
    },
};
/// Uploads the game makes for the native frame, after meshes and before the
/// frame is drawn.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FrameUploads;

pub(crate) use uploads::{Uploads, install as install_uploads};
/// Default plugins with the given device settings.
pub fn render_plugins(render: bevy::render::RenderPlugin) -> bevy::app::PluginGroupBuilder {
    default_plugins().set(render)
}

/// Shared by interactive rendering and offscreen field captures.
pub(crate) fn default_plugins() -> bevy::app::PluginGroupBuilder {
    let mut pools = bevy::app::TaskPoolOptions::default();
    // Parallel queries and encoder finishing block the frame's own threads.
    pools.compute.on_thread_spawn = Some(std::sync::Arc::new(|| {
        crate::performance::affinity::pin_current_thread();
    }));
    DefaultPlugins
        .build()
        .set(bevy::app::TaskPoolPlugin {
            task_pool_options: pools,
        })
        .add(crate::texture::TexturePlugin)
        .add_before::<bevy::transform::TransformPlugin>(SerialTransformPlugin)
        .disable::<bevy::transform::TransformPlugin>()
        // The native frame renderer draws every camera; Bevy's camera render
        // graphs are not installed.
        .disable::<bevy::core_pipeline::CorePipelinePlugin>()
        .add(FramePlugin)
}

/// What the native frame renderer uses of Bevy's core pipeline: the
/// fullscreen triangle effects import, the blit pipeline SMAA writes its output
/// with, and submission of the frame's command buffers.
struct FramePlugin;

impl Plugin for FramePlugin {
    fn build(&self, app: &mut App) {
        // Registered under Bevy's path, as its core pipeline does, so its
        // import and `FullscreenShader` resolve.
        let path = std::path::Path::new("bevy_core_pipeline/fullscreen_vertex_shader.wesl");
        app.world()
            .resource::<bevy::asset::io::embedded::EmbeddedAssetRegistry>()
            .insert_asset(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/fullscreen_vertex_shader.wesl"
                )
                .into(),
                path,
                include_bytes!("fullscreen_vertex_shader.wesl").as_slice(),
            );
        let shader: Handle<bevy::shader::Shader> = app
            .world()
            .resource::<AssetServer>()
            .load(bevy::asset::AssetPath::from_path(path).with_source("embedded"));
        app.add_plugins(bevy::core_pipeline::blit::BlitPlugin)
            // Cameras have no render graph of Bevy's to run; the native frame
            // draws them. The label only satisfies Bevy's camera hook.
            .register_required_components_with::<Camera, bevy::render::camera::CameraRenderGraph>(
                || bevy::render::camera::CameraRenderGraph::new(NativeFrame),
            );
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render
            .insert_resource(FullscreenShaderLibrary(shader))
            .init_resource::<bevy::core_pipeline::FullscreenShader>()
            .add_systems(
                RenderGraph,
                submit_commands.in_set(RenderGraphSystems::Submit),
            );
    }

    fn finish(&self, app: &mut App) {
        use bevy::{
            camera::visibility::VisibilitySystems,
            ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
            render::{ExtractSchedule, MainWorld, camera::extract_cameras},
        };
        // Native rendering owns per-view visibility and extraction. Keep Bevy's
        // bounds, frusta and inherited visibility, and leave logical camera
        // activity available to scene systems throughout the frame.
        for set in [
            VisibilitySystems::CheckVisibility,
            VisibilitySystems::MarkNewlyHiddenEntitiesInvisible,
        ] {
            app.remove_systems_in_set(PostUpdate, set, RemoveSystemsOnly)
                .expect("native renderer replaces Bevy view visibility");
        }
        if app.get_sub_app(RenderApp).is_some() {
            // Removing systems initializes the extraction schedule. Its query
            // states must bind to the real main world, just as during extraction.
            let mut main = MainWorld::default();
            std::mem::swap(&mut *main, app.world_mut());
            let render = app.sub_app_mut(RenderApp);
            render.insert_resource(main);
            let removed =
                render.remove_systems_in_set(ExtractSchedule, extract_cameras, RemoveSystemsOnly);
            let mut main = render.world_mut().remove_resource::<MainWorld>().unwrap();
            std::mem::swap(&mut *main, app.world_mut());
            removed.expect("native renderer replaces Bevy camera extraction");
        }
    }
}

/// The render graph label of every camera; nothing runs it.
#[derive(bevy::ecs::schedule::ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct NativeFrame;

/// Keeps the fullscreen shader library loaded for the imports that use it.
#[derive(Resource)]
struct FullscreenShaderLibrary(#[allow(dead_code)] Handle<bevy::shader::Shader>);

/// Submit the frame's recorded command buffers, as Bevy's core pipeline does.
/// Exclusive, so the render systems' deferred command buffers are applied
/// first, before the windows they draw are presented.
fn submit_commands(world: &mut World) {
    let queue = world
        .resource::<bevy::render::renderer::RenderQueue>()
        .clone();
    let mut pending = world.resource_mut::<bevy::render::renderer::PendingCommandBuffers>();
    let mut buffers = pending.finish().peekable();
    if buffers.peek().is_some() {
        queue.submit(buffers);
    }
}

/// The scene image's format, which effects and warmed pipelines render to.
pub(crate) const SCENE_COLOR_FORMAT: bevy::render::render_resource::TextureFormat =
    bevy::render::render_resource::TextureFormat::Bgra8Unorm;

pub(super) fn configure(app: &mut App) {
    app.insert_resource(bevy::render::error_handler::RenderErrorHandler(
        |error, main_world, render_world| {
            let adapter = render_world
                .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
                .map(|adapter| &**adapter);
            crate::diagnostics::fatal(
                main_world,
                "graphics",
                anyhow::anyhow!("{error:?}; adapter={adapter:?}"),
            );
            bevy::render::error_handler::RenderErrorPolicy::StopRendering
        },
    ));
    app.add_plugins(crate::anti_aliasing::AntiAliasingPlugin);
    // These short, dependent schedules spend more time handing work between
    // workers than they gain from it. The render app still runs separately
    // from the main app; asset workers and parallel mesh processing remain.
    for label in [
        First.intern(),
        PreUpdate.intern(),
        FixedFirst.intern(),
        FixedPreUpdate.intern(),
        FixedUpdate.intern(),
        FixedPostUpdate.intern(),
        FixedLast.intern(),
        Update.intern(),
        PostUpdate.intern(),
        Last.intern(),
    ] {
        app.edit_schedule(label, |schedule| {
            schedule.set_executor(SingleThreadedExecutor::new());
        });
    }
    let render = app.sub_app_mut(RenderApp);
    render.configure_sets(
        Render,
        FrameUploads
            .after(bevy::render::RenderSystems::PrepareMeshes)
            .before(bevy::render::RenderSystems::Prepare),
    );
    // A field upload must not grow and copy the large resident boot buffers.
    render
        .world_mut()
        .resource_mut::<bevy::render::mesh::allocator::MeshAllocatorSettings>()
        .max_slab_size = 16 * 1024 * 1024;
    for label in [
        // Extraction is several hundred tiny systems; dispatching them to
        // workers costs more in wake-ups than the systems themselves.
        bevy::render::ExtractSchedule.intern(),
        Render.intern(),
        RenderGraph.intern(),
    ] {
        render
            .get_schedule_mut(label)
            .unwrap()
            .set_executor(SingleThreadedExecutor::new());
    }
    // These run on the threads that execute the main and render schedules.
    app.add_systems(First, pin_frame_thread);
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        pin_frame_thread.before(bevy::render::RenderSystems::ExtractCommands),
    );
}

fn pin_frame_thread(_: bevy::ecs::system::NonSendMarker) {
    crate::performance::affinity::pin_current_thread();
}
