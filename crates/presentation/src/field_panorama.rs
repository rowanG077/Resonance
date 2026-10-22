//! A field's scrolling background, drawn behind its scenery without depth writes.
use super::{Art, State};
use crate::{
    draw_order::{DrawOrder, Layer},
    loading::LoadTasks,
    materials::TitleSurface,
};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
};

pub(crate) struct Artwork {
    pub image: Handle<Image>,
    pub draw: Option<Draw>,
}

pub(crate) struct Draw {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub material: Handle<TitleSurface>,
    from_clip: Option<Mat4>,
}

impl Artwork {
    pub fn load(path: &str, server: &AssetServer, loads: &LoadTasks) -> Self {
        Self {
            image: server
                .load_builder()
                .with_guard(loads.ticket())
                .with_settings(|settings: &mut ImageLoaderSettings| {
                    settings.is_srgb = false;
                    settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    });
                })
                .load(path.to_owned()),
            draw: None,
        }
    }

    pub fn despawn(&mut self, world: &mut World) {
        if let Some(draw) = self.draw.take() {
            world.despawn(draw.entity);
        }
    }
}

impl Draw {
    pub fn spawn(
        image: Handle<Image>,
        layers: RenderLayers,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<TitleSurface>,
    ) -> Self {
        let mesh = meshes.add(Rectangle::new(2., 2.));
        let material = materials.add(TitleSurface {
            depth_test: false,
            depth_write: false,
            vertex_color: false,
            ..TitleSurface::textured(Some(image))
        });
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                DrawOrder(Layer::Background, 0, 0),
                NoFrustumCulling,
                layers,
            ))
            .id();
        Self {
            entity,
            mesh,
            material,
            from_clip: None,
        }
    }

    pub fn update(
        &mut self,
        from_clip: Mat4,
        scales: Vec4,
        offsets: Vec4,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<TitleSurface>,
    ) {
        // Project a screen rectangle back into the view, including camera shake
        // and raster alignment, without exposing a strip at the edges.
        if self.from_clip != Some(from_clip) {
            let corners = [
                Vec3::new(1., 1., 0.5),
                Vec3::new(-1., 1., 0.5),
                Vec3::new(-1., -1., 0.5),
                Vec3::new(1., -1., 0.5),
            ];
            meshes.get_mut(&self.mesh).unwrap().insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                corners
                    .map(|p| from_clip.project_point3(p).to_array())
                    .to_vec(),
            );
            self.from_clip = Some(from_clip);
        }
        let surface = materials.get(&self.material).unwrap();
        if surface.uv_scales != scales || surface.uv_offsets != offsets {
            let mut surface = materials.get_mut(&self.material).unwrap();
            surface.uv_scales = scales;
            surface.uv_offsets = offsets;
        }
    }
}

pub(super) fn render(
    mut commands: Commands,
    state: State,
    mut art: ResMut<Art>,
    views: Query<(&Transform, &Projection), With<crate::FieldCamera>>,
    display: Res<crate::display::Display>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TitleSurface>>,
) {
    let Some(background) = art.panorama.as_mut() else {
        return;
    };
    let Some(camera) = &state.get().events.world.field_camera else {
        return;
    };
    let Ok((view, projection)) = views.single() else {
        return;
    };
    let draw = background.draw.get_or_insert_with(|| {
        Draw::spawn(
            background.image.clone(),
            RenderLayers::layer(0),
            &mut commands,
            &mut meshes,
            &mut materials,
        )
    });
    let from_clip = view.to_matrix() * projection.get_clip_from_view().inverse();
    let horizontal_span = display.0.aspect() / (4. / 3.);
    let vertical = -camera.angles[0] / 90.;
    let vertical = if vertical < -0.6 {
        0.
    } else {
        vertical.max(-0.5)
    };
    let scales = Vec4::new(horizontal_span, 0.5, 1., 1.);
    let offsets = Vec4::new(
        -camera.angles[2] / 90. + (1. - horizontal_span) * 0.5,
        0.5 + vertical,
        0.,
        0.,
    );
    draw.update(from_clip, scales, offsets, &mut meshes, &mut materials);
}
