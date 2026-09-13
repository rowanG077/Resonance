//! Scheduling and lighting configuration for Resonance's authored shaders.
mod process;
use bevy::{
    ecs::schedule::SingleThreadedExecutor,
    light::cluster::{ClusterConfig, GlobalClusterSettings},
    prelude::*,
    render::{Render, RenderApp, renderer::RenderGraph},
};
pub use process::prepare_ray_tracing_process;

pub(super) fn plugin() -> bevy::render::RenderPlugin {
    use bevy::render::settings::{WgpuFeatures, WgpuSettings};
    bevy::render::RenderPlugin {
        render_creation: WgpuSettings {
            // wgpu 29 detects cooperative matrices from advertised properties,
            // but Lavapipe/Mesa 26.2 rejects enabling that device feature. It is
            // unused by our shaders and Solari; ray queries remain enabled.
            disabled_features: Some(WgpuFeatures::EXPERIMENTAL_COOPERATIVE_MATRIX),
            ..default()
        }
        .into(),
        ..default()
    }
}

pub(super) fn configure(app: &mut App) {
    // These small render schedules spend more time handing work between
    // workers than they gain from it. The render app still runs separately
    // from the main app; asset workers and parallel mesh processing remain.
    let render = app.sub_app_mut(RenderApp);
    render
        .get_schedule_mut(Render)
        .unwrap()
        .set_executor(SingleThreadedExecutor::new());
    render
        .get_schedule_mut(RenderGraph)
        .unwrap()
        .set_executor(SingleThreadedExecutor::new());
    app.add_systems(Startup, unlit_settings)
        .add_systems(Update, unlit_views);
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
