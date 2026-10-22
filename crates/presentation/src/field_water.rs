//! Planar reflections share the field's animated meshes and use one small render target.
use super::{ActorPart, Art, State};
use crate::{
    draw_order::{DrawOrder, Layer},
    materials::TitleSurface,
};
use bevy::{
    camera::{
        RenderTarget,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    mesh::MeshVertexBufferLayoutRef,
    prelude::*,
    render::render_resource::{
        AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
        TextureFormat,
    },
    shader::ShaderRef,
};
use resonance_content::field::{FieldWater, MODEL_DRAW_SPAN};

const REFLECTION_LAYER: usize = 24;

pub(super) fn install(app: &mut App) {
    bevy::asset::embedded_asset!(app, "field_water.wgsl");
    app.add_plugins(MaterialPlugin::<Surface>::default())
        .add_systems(PostUpdate, camera_activity);
}

pub(super) struct Artwork {
    pub spec: FieldWater,
    draw: Option<Draw>,
}

struct Draw {
    camera: Entity,
    plane: Entity,
    background: Option<super::panorama::Draw>,
}

impl Artwork {
    pub fn new(spec: FieldWater) -> Self {
        Self { spec, draw: None }
    }

    pub fn despawn(&mut self, world: &mut World) {
        if let Some(draw) = self.draw.take() {
            world.despawn(draw.camera);
            world.despawn(draw.plane);
            if let Some(background) = draw.background {
                world.despawn(background.entity);
            }
        }
    }

    pub fn reflects(&self, actor: i32, controlled: i32) -> bool {
        actor == controlled || self.spec.reflected_actors.contains(&actor)
    }
}

#[derive(Component)]
pub(super) struct ReflectionCamera;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct Surface {
    #[texture(0)]
    #[sampler(1)]
    reflection: Handle<Image>,
    #[uniform(2)]
    tint: Vec4,
    #[uniform(3)]
    dimensions: Vec2,
}

impl Material for Surface {
    fn fragment_shader() -> ShaderRef {
        "embedded://resonance_presentation/field_water.wgsl".into()
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
            depth.depth_write_enabled = Some(true);
            depth.depth_compare = Some(CompareFunction::Greater);
        }
        Ok(())
    }
}

fn reflected_view(view: &Transform, height: f32) -> Transform {
    let mirror =
        Mat4::from_translation(Vec3::Z * (2. * height)) * Mat4::from_scale(Vec3::new(1., 1., -1.));
    // Flipping the capture horizontally restores ordinary triangle winding.
    // The water sampler reverses that flip when projecting the reflection.
    Transform::from_matrix(mirror * view.to_matrix() * Mat4::from_scale(Vec3::new(-1., 1., 1.)))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render(
    mut commands: Commands,
    mut art: ResMut<Art>,
    display: Res<crate::display::Display>,
    views: Query<
        (&Transform, &Projection, &Camera, Option<&DistanceFog>),
        (With<crate::FieldCamera>, Without<ReflectionCamera>),
    >,
    mut cameras: Query<(&mut Transform, &mut Projection), With<ReflectionCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Surface>>,
    mut surfaces: ResMut<Assets<TitleSurface>>,
) {
    if !art.ready {
        return;
    }
    let background = art.panorama.as_ref().and_then(|panorama| {
        let surface = surfaces.get(&panorama.draw.as_ref()?.material)?;
        Some((
            panorama.image.clone(),
            surface.uv_scales,
            surface.uv_offsets,
        ))
    });
    let Some(water) = &mut art.water else {
        return;
    };
    let Ok((view, projection, main, fog)) = views.single() else {
        return;
    };
    let size = display.0;
    let draw = water.draw.get_or_insert_with(|| {
        let mut image = Image::new_target_texture(
            (size.width / 2).max(1),
            (size.scene_height() / 2).max(1),
            TextureFormat::Bgra8Unorm,
            None,
        );
        image.sampler = ImageSampler::linear();
        let image = images.add(image);
        let material = materials.add(Surface {
            reflection: image.clone(),
            tint: Vec4::from_array(water.spec.tint),
            dimensions: Vec2::new(size.width as f32, size.scene_height() as f32),
        });
        let [min, max] = water.spec.bounds.map(Vec2::from_array);
        let mesh = meshes.add(Rectangle::from_size(max - min));
        let plane = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(((min + max) * 0.5).extend(water.spec.height)),
                NoFrustumCulling,
                DrawOrder(Layer::ModelEffects(2 * MODEL_DRAW_SPAN - 1), 0, 0),
            ))
            .id();
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: main.order - 1,
                    is_active: main.is_active,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                RenderLayers::layer(REFLECTION_LAYER),
                ReflectionCamera,
                Tonemapping::None,
                Msaa::Off,
                reflected_view(view, water.spec.height),
                projection.clone(),
            ))
            .id();
        let background = background.as_ref().map(|(image, _, _)| {
            super::panorama::Draw::spawn(
                image.clone(),
                RenderLayers::layer(REFLECTION_LAYER),
                &mut commands,
                &mut meshes,
                &mut surfaces,
            )
        });
        Draw {
            camera,
            plane,
            background,
        }
    });
    if let Ok((mut transform, mut camera_projection)) = cameras.get_mut(draw.camera) {
        *transform = reflected_view(view, water.spec.height);
        *camera_projection = projection.clone();
    }
    if let Some(fog) = fog {
        commands.entity(draw.camera).insert(fog.clone());
    }
    if let (Some(draw), Some((_, scales, offsets))) = (&mut draw.background, background) {
        // The reflection retains the upper-left quarter of the scene backdrop.
        // Reverse its horizontal span along with the reflected geometry.
        draw.update(
            reflected_view(view, water.spec.height).to_matrix()
                * projection.get_clip_from_view().inverse(),
            scales * Vec4::new(-0.5, 0.5, 1., 1.),
            offsets + Vec4::new(scales.x * 0.5, 0., 0., 0.),
            &mut meshes,
            &mut surfaces,
        );
    }
}

