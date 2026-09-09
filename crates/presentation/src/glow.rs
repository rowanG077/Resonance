//! Native billboards for the title event's feather and reflection trails.
use super::{Art, Events, FieldCamera};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    image::ImageLoaderSettings,
    mesh::{Indices, MeshVertexBufferLayoutRef},
    prelude::*,
    render::render_resource::*,
    shader::ShaderRef,
};

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct GlowMaterial {
    #[texture(0)]
    #[sampler(1)]
    texture: Handle<Image>,
}
impl Material for GlowMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/title_glow.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn specialize(
        _: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(CompareFunction::Greater);
        }
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent::OVER,
                });
            }
        }
        Ok(())
    }
}

#[derive(Component)]
pub(super) struct GlowMesh;

pub(super) fn setup(
    mut commands: Commands,
    art: Res<Art>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GlowMaterial>>,
) {
    let Some(scene) = &art.manifest.scene else {
        return;
    };
    let texture = server
        .load_builder()
        .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
        .load(scene.glow.texture.clone());
    commands.spawn((
        GlowMesh,
        // Draw scene effects after models and lights.
        super::draw_order::DrawOrder((1 << 24) - 1),
        Mesh3d(meshes.add(Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        ))),
        MeshMaterial3d(materials.add(GlowMaterial { texture })),
        Transform::default(),
        NoFrustumCulling,
    ));
}

/// Render the particles emitted by SymphoniaScript. Their lifecycle belongs to
/// resonance-events; this module only builds standard billboard geometry.
pub(super) fn update(
    events: Option<Res<Events>>,
    camera: Single<&Transform, With<FieldCamera>>,
    mesh: Single<&Mesh3d, With<GlowMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(events) = events else {
        return;
    };
    let tick = events.0.tick();
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut emit =
        |point: [f32; 3], velocity: Vec3, size: f32, rgb: [f32; 3], alpha: f32, age: u32| {
            let center = Vec3::from_array(point) + velocity * age as f32;
            let half = (size * 0.5).trunc();
            let right = camera.right() * half;
            let up = camera.up() * half;
            let base = positions.len() as u32;
            for point in [
                center - right + up,
                center + right + up,
                center + right - up,
                center - right - up,
            ] {
                positions.push(point.to_array());
            }
            // Effect table 0x2A4: origin (192,0), span (62,62) in a 256 atlas.
            uvs.extend([
                [192. / 256., 0.],
                [254. / 256., 0.],
                [254. / 256., 62. / 256.],
                [192. / 256., 62. / 256.],
            ]);
            colors.extend([[rgb[0] / 255., rgb[1] / 255., rgb[2] / 255., alpha / 255.]; 4]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        };
    for particle in &events.0.world.particles {
        debug_assert_eq!(
            particle.kind, 10,
            "Unimplemented title particle {} (kind {}) at VM tick {tick}",
            particle.handle, particle.kind
        );
        let age = tick - particle.born;
        emit(
            particle.position,
            Vec3::from_array(particle.velocity),
            particle.size + particle.size_delta * age as f32,
            [particle.rgba[0], particle.rgba[1], particle.rgba[2]],
            particle.alpha(tick).max(0.),
            age,
        );
    }
    let mut mesh = meshes.get_mut(&mesh.0).expect("glow mesh exists");
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}
