//! Backend selection, scheduling and draw configuration for Resonance's authored shaders.
mod backend;
pub(super) use backend::plugin;
use bevy::{
    camera::CameraOutputMode,
    core_pipeline::schedule::{Core2d, Core3d},
    ecs::schedule::{ScheduleLabel, SingleThreadedExecutor},
    light::cluster::{ClusterConfig, GlobalClusterSettings},
    prelude::*,
    render::{
        Render, RenderApp, render_resource::BlendState, renderer::RenderGraph,
        view::NoIndirectDrawing,
    },
};

// Later cameras default to alpha blending. A completed scene must be copied
// without applying its retained transparency a second time.
pub(super) const COMPOSITED_OUTPUT: CameraOutputMode = CameraOutputMode::Write {
    blend_state: Some(BlendState::REPLACE),
    clear_color: ClearColorConfig::Default,
};

pub(super) fn configure(app: &mut App) {
    app.add_plugins(crate::anti_aliasing::AntiAliasingPlugin);
    // Our modest, mostly transparent scenes cost less with direct draws than
    // GPU indirect culling. Keep GPU mesh preparation and visibility checks.
    // Require this at spawn: changing a warm-up camera's mode after its first
    // extraction can leave Bevy's cached preprocessing bind group incompatible
    // with the newly selected compute pipeline.
    app.register_required_components::<Camera3d, NoIndirectDrawing>();
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
    for label in [
        Render.intern(),
        RenderGraph.intern(),
        Core2d.intern(),
        Core3d.intern(),
    ] {
        render
            .get_schedule_mut(label)
            .unwrap()
            .set_executor(SingleThreadedExecutor::new());
    }
    app.add_systems(Startup, unlit_settings)
        .add_systems(PostStartup, scene_output)
        .add_systems(Update, unlit_views);
}

fn scene_output(
    mut cameras: Query<(&mut Camera, Has<crate::FieldCamera>), crate::FieldCameraFilter>,
) {
    // The UI camera shares the scene's main textures and publishes the complete
    // composition afterward. Copy it without blending its retained alpha again.
    for (mut camera, is_scene) in &mut cameras {
        camera.output_mode = if is_scene {
            CameraOutputMode::Skip
        } else {
            COMPOSITED_OUTPUT
        };
    }
}

fn unlit_settings(mut settings: ResMut<GlobalClusterSettings>) {
    // Lighting comes from the cooked ramps and native light parameters. The
    // GPU cluster passes are unused. Set this after PBR plugin initialization;
    // ClusterConfig::None alone is not supported by Bevy 0.19's GPU path.
    settings.gpu_clustering = None;
}

fn unlit_views(mut commands: Commands, views: Query<(Entity, &Projection), Added<Camera3d>>) {
    for (entity, projection) in &views {
        if let Projection::Custom(projection) = projection
            && projection.get::<crate::camera::TitleProjection>().is_some()
        {
            commands.entity(entity).insert(ClusterConfig::None);
        }
    }
}
