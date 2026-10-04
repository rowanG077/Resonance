//! Meshes whose texture is the completed field image, before UI composition.
use super::{
    field_audit::{Applied, Request},
    field_view::State,
};
use bevy::{
    mesh::{VertexAttributeValues, VertexBufferLayout},
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::*,
    },
};
use std::collections::HashMap;

#[derive(Component)]
pub(super) struct Part {
    pub request: Request,
    pub particle: i32,
    pub vertex_color: bool,
}

#[derive(Component, Clone, Default, ExtractComponent)]
pub(super) struct Meshes(pub Vec<f32>);

const VERTEX_FLOATS: u64 = 9;

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "field_capture.wgsl");
    app.add_plugins(ExtractComponentPlugin::<Meshes>::default())
        .add_systems(
            PostUpdate,
            sync.after(bevy::transform::TransformSystems::Propagate)
                .after(super::field_model_particles::sync)
                .before(super::field_audit::check)
                .run_if(resource_exists::<super::field_view::Art>),
        );
}

struct Vertex {
    position: Vec3,
    uv: Vec2,
    color: Vec4,
}
fn vertices(mesh: &Mesh) -> anyhow::Result<Vec<Vertex>> {
    use anyhow::{Context, ensure};
    ensure!(
        mesh.primitive_topology() == PrimitiveTopology::TriangleList,
        "capture requires triangles"
    );
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(VertexAttributeValues::as_float3)
        .context("capture positions")?;
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        anyhow::bail!("capture UVs are missing");
    };
    let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colors)) => Some(colors),
        None => None,
        _ => anyhow::bail!("unsupported capture colors"),
    };
    ensure!(
        positions.len() == uvs.len() && colors.is_none_or(|c| c.len() == positions.len()),
        "capture attributes differ in length"
    );
    let indices: Vec<_> = mesh
        .indices()
        .map_or_else(|| (0..positions.len()).collect(), |i| i.iter().collect());
    ensure!(
        indices.len().is_multiple_of(3) && indices.iter().all(|i| *i < positions.len()),
        "invalid capture triangles"
    );
    Ok(indices
        .into_iter()
        .map(|i| Vertex {
            position: Vec3::from_array(positions[i]),
            uv: Vec2::from_array(uvs[i]),
            color: colors.map_or(Vec4::ONE, |c| Vec4::from_array(c[i])),
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    state: State,
    assets: Res<Assets<Mesh>>,
    parts: Query<(&Part, &Mesh3d, &GlobalTransform)>,
    views: Query<Entity, With<super::FieldCamera>>,
    ready: Res<super::field_refraction::Ready>,
    mut applied: ResMut<Applied>,
    mut cached: Local<HashMap<AssetId<Mesh>, Vec<Vertex>>>,
) {
    let world = &state.get().events.world;
    cached.retain(|id, _| assets.contains(*id));
    let mut output = Meshes::default();
    for (part, mesh, transform) in &parts {
        let Some(particle) = world.model_particles.get(&part.particle) else {
            continue;
        };
        let Some(source) = assets.get(&mesh.0) else {
            applied.loading(part.request.clone());
            continue;
        };
        let vertices = cached
            .entry(mesh.0.id())
            .or_insert_with(|| vertices(source).expect("validated capture mesh"));
        let tint = Vec4::from_array(particle.rgba.map(|v| f32::from(v) / 255.))
            * Vec4::new(4., 4., 4., 1.);
        for vertex in vertices {
            output
                .0
                .extend(transform.transform_point(vertex.position).to_array());
            output.0.extend(vertex.uv.to_array());
            output.0.extend(
                (tint
                    * if part.vertex_color {
                        vertex.color
                    } else {
                        Vec4::ONE
                    })
                .to_array(),
            );
        }
        if ready.get() && !views.is_empty() {
            applied.ack(part.request.clone());
        } else {
            applied.loading(part.request.clone());
        }
    }
    for view in &views {
        commands.entity(view).insert(output.clone());
    }
}

pub(super) fn pipeline(
    layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("resonance/captured-mesh".into()),
        layout: vec![layout],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: VERTEX_FLOATS * 4,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![
                    VertexAttribute {
                        format: VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x2,
                        offset: 12,
                        shader_location: 1,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x4,
                        offset: 20,
                        shader_location: 2,
                    },
                ],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Bgra8Unorm,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        primitive: PrimitiveState {
            cull_mode: Some(Face::Back),
            ..default()
        },
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            // Captured meshes test scene depth without occluding later transparent layers.
            depth_compare: Some(CompareFunction::Greater),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    }
}

pub(super) fn vertex_count(meshes: &Meshes) -> u32 {
    (meshes.0.len() as u64 / VERTEX_FLOATS) as u32
}
