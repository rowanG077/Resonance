//! Native billboards for the title event's feather and reflection trails.
use super::{Art, Events, FieldCamera, field_effects::Quad};
use bevy::{
    camera::visibility::NoFrustumCulling, image::ImageLoaderSettings,
    mesh::MeshVertexBufferLayoutRef, prelude::*, render::render_resource::*, shader::ShaderRef,
};

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct GlowMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(super) texture: Handle<Image>,
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
        super::draw_order::DrawOrder(super::draw_order::Layer::Overlay, 0, 0),
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
    let Some(events) = events.filter(|events| !events.0.world.billboards.is_empty()) else {
        // Bevy skips allocating empty meshes but still attempts their upload.
        // Keep the last nonempty buffer and hide it between particle bursts;
        // before the first burst there is no mesh asset to extract at all.
        *visibility = Visibility::Hidden;
        return;
    };
    let tick = events.0.tick();
    let quads: Vec<_> = events
        .0
        .world
        .billboards
        .values()
        .map(|particle| {
            let mut color = particle.rgba.map(|v| f32::from(v) / 255.);
            color[3] = particle.alpha(tick).max(0.) / 255.;
            Quad::new(
                Vec3::from_array(particle.position),
                camera.rotation,
                particle.size,
                [192., 0., 254., 62.].map(|v| v / 256.),
                color,
                particle.anchor,
            )
        })
        .collect();
    let geometry = Quad::mesh(&quads);
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
        let mut particle = resonance_events::effect::BillboardEffect::default();
        particle.lifetime = 100;
        particle.size = [16.; 2];
        particle.rgba = [255; 4];
        for count in [0, 1, 0, 2] {
            app.world_mut().resource_mut::<Events>().0.world.billboards =
                (0..count).map(|id| (id, particle.clone())).collect();
            app.update();
            let expected = if count == 0 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            assert_eq!(app.world().get::<Visibility>(entity), Some(&expected));
            let meshes = app.world().resource::<Assets<Mesh>>();
            assert!(meshes.iter().all(|(_, mesh)| mesh.count_vertices() > 0));
            if count > 0 {
                let handle = app.world().get::<Mesh3d>(entity).unwrap();
                assert_eq!(
                    meshes.get(&handle.0).unwrap().count_vertices(),
                    count as usize * 4
                );
            }
        }

        app.world_mut().remove_resource::<Events>();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(entity),
            Some(&Visibility::Hidden)
        );
    }
}
