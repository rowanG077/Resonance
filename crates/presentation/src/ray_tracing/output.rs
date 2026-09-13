//! Convert Solari's tonemapped linear color back to the encoded scene format
//! before the original dialogue/UI compositor and final display conversion.
use super::ModernView;
use bevy::{
    anti_alias::smaa::smaa,
    core_pipeline::{Core3dSystems, FullscreenShader, schedule::Core3d, tonemapping::tonemapping},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        render_resource::{binding_types::texture_2d, *},
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::ViewTarget,
    },
};
pub(super) fn install(app: &mut App) {
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, prepare)
        .add_systems(
            Core3d,
            encode
                .in_set(Core3dSystems::PostProcess)
                .after(tonemapping)
                // SMAA's luma detection expects encoded color. Anti-alias the
                // 3D scene here, before refraction and the separate dialogue UI.
                .before(smaa)
                .before(crate::field_refraction::RefractionPass),
        )
        .configure_sets(Core3d, crate::field_refraction::RefractionPass.after(smaa));
}
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedRenderPipelineId,
}
fn prepare(
    mut commands: Commands,
    server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "resonance/solari-output",
        &BindGroupLayoutEntries::single(
            ShaderStages::FRAGMENT,
            texture_2d(TextureSampleType::Float { filterable: false }),
        ),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("resonance/solari-output".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: server.load("embedded://resonance_presentation/ray_tracing_output.wgsl"),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(Pipeline { layout, id });
}
fn encode(
    view: ViewQuery<(&ViewTarget, &ModernView)>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut context: RenderContext,
) {
    let (target, _) = view.into_inner();
    let Some(gpu_pipeline) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    let post = target.post_process_write();
    let binding = device.create_bind_group(
        "resonance/solari-output",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::single(post.source),
    );
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("resonance/solari-output"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(default()),
                    store: StoreOp::Store,
                },
            })],
            ..default()
        });
    pass.set_pipeline(gpu_pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.draw(0..3, 0..1);
}
