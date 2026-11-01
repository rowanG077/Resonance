//! High-level startup logos, rendered from cooked textures.
use super::*;
use resonance_content::BootAssets;
use resonance_game::boot::Logos;

#[derive(Resource, Default)]
pub(super) struct Playback {
    pub logos: Option<Logos>,
    images: Vec<Handle<Image>>,
}

impl Playback {
    pub fn from_assets(asset: Option<BootAssets>) -> Result<Self> {
        Ok(Self {
            logos: asset.map(Logos::new).transpose()?,
            images: Vec::new(),
        })
    }

    pub fn duration(&self) -> u32 {
        self.logos.as_ref().map_or(0, Logos::duration)
    }

    pub fn active(&self) -> bool {
        self.logos.as_ref().is_some_and(Logos::active)
    }

    pub fn ready(&self, server: &AssetServer) -> bool {
        self.logos.as_ref().is_none_or(|logos| {
            self.images.len() == logos.sequence().0.len()
                && self
                    .images
                    .iter()
                    .all(|h| server.is_loaded_with_dependencies(h.id()))
        })
    }
}

#[derive(Component)]
pub(super) struct BootCamera;
#[derive(Component)]
pub(super) struct LogoQuad;

pub(super) fn setup(
    commands: &mut Commands,
    server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<TitleText>,
    boot: &mut Playback,
    source: Handle<Image>,
) {
    let Some(logos) = &boot.logos else {
        return;
    };
    if !logos.active() {
        return;
    }
    boot.images = logos
        .sequence()
        .0
        .iter()
        .map(|logo| {
            server
                .load_builder()
                .with_settings(|s: &mut ImageLoaderSettings| s.is_srgb = false)
                .load(logo.image.clone())
        })
        .collect();
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(1., 1.))),
        MeshMaterial2d(materials.add(TitleText {
            source: boot.images[0].clone(),
            opacity_pulse: Vec4::ZERO,
        })),
        LogoQuad,
        RenderLayers::layer(4),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::Linear,
        Msaa::Off,
        BootCamera,
        RenderLayers::layer(4),
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        camera::overlay_alignment(),
        RenderTarget::Image(source.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

pub(super) fn advance(
    mut boot: ResMut<Playback>,
    ready: Res<timing::Ready>,
    mut input: ResMut<PendingInput>,
    scenario: Option<ResMut<saves::ScenarioInput>>,
) {
    if !ready.0 {
        return;
    }
    if let Some(logos) = &mut boot.logos
        && logos.active()
    {
        if let Some(mut scenario) = scenario {
            scenario.acknowledge_input();
        }
        logos.step(std::mem::take(&mut input.pressed).accept);
    }
}

pub(super) fn update(
    boot: Res<Playback>,
    mut cameras: Query<&mut Camera, With<BootCamera>>,
    mut quads: Query<(&MeshMaterial2d<TitleText>, &mut Transform), With<LogoQuad>>,
    mut materials: ResMut<Assets<TitleText>>,
    images: Res<Assets<Image>>,
) {
    for mut camera in &mut cameras {
        camera.is_active = boot.active();
    }
    let Some(logos) = &boot.logos else {
        return;
    };
    if !logos.active() {
        return;
    }
    let logo = &logos.sequence().0[logos.texture];
    let source = &boot.images[logos.texture];
    let Some(image) = images.get(source) else {
        return;
    };
    let alpha = f32::from(logos.alpha) / 255.;
    for mut camera in &mut cameras {
        let rgb = logo.background.map(|v| f32::from(v) / 255. * alpha);
        camera.clear_color = ClearColorConfig::Custom(Color::linear_rgb(rgb[0], rgb[1], rgb[2]));
    }
    for (handle, mut transform) in &mut quads {
        let mut material = materials.get_mut(&handle.0).unwrap();
        let opacity = Vec4::new(alpha, 0., 0., 0.);
        if material.source != *source || material.opacity_pulse != opacity {
            material.source = source.clone();
            material.opacity_pulse = opacity;
        }
        // Use symmetric integer half-extents: an odd width such as 379 draws as 378.
        transform.scale = Vec3::new(
            (image.width() & !1) as f32,
            (image.height() & !1) as f32,
            1.,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        ecs::system::RunSystemOnce,
        render::render_resource::{Extent3d, TextureDimension},
    };

    #[test]
    fn authored_logo_renders_its_image_size_background_and_empty_sequence() -> Result<()> {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<Assets<TitleText>>();
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_fill(
                Extent3d {
                    width: 7,
                    height: 5,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[255; 4],
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::default(),
            ));
        let sequence = serde_json::from_value(serde_json::json!([
            {"image":"custom.ktx2","fade_out_at":4,"fade_ticks":2,"background":[255,0,0]}]))?;
        let mut playback = Playback::from_assets(Some(sequence))?;
        assert_eq!(playback.duration(), 6);
        playback.images.push(image.clone());
        playback.logos.as_mut().unwrap().step(false);
        app.insert_resource(playback);
        let material = app
            .world_mut()
            .resource_mut::<Assets<TitleText>>()
            .add(TitleText {
                source: Handle::default(),
                opacity_pulse: Vec4::ZERO,
            });
        let quad = app
            .world_mut()
            .spawn((
                LogoQuad,
                MeshMaterial2d(material.clone()),
                Transform::default(),
            ))
            .id();
        let camera = app.world_mut().spawn((BootCamera, Camera::default())).id();
        app.world_mut().run_system_once(update).unwrap();
        assert_eq!(
            app.world().get::<Transform>(quad).unwrap().scale,
            Vec3::new(6., 4., 1.)
        );
        let drawn = app
            .world()
            .resource::<Assets<TitleText>>()
            .get(&material)
            .unwrap();
        assert_eq!(drawn.source, image);
        assert_eq!(drawn.opacity_pulse.x, 128. / 255.);
        let camera_value = app.world().get::<Camera>(camera).unwrap();
        assert!(camera_value.is_active);
        let ClearColorConfig::Custom(background) = camera_value.clear_color else {
            panic!("logo did not set its authored background");
        };
        assert_eq!(background, Color::linear_rgb(128. / 255., 0., 0.));
        for _ in 0..5 {
            app.world_mut()
                .resource_mut::<Playback>()
                .logos
                .as_mut()
                .unwrap()
                .step(false);
        }
        app.world_mut().run_system_once(update).unwrap();
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        app.insert_resource(Playback::from_assets(Some(BootAssets::default()))?);
        app.world_mut().run_system_once(update).unwrap();
        assert!(!app.world().get::<Camera>(camera).unwrap().is_active);
        Ok(())
    }
}
