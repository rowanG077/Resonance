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
        super::draw_order::DrawOrder((1 << 24) - 1, 0),
        MeshMaterial3d(materials.add(GlowMaterial { texture })),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
    ));
}

/// Render the particles emitted by SymphoniaScript. Their lifecycle belongs to
/// resonance-events; this module only builds standard billboard geometry.
pub(super) fn update(
    mut commands: Commands,
    events: Option<Res<Events>>,
    camera: Single<&Transform, With<FieldCamera>>,
    glow: Single<(Entity, Option<&Mesh3d>, &mut Visibility), With<GlowMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (entity, handle, mut visibility) = glow.into_inner();
    let Some(events) = events.filter(|events| !events.0.world.particles.is_empty()) else {
        // Bevy skips allocating empty meshes but still attempts their upload.
        // Keep the last nonempty buffer and hide it between particle bursts;
        // before the first burst there is no mesh asset to extract at all.
        *visibility = Visibility::Hidden;
        return;
    };
    let tick = events.0.tick();
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for particle in &events.0.world.particles {
        let (position, size, rgba) = particle.sample(tick);
        let center = Vec3::from_array(position);
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
        // Feather and reflection artwork occupy this atlas rectangle.
        uvs.extend([
            [192. / 256., 0.],
            [254. / 256., 0.],
            [254. / 256., 62. / 256.],
            [192. / 256., 62. / 256.],
        ]);
        colors.extend([rgba.map(|v| v / 255.); 4]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let geometry = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    if let Some(handle) = handle {
        *meshes.get_mut(&handle.0).expect("glow mesh exists") = geometry;
    } else {
        commands.entity(entity).insert(Mesh3d(meshes.add(geometry)));
    }
    *visibility = Visibility::Inherited;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn particle_bursts_never_publish_empty_meshes_or_leave_stale_glows_visible() {
        // An empty event registry followed by a terminating main script.
        let bytes: Vec<_> = [4_u16, 0, 0, 0, 0x20ff]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect();
        let events = resonance_events::EventRuntime::new(
            Arc::new(symphonia_script::Program::decode(&bytes).unwrap()),
            Arc::default(),
        )
        .unwrap();
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .insert_resource(Events(events))
            .add_systems(Update, update);
        app.world_mut().spawn((FieldCamera, Transform::default()));
        let entity = app
            .world_mut()
            .spawn((GlowMesh, Transform::default(), Visibility::Hidden))
            .id();
        app.update();
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert!(app.world().get::<Mesh3d>(entity).is_none());

        let particle = resonance_events::Particle {
            kind: 10,
            handle: 1,
            born: 0,
            lifetime: 100,
            position: [0.; 3],
            velocity: [0.; 3],
            size: 16.,
            size_delta: 0.,
            rgba: [255.; 4],
            alpha_delta: 0.,
            flutter: None,
        };
        app.world_mut()
            .resource_mut::<Events>()
            .0
            .world
            .particles
            .push(particle.clone());
        app.update();
        let handle = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
        let mesh = app.world().resource::<Assets<Mesh>>().get(&handle).unwrap();
        assert_eq!(mesh.count_vertices(), 4);
        assert_eq!(mesh.indices().unwrap().len(), 6);
        assert_eq!(
            app.world().get::<Visibility>(entity),
            Some(&Visibility::Inherited)
        );

        app.world_mut()
            .resource_mut::<Events>()
            .0
            .world
            .particles
            .clear();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().get::<Visibility>(entity),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&handle)
                .unwrap()
                .count_vertices(),
            4
        );

        app.world_mut()
            .resource_mut::<Events>()
            .0
            .world
            .particles
            .extend([particle.clone(), particle]);
        app.update();
        assert_eq!(
            app.world().get::<Mesh3d>(entity).unwrap().0.id(),
            handle.id()
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&handle)
                .unwrap()
                .count_vertices(),
            8
        );
        assert_eq!(
            app.world().get::<Visibility>(entity),
            Some(&Visibility::Inherited)
        );

        app.world_mut().remove_resource::<Events>();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(entity),
            Some(&Visibility::Hidden)
        );
    }
}