fn camera_activity(
    main: Query<
        (&Camera, Option<&RenderLayers>),
        (With<crate::FieldCamera>, Without<ReflectionCamera>),
    >,
    mut reflections: Query<&mut Camera, With<ReflectionCamera>>,
) {
    let active = main.single().is_ok_and(|(camera, layers)| {
        camera.is_active && layers.is_none_or(|layers| layers.intersects(&RenderLayers::layer(0)))
    });
    for mut camera in &mut reflections {
        camera.is_active = active;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_preserves_water_coordinates_and_triangle_winding() {
        let view =
            Transform::from_xyz(300., -900., 700.).looking_at(Vec3::new(100., 50., -70.), Vec3::Z);
        let reflected = reflected_view(&view, -70.);
        assert!((reflected.to_matrix().determinant() - 1.).abs() < 0.0001);
        let camera = view.to_matrix().inverse();
        let mirrored_camera = reflected.to_matrix().inverse();
        for point in [Vec3::new(100., 50., -70.), Vec3::new(-300., 400., -70.)] {
            let original = camera.transform_point3(point);
            let mirrored = mirrored_camera.transform_point3(point);
            assert!((original - mirrored * Vec3::new(-1., 1., 1.)).length() < 0.001);
        }
        let actor = Vec3::new(100., 50., 20.);
        let below = Vec3::new(actor.x, actor.y, -140. - actor.z);
        assert!(
            (mirrored_camera.transform_point3(actor) * Vec3::new(-1., 1., 1.)
                - camera.transform_point3(below))
            .length()
                < 0.001
        );
    }
}

pub(super) fn layers(
    mut commands: Commands,
    art: Res<Art>,
    state: State,
    actors: Query<&ActorPart>,
    mut layers: Query<(Option<&RenderLayers>, &mut Visibility), With<MeshMaterial3d<TitleSurface>>>,
) {
    let Some(water) = &art.water else {
        return;
    };
    for actor in &actors {
        let reflection_only = water.spec.reflection_only_actors.contains(&actor.actor);
        let desired = if reflection_only {
            RenderLayers::layer(REFLECTION_LAYER)
        } else if water.reflects(actor.actor, state.get().events.world.controlled_actor) {
            RenderLayers::from_layers(&[0, REFLECTION_LAYER])
        } else {
            RenderLayers::layer(0)
        };
        for &(_, entity) in &actor.geometry {
            if let Ok((current, mut visibility)) = layers.get_mut(entity) {
                if reflection_only {
                    *visibility = Visibility::Inherited;
                }
                if current == Some(&desired) {
                    continue;
                }
            }
            commands.entity(entity).insert(desired.clone());
        }
    }
}
